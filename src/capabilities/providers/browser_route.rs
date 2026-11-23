//! The browser transport bound to one browser route: the route identity and its declared
//! browser environment, the route configuration (`timeout` and the browser it launches), its
//! access limiter, and the request deadline. Route adapters build execution settings and run
//! their page logic through it; what the page logic reads and how to judge it stay with each
//! route.
//!
//! It also owns the reading session every route's page logic drives a page with, and the
//! preflight check every browser route passes before a platform command enters its route
//! chain: the browser can be found, and its platform's Browser Profile had a login.

use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::catalog::{
    BrowserEnvironment, ProviderId, ProviderTransport, registration, route_platform,
};
use crate::config::{BrowserRouteRuntimeConfig, BrowserRuntimeConfig};
use crate::net::{AttemptFailure, RetryPolicy};
use crate::providers::browser::{
    self, BrowserSession, CaptureFilter, Exchange, Page, WindowMode, runtime,
};
use crate::providers::browser_profile::BrowserProfile;
use crate::providers::execution::ExecutionSettings;
use crate::providers::managed_browser;
use crate::rate_limit::RateLimiter;
use crate::types::{AttemptTarget, Deadline};

// A page that is navigating briefly answers no script.
const MAX_UNANSWERED: u32 = 5;

pub(crate) struct BrowserRoute {
    route: ProviderId,
    environment: BrowserEnvironment,
    config: BrowserRouteRuntimeConfig,
    limiter: RateLimiter,
    deadline: Deadline,
}

impl BrowserRoute {
    /// Binds the transport to `route`, whose registration declares a browser transport.
    pub(crate) fn new(
        route: ProviderId,
        config: BrowserRouteRuntimeConfig,
        limiter: RateLimiter,
        deadline: Deadline,
    ) -> Self {
        let ProviderTransport::Browser(environment) = registration(route).transport else {
            unreachable!("{} registers a browser transport", route.name());
        };
        Self {
            route,
            environment,
            config,
            limiter,
            deadline,
        }
    }

    /// The execution settings of one route operation: each attempt runs one browser command
    /// within the configured timeout and the request deadline.
    pub(crate) fn settings(
        &self,
        target: AttemptTarget,
        retry_policy: RetryPolicy,
    ) -> ExecutionSettings {
        ExecutionSettings {
            provider: self.route.name(),
            target,
            retry_policy,
            deadline: self.deadline,
            attempt_timeout: Duration::from_secs(self.config.timeout_seconds),
            verbose: false,
            timeout_message: "browser command timed out",
            model: None,
            transport: Some("browser"),
            endpoint_host: None,
        }
    }

    /// Runs `work` on the page of one headless browser command within `deadline`, the attempt
    /// deadline, keeping what `capture` names.
    ///
    /// # Errors
    ///
    /// Returns Runtime when the browser or the Browser Profile cannot be found, and otherwise
    /// the failure of [`browser::run`].
    pub(crate) async fn read<T>(
        &self,
        capture: CaptureFilter,
        deadline: Deadline,
        work: impl AsyncFnOnce(&mut Page) -> Result<T, AttemptFailure>,
    ) -> Result<T, AttemptFailure> {
        let executable = managed_browser::locate(&self.config.browser)
            .map_err(|error| runtime(error.to_string()))?;
        let profile = BrowserProfile::named(route_platform(self.route).as_str())
            .map_err(|error| runtime(error.to_string()))?;
        let session = BrowserSession {
            executable: &executable,
            profile: &profile,
            language: self.environment.language,
            window: WindowMode::Headless,
            capture,
        };
        let (value, _profile) = browser::run(&session, &self.limiter, deadline, work).await?;
        Ok(value)
    }
}

/// One operation's read of a page: it navigates, keeps every exchange capture holds, and
/// evaluates the page logic's read-only scripts, tolerating the moments a navigating page
/// answers none. What the scripts read and how to judge it stay with the route.
pub(crate) struct PageReading<'p> {
    page: &'p mut Page,
    requested: String,
    exchanges: Vec<Exchange>,
    /// The network error of a navigation that never loaded a document.
    navigation_error: Option<String>,
    unanswered: u32,
}

impl<'p> PageReading<'p> {
    pub(crate) fn new(page: &'p mut Page) -> Self {
        Self {
            page,
            requested: String::new(),
            exchanges: Vec::new(),
            navigation_error: None,
            unanswered: 0,
        }
    }

    pub(crate) fn page(&mut self) -> &mut Page {
        self.page
    }

    /// The URL of the last navigation.
    pub(crate) fn requested(&self) -> &str {
        &self.requested
    }

    /// The network error of the last navigation, when it never loaded a document.
    pub(crate) fn navigation_error(&self) -> Option<&str> {
        self.navigation_error.as_deref()
    }

    /// Every exchange taken so far, in the order the responses ended.
    pub(crate) fn exchanges(&self) -> &[Exchange] {
        &self.exchanges
    }

    pub(crate) fn into_exchanges(self) -> Vec<Exchange> {
        self.exchanges
    }

    /// Opens `url` and waits until its document has loaded, at most until the read deadline.
    ///
    /// # Errors
    ///
    /// Returns a Runtime failure when the browser refuses the navigation command.
    pub(crate) async fn navigate(&mut self, url: &str) -> Result<(), AttemptFailure> {
        url.clone_into(&mut self.requested);
        let navigation = self.page.navigate(url).await?;
        self.navigation_error = navigation
            .error
            .filter(|_| !navigation.loaded)
            .map(|error| error.trim_start_matches("net::").to_owned());
        Ok(())
    }

    /// Takes what capture holds and returns the exchanges it added.
    ///
    /// # Errors
    ///
    /// Returns a Runtime failure when a kept response body exceeded the body limit.
    pub(crate) fn capture(&mut self) -> Result<&[Exchange], AttemptFailure> {
        let taken = self.exchanges.len();
        self.exchanges.extend(self.page.take_exchanges()?);
        Ok(&self.exchanges[taken..])
    }

    /// Evaluates a read-only `script`; `None` while the page briefly answers no script, as it
    /// does while it navigates.
    ///
    /// # Errors
    ///
    /// Returns a transport failure, or a script failure that lasted several reads.
    pub(crate) async fn evaluate<T: DeserializeOwned>(
        &mut self,
        script: &str,
    ) -> Result<Option<T>, AttemptFailure> {
        match self.page.evaluate::<T>(script).await {
            Ok(value) => {
                self.unanswered = 0;
                Ok(Some(value))
            }
            Err(failure) if self.page.closed() || self.unanswered >= MAX_UNANSWERED => Err(failure),
            Err(_) => {
                self.unanswered += 1;
                Ok(None)
            }
        }
    }

    /// The load error of a page at `url` that shows none itself: a navigation that failed
    /// before the browser showed its error page leaves the tab blank, where only the navigation
    /// result names the error.
    pub(crate) fn blank_page_error(&self, url: &str) -> Option<String> {
        self.navigation_error
            .clone()
            .filter(|_| !url.starts_with("http"))
    }
}

/// Checks, without launching anything, that a browser route can run: the browser can be found,
/// and its platform's Browser Profile had a login. The login may have expired
/// since; the page reports that.
///
/// # Errors
///
/// Returns the reason with the command that fixes it.
pub(crate) fn preflight(route: ProviderId, browser: &BrowserRuntimeConfig) -> Result<(), String> {
    managed_browser::locate(browser).map_err(|error| error.to_string())?;
    let platform = route_platform(route).as_str();
    let profile = BrowserProfile::named(platform).map_err(|error| error.to_string())?;
    if profile.has_logged_in() {
        Ok(())
    } else {
        Err(format!(
            "the {platform} Browser Profile has never been logged in; run `forager browser login {platform}`"
        ))
    }
}

/// The retry policy of an operation that runs exactly one attempt.
pub(crate) fn single_attempt() -> RetryPolicy {
    RetryPolicy::new(1, 1.0, Duration::ZERO)
}
