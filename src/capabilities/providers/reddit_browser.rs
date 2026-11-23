//! The `reddit_browser` route: Reddit's own pages, read in the headless Managed Browser with the
//! logged-in Reddit Browser Profile.
//!
//! Each operation only navigates to a real page URL and evaluates a read-only script in the
//! page. The script projects what the page shows into compact JSON: the page facts (title, final
//! URL, the login attribute, the verification markers, a load error, and the operation's own
//! markers) and the data records (attribute values of the elements the operation reads). It
//! selects and copies, never judges, requests, or changes the page. Rust holds every judgement:
//! it decodes the projection, recognizes a verification page or a logged-out session, and maps
//! the records to platform items, without a browser.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use reqwest::Url;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::catalog::ProviderId;
use crate::config::BrowserRouteRuntimeConfig;
use crate::net::AttemptFailure;
use crate::providers::browser::{CaptureLevel, Exchange, RequestMethod, runtime};
use crate::providers::browser_route::{BrowserRoute, PageReading, single_attempt};
use crate::providers::execution::ExecutionSettings;
use crate::providers::route_declaration::{PlatformRoute, RouteAdapter, RouteDeclaration};
use crate::providers::shared::utc_second;
use crate::providers::{PlatformFetch, PlatformSearch, route_limiter};
use crate::types::{
    AttemptErrorKind, AttemptTarget, Deadline, Platform, PlatformFetchOutcome,
    PlatformFetchRequest, PlatformSearchOutcome, PlatformSearchRequest, ProviderError,
};

const ROUTE: ProviderId = ProviderId::RedditBrowser;
const SITE_HOST: &str = "www.reddit.com";
const SITE_ORIGIN: &str = "https://www.reddit.com";
/// The title of the site's verification page, which a browser it distrusts gets instead.
const VERIFICATION_TITLE: &str = "Prove your humanity";
/// The document statuses with which Reddit refuses a session or a network.
const REFUSAL_STATUSES: [u16; 2] = [403, 429];
const POLL: Duration = Duration::from_millis(500);

#[path = "reddit_browser_login.rs"]
mod login;
#[path = "reddit_markup.rs"]
mod markup;
#[path = "reddit_browser_post.rs"]
mod post;
#[path = "reddit_browser_search.rs"]
mod search;
#[path = "reddit_browser_thread.rs"]
mod thread;

pub(crate) use login::LOGIN_PAGE;
use post::fetch_support;
use search::search_support;

pub(crate) struct RedditBrowser {
    browser: BrowserRoute,
}

/// The route's declaration: Search and Fetch of the catalog, and the login into the Reddit
/// Browser Profile.
pub(crate) const DECLARATION: RouteDeclaration<BrowserRouteRuntimeConfig> = RouteDeclaration {
    build: |config, context| Box::new(RedditBrowser::build(config, context.deadline)),
    adapter: RouteAdapter {
        search_support: Some(search_support),
        fetch_support: Some(fetch_support),
        login_page: Some(LOGIN_PAGE),
    },
};

impl PlatformRoute for RedditBrowser {
    fn into_search(self: Box<Self>) -> Option<Box<dyn PlatformSearch>> {
        Some(self)
    }

    fn into_fetch(self: Box<Self>) -> Option<Box<dyn PlatformFetch>> {
        Some(self)
    }
}

impl PlatformSearch for RedditBrowser {
    fn search<'a>(
        &'a self,
        request: &'a PlatformSearchRequest,
    ) -> Pin<Box<dyn Future<Output = Result<PlatformSearchOutcome, ProviderError>> + Send + 'a>>
    {
        Box::pin(RedditBrowser::search(self, request))
    }
}

impl PlatformFetch for RedditBrowser {
    fn fetch<'a>(
        &'a self,
        request: &'a PlatformFetchRequest,
    ) -> Pin<Box<dyn Future<Output = Result<PlatformFetchOutcome, ProviderError>> + Send + 'a>>
    {
        Box::pin(RedditBrowser::fetch(self, request))
    }
}

impl RedditBrowser {
    /// Builds the route under the access policy its registration declares. A browser route
    /// sends no HTTP request of its own, so the client and the retry policy do not reach it.
    fn build(config: BrowserRouteRuntimeConfig, deadline: Deadline) -> Self {
        let limiter =
            route_limiter(ROUTE).expect("reddit_browser registration declares an access policy");
        Self {
            browser: BrowserRoute::new(ROUTE, config, limiter, deadline),
        }
    }

    // The route never retries: repeating a page the site distrusts pushes the account further
    // into its risk scoring.
    fn settings(&self, operation: &'static str) -> ExecutionSettings {
        self.browser.settings(
            AttemptTarget::platform(Platform::Reddit.as_str(), operation),
            single_attempt(),
        )
    }
}

/// Keeps the status of every request to the site, which carries the document status; no body.
fn capture(url: &Url) -> CaptureLevel {
    if url.host_str() == Some(SITE_HOST) {
        CaptureLevel::Status
    } else {
        CaptureLevel::Ignore
    }
}

/// What every projection reports about the page it read. Every field must be present, a
/// nullable one as `null`, so a script that drifted from this shape fails to decode.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct PageFacts {
    /// The final page URL, or the URL the browser tried when it shows its own error page.
    url: String,
    title: String,
    /// `document.readyState`.
    ready_state: String,
    /// Whether the page has the site's `shreddit-app` element.
    app: bool,
    /// The raw `user-logged-in` attribute of `shreddit-app`.
    #[serde(deserialize_with = "Option::deserialize")]
    user_logged_in: Option<String>,
    /// Whether a form holds a reCAPTCHA widget, as the verification page does.
    recaptcha_form: bool,
    /// Whether the page has a `js_challenge` field.
    js_challenge: bool,
    /// The network error code, such as `ERR_CONNECTION_CLOSED`, while the browser shows its own
    /// error page instead of the Reddit page.
    #[serde(deserialize_with = "Option::deserialize")]
    load_error: Option<String>,
}

impl PageFacts {
    fn logged_in(&self) -> bool {
        self.app && self.user_logged_in.as_deref() == Some("true")
    }

    /// Whether the document finished loading the Reddit page itself.
    fn loaded(&self) -> bool {
        self.ready_state == "complete" && self.load_error.is_none()
    }

    fn verification(&self) -> bool {
        self.recaptcha_form || self.js_challenge || self.title.contains(VERIFICATION_TITLE)
    }
}

// The facts every projection starts with, as `page`. The browser's own error page for a
// navigation that failed has the location chrome-error://chromewebdata/; the navigation entry
// keeps the URL it tried.
const PAGE_FACTS: &str = r"
  const loadError = location.protocol === 'chrome-error:'
    ? ((document.querySelector('.error-code') || {}).textContent || '').trim() || 'unknown network error'
    : null;
  const navigation = performance.getEntriesByType('navigation')[0];
  const app = document.querySelector('shreddit-app');
  const page = {
    url: loadError && navigation ? navigation.name : location.href,
    title: document.title,
    ready_state: document.readyState,
    app: app !== null,
    user_logged_in: app ? app.getAttribute('user-logged-in') : null,
    recaptcha_form: document.querySelector('form .g-recaptcha') !== null,
    js_challenge: document.querySelector('input[name=js_challenge]') !== null,
    load_error: loadError,
  };";

/// Wraps the read-only body of an operation's projection, with the page facts in scope as
/// `page`.
fn projection_script(body: &str) -> String {
    format!("(() => {{{PAGE_FACTS}\n{body}\n}})()")
}

/// A projection: the page facts and the operation's own markers and records.
trait Projection: DeserializeOwned + Default {
    fn page_mut(&mut self) -> &mut PageFacts;
}

/// What one read of a page observed.
#[derive(Debug)]
struct Observation<P> {
    /// The last projection; a search's holds every result any projection showed.
    projection: P,
    document_status: Option<u16>,
    /// The read deadline passed before the page reached a final state.
    timed_out: bool,
}

impl<P: Projection> Observation<P> {
    /// What a read observed that ended at the projection `last`, or, without one, at the
    /// requested page with the navigation's network error.
    fn new(
        last: Option<P>,
        reading: &PageReading<'_>,
        document_status: Option<u16>,
        timed_out: bool,
    ) -> Self {
        let projection = last.unwrap_or_else(|| {
            let mut unread = P::default();
            *unread.page_mut() = PageFacts {
                url: reading.requested().to_owned(),
                load_error: reading.navigation_error().map(str::to_owned),
                ..PageFacts::default()
            };
            unread
        });
        Self {
            projection,
            document_status,
            timed_out,
        }
    }
}

/// Fails on the facts that end any Reddit page read with Auth: a verification page, a document
/// the site refused with 403 or 429, and a page that is not logged in. The route never answers
/// a check or retries.
fn blocked(facts: &PageFacts, document_status: Option<u16>) -> Option<AttemptFailure> {
    if facts.verification() {
        return Some(AttemptFailure {
            kind: AttemptErrorKind::Auth,
            status: None,
            message: format!(
                "Reddit showed a verification page instead of {}; forager never answers one, so stop, run `forager browser login reddit` and complete any check the site shows in its window, then retry later",
                describe(facts)
            ),
        });
    }
    if let Some(status) = document_status.filter(|status| REFUSAL_STATUSES.contains(status)) {
        return Some(AttemptFailure {
            kind: AttemptErrorKind::Auth,
            status: Some(status),
            message: format!(
                "Reddit answered the page with HTTP {status}; stop and wait, or run `forager browser login reddit` and complete any check the site shows in its window, then retry later"
            ),
        });
    }
    (facts.app && !facts.logged_in()).then(|| AttemptFailure {
        kind: AttemptErrorKind::Auth,
        status: None,
        message: "Reddit treats the browser session as logged out; run `forager browser login reddit`, then retry".into(),
    })
}

/// Whether a page that shows none of the operation's content reached a final state, which
/// renders the content no later: a block, or a loaded page of a logged-in session or of a
/// document the site failed to serve.
fn ended_without_content(facts: &PageFacts, document_status: Option<u16>) -> bool {
    blocked(facts, document_status).is_some()
        || (facts.loaded() && (facts.logged_in() || server_error(document_status).is_some()))
}

/// Fails a page that shows none of the operation's content when the site failed to serve its
/// document: a site failure says nothing about the content.
fn server_failure(facts: &PageFacts, document_status: Option<u16>) -> Option<AttemptFailure> {
    server_error(document_status).map(|status| AttemptFailure {
        kind: AttemptErrorKind::Runtime,
        status: Some(status),
        message: format!(
            "Reddit answered {} with HTTP {status} instead of the page; the site may be failing, so retry later",
            facts.url
        ),
    })
}

/// The status of a document the site failed to serve.
fn server_error(document_status: Option<u16>) -> Option<u16> {
    document_status.filter(|status| *status >= 500)
}

/// Fails a read whose deadline passed before the page reached a final state: the browser never
/// loaded the page (Network), the page stayed the expected one (Timeout, with
/// `timeout_message`), or the read stopped at another page (Runtime).
fn unfinished(facts: &PageFacts, on_expected_page: bool, timeout_message: &str) -> AttemptFailure {
    if let Some(code) = &facts.load_error {
        return AttemptFailure {
            kind: AttemptErrorKind::Network,
            status: None,
            message: format!(
                "the browser could not load the Reddit page {} ({}) and had not loaded it again by the read deadline; check the network and retry",
                facts.url,
                code.trim()
            ),
        };
    }
    if !on_expected_page {
        return unexpected_page(facts);
    }
    AttemptFailure {
        kind: AttemptErrorKind::Timeout,
        status: None,
        message: format!("{timeout_message} on {}", describe(facts)),
    }
}

fn unexpected_page(facts: &PageFacts) -> AttemptFailure {
    runtime(format!(
        "the Reddit page logic stopped at an unexpected page: {}",
        describe(facts)
    ))
}

fn describe(facts: &PageFacts) -> String {
    format!("`{}` ({})", facts.title.trim(), facts.url)
}

/// Formats a site timestamp such as `2026-09-30T17:37:11.509000+0000` as a platform item
/// timestamp.
fn site_timestamp(value: &str) -> Option<String> {
    chrono::DateTime::parse_from_str(value.trim(), "%Y-%m-%dT%H:%M:%S%.f%z")
        .ok()
        .map(|time| utc_second(time.to_utc()))
}

/// The site URL of a site-relative `path`, such as the permalink `/r/rust/comments/<id>/<slug>/`.
fn site_url(path: &str) -> Option<String> {
    path.starts_with('/')
        .then(|| format!("{SITE_ORIGIN}{path}"))
}

/// The path of a site URL without its trailing `/`, in lower case: the site redirects a
/// subreddit path to its own capitalization.
fn site_path(url: &Url) -> Option<String> {
    (url.host_str() == Some(SITE_HOST))
        .then(|| url.path().trim_end_matches('/').to_ascii_lowercase())
}

/// Whether `url` is the site page at `path`.
fn is_page(url: &str, path: &str) -> bool {
    Url::parse(url)
        .ok()
        .and_then(|url| site_path(&url))
        .is_some_and(|shown| shown == path.trim_end_matches('/').to_ascii_lowercase())
}

/// The status of the page's document: the last site GET whose path is the requested page or
/// the page the browser ended at, which a redirect leads to.
fn document_status(exchanges: &[Exchange], requested_path: &str, final_url: &str) -> Option<u16> {
    let wanted = [
        Some(requested_path.trim_end_matches('/').to_ascii_lowercase()),
        Url::parse(final_url).ok().and_then(|url| site_path(&url)),
    ];
    exchanges
        .iter()
        .rev()
        .filter(|exchange| exchange.method == RequestMethod::Get)
        .find(|exchange| site_path(&exchange.url).is_some_and(|path| wanted.contains(&Some(path))))
        .and_then(|exchange| exchange.status)
}

/// Takes what capture holds and evaluates the projection `script`; `None` while the page
/// briefly answers no script, as it does while it navigates.
///
/// # Errors
///
/// Returns a transport failure, or a script failure that lasted several reads.
async fn project<T: Projection>(
    reading: &mut PageReading<'_>,
    script: &str,
) -> Result<Option<T>, AttemptFailure> {
    reading.capture()?;
    let Some(mut projection) = reading.evaluate::<T>(script).await? else {
        return Ok(None);
    };
    let page = projection.page_mut();
    if page.load_error.is_none() {
        page.load_error = reading.blank_page_error(&page.url);
    }
    Ok(Some(projection))
}

#[cfg(test)]
mod tests {
    use reqwest::Url;
    #[cfg(unix)]
    use serde_json::Value;

    use super::document_status;
    use crate::providers::browser::{Exchange, RequestMethod, ResponseBody};
    use crate::samples::sample;

    /// The stored projection or page `name` of `tests/samples/reddit_browser/`.
    pub(super) fn stored(name: &str) -> String {
        sample(&format!("reddit_browser/{name}"))
    }

    fn exchange(url: &str, status: u16) -> Exchange {
        Exchange {
            sequence: 0,
            method: RequestMethod::Get,
            url: Url::parse(url).expect("site URL"),
            status: Some(status),
            request_body: None,
            body: ResponseBody::NotKept,
        }
    }

    #[test]
    fn the_document_status_is_the_last_get_of_the_requested_page_or_its_redirect_target() {
        let exchanges = [
            exchange("https://www.reddit.com/r/Rust/search/?q=tokio", 301),
            exchange("https://www.reddit.com/r/rust/search/?q=tokio", 403),
            exchange(
                "https://www.reddit.com/svc/shreddit/r/rust/search/?cursor=x",
                200,
            ),
            exchange("https://www.reddit.com/svc/shreddit/events", 200),
        ];

        let status = document_status(
            &exchanges,
            "/r/Rust/search/",
            "https://www.reddit.com/r/rust/search/?q=tokio",
        );

        assert_eq!(status, Some(403));
    }

    #[test]
    fn requests_to_other_paths_or_hosts_carry_no_document_status() {
        let exchanges = [
            exchange("https://www.reddit.com/svc/shreddit/search/?q=rust", 429),
            exchange("https://alb.reddit.com/search/", 403),
        ];

        let status = document_status(
            &exchanges,
            "/search/",
            "https://www.reddit.com/search/?q=rust",
        );

        assert_eq!(status, None);
    }

    /// Runs the projection `script` on each local page of `projected` (the path, the sample the
    /// server serves it from, and the stored projection of `tests/samples/reddit_browser/` it must
    /// yield), with the server's origin written as Reddit's; with `FORAGER_WRITE_PROJECTIONS` set,
    /// it writes the stored projections instead.
    #[cfg(unix)]
    pub(super) fn check_projections(
        served: crate::providers::real_browser::FixturePages,
        projected: &[(&str, &str, &str)],
        script: &str,
    ) {
        use crate::providers::real_browser::{Pages, Sandbox, executable};

        let pages = Pages::serving(served);
        let executable = executable();
        for (path, served, expected) in projected {
            let url = pages.url(path);
            let projection: Value = Sandbox::new().run(&executable, async |page| {
                page.navigate(&url).await?;
                page.evaluate(script).await
            });
            let projection = serde_json::to_string_pretty(&projection)
                .expect("a JSON value serializes")
                .replace(pages.origin(), super::SITE_ORIGIN);
            if std::env::var_os("FORAGER_WRITE_PROJECTIONS").is_some() {
                let file = format!(
                    "{}/tests/samples/reddit_browser/{expected}",
                    env!("CARGO_MANIFEST_DIR")
                );
                std::fs::write(&file, format!("{projection}\n")).expect("write the projection");
                continue;
            }
            assert_eq!(
                projection.trim_end(),
                stored(expected).trim_end(),
                "{served}"
            );
        }
    }
}
