//! The browser transport bound to one browser route: the route identity and its declared
//! browser environment, the route configuration (`timeout` and the browser it launches), its
//! access limiter, and the request deadline. Route adapters build execution settings and run
//! their page logic through it; what the page logic reads and how to judge it stay with each
//! route.
//!
//! It also owns the preflight check every browser route passes before a platform command
//! enters its route chain: the browser can be found, and a profile that needs a login had one.

use std::time::Duration;

use crate::catalog::{BrowserEnvironment, ProviderId, ProviderTransport, registration};
use crate::config::{BrowserRouteRuntimeConfig, BrowserRuntimeConfig};
use crate::net::{AttemptFailure, RetryPolicy};
use crate::providers::browser::{self, BrowserSession, CaptureFilter, Display, Page, runtime};
use crate::providers::browser_profile::BrowserProfile;
use crate::providers::execution::ExecutionSettings;
use crate::providers::managed_browser;
use crate::rate_limit::RateLimiter;
use crate::types::{AttemptTarget, Deadline};

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
        let profile = BrowserProfile::named(self.environment.profile.as_str())
            .map_err(|error| runtime(error.to_string()))?;
        let session = BrowserSession {
            executable: &executable,
            profile: &profile,
            language: self.environment.language,
            display: Display::Headless,
            capture,
        };
        let (value, _profile) = browser::run(&session, &self.limiter, deadline, work).await?;
        Ok(value)
    }
}

/// Checks, without launching anything, that a browser route in `environment` can run: the
/// browser can be found, and a profile that needs a login had one. The login may have expired
/// since; the page reports that.
///
/// # Errors
///
/// Returns the reason with the command that fixes it.
pub(crate) fn preflight(
    environment: &BrowserEnvironment,
    browser: &BrowserRuntimeConfig,
) -> Result<(), String> {
    managed_browser::locate(browser).map_err(|error| error.to_string())?;
    if !environment.requires_login {
        return Ok(());
    }
    let platform = environment.profile.as_str();
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
