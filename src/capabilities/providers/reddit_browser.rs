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
use crate::providers::shared::utc_second;
use crate::rate_limit::RateLimiter;
use crate::types::{AttemptErrorKind, AttemptTarget, Deadline, Platform};

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
pub(crate) use post::fetch_support;
pub(crate) use search::search_support;

pub(crate) struct RedditBrowser {
    browser: BrowserRoute,
}

impl RedditBrowser {
    pub(crate) fn new(
        config: BrowserRouteRuntimeConfig,
        limiter: RateLimiter,
        deadline: Deadline,
    ) -> Self {
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
trait Projection: DeserializeOwned {
    fn page_mut(&mut self) -> &mut PageFacts;
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

/// Fails a read whose deadline passed before the page reached a final state: the browser never
/// loaded the page (Network), the page stayed the expected one (Timeout, with
/// `timeout_message`), or the read stopped at another page (Runtime).
fn unfinished(
    facts: &PageFacts,
    on_expected_page: bool,
    timeout_message: impl FnOnce() -> String,
) -> AttemptFailure {
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
        message: format!("{} on {}", timeout_message(), describe(facts)),
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

/// The facts of a read that never had a projection: the requested page, with the navigation's
/// network error.
fn unread(reading: &PageReading<'_>) -> PageFacts {
    PageFacts {
        url: reading.requested().to_owned(),
        load_error: reading.navigation_error().map(str::to_owned),
        ..PageFacts::default()
    }
}

#[cfg(test)]
mod tests {
    use reqwest::Url;
    #[cfg(unix)]
    use serde_json::Value;

    use super::document_status;
    use crate::providers::browser::{Exchange, RequestMethod, ResponseBody};

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

    /// Runs the projection `script` on each local page of `projected` (the path, the file the
    /// server serves it from, and the projection fixture it must yield), with the server's origin
    /// written as Reddit's; with `FORAGER_WRITE_PROJECTIONS` set, it writes the fixtures instead.
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
            let file = format!("{}/tests/fixtures/{expected}", env!("CARGO_MANIFEST_DIR"));
            if std::env::var_os("FORAGER_WRITE_PROJECTIONS").is_some() {
                std::fs::write(&file, format!("{projection}\n")).expect("write the projection");
                continue;
            }
            let fixture = std::fs::read_to_string(&file)
                .unwrap_or_else(|error| panic!("read {file}: {error}"));
            assert_eq!(projection.trim_end(), fixture.trim_end(), "{served}");
        }
    }
}
