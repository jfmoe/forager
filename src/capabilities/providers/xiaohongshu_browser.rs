//! The `xiaohongshu_browser` route: Xiaohongshu search, note, and comment pages, read in the
//! headless Managed Browser with the logged-in Xiaohongshu Browser Profile.
//!
//! The page logic of each operation has two parts. Driving the page navigates, types, clicks,
//! and scrolls with native input, and observes what the page itself requested, received, and
//! rendered. Judging the observation classifies the page facts, checks that the responses
//! answer the request, and decodes them into items; it never touches a browser.

use chrono::{DateTime, FixedOffset, SecondsFormat};
#[cfg(test)]
use serde::Deserialize;

use crate::catalog::ProviderId;
use crate::config::BrowserRouteRuntimeConfig;
use crate::net::AttemptFailure;
use crate::providers::browser_route::{BrowserRoute, single_attempt};
use crate::providers::execution::ExecutionSettings;
use crate::rate_limit::RateLimiter;
use crate::redact::CREDENTIAL_MASK;
use crate::types::{
    AccessToken, AttemptErrorKind, AttemptTarget, Deadline, Platform, XiaohongshuRef,
};

const ROUTE: ProviderId = ProviderId::XiaohongshuBrowser;
// The login wall's own notice, such as `登录后查看搜索结果`; a bare `登录` would also match a
// block notice that mentions logging in.
const LOGIN_NOTICES: [&str; 1] = ["登录后查看"];
const BLOCK_NOTICES: [&str; 2] = ["安全限制", "访问链接异常"];
const BLOCK_CODES: [&str; 2] = ["300031", "300017"];
const RISK_CONTROL_STATUS: u16 = 461;
const SITE_HOST: &str = "www.xiaohongshu.com";
/// The site's home page: where a search starts, and where a logged-out visitor can log in.
const HOME_URL: &str = "https://www.xiaohongshu.com/explore";
/// The page's own account request, which says whether the session is a guest's.
const ACCOUNT_PATH: &str = "/api/sns/web/v2/user/me";
const BEIJING_SECONDS: i32 = 8 * 3600;

#[path = "xiaohongshu_browser_comments.rs"]
mod comments;
#[path = "xiaohongshu_browser_login.rs"]
mod login;
#[path = "xiaohongshu_browser_note.rs"]
mod note;
#[path = "xiaohongshu_browser_page.rs"]
mod page;
#[path = "xiaohongshu_browser_search.rs"]
mod search;

pub(crate) use login::LOGIN_PAGE;
pub(crate) use note::fetch_support;
pub(crate) use search::search_support;

pub(crate) struct XiaohongshuBrowser {
    browser: BrowserRoute,
}

impl XiaohongshuBrowser {
    pub(crate) fn new(
        config: BrowserRouteRuntimeConfig,
        limiter: RateLimiter,
        deadline: Deadline,
    ) -> Self {
        Self {
            browser: BrowserRoute::new(ROUTE, config, limiter, deadline),
        }
    }

    // The route never retries: repeating a blocked page pushes the account further into risk
    // control.
    fn settings(&self, operation: &'static str) -> ExecutionSettings {
        self.browser.settings(
            AttemptTarget::platform(Platform::Xiaohongshu.as_str(), operation),
            single_attempt(),
        )
    }
}

/// Whether `url` is on a Xiaohongshu host, where the page's own API requests go.
fn on_site(url: &reqwest::Url) -> bool {
    url.host_str()
        .is_some_and(|host| host == "xiaohongshu.com" || host.ends_with(".xiaohongshu.com"))
}

fn runtime(message: String) -> AttemptFailure {
    AttemptFailure {
        kind: AttemptErrorKind::Runtime,
        status: None,
        message,
    }
}

/// Masks the access token of the request wherever an attempt message repeats it: a browser
/// error can quote the note URL, a page script error can quote page state, and a redirect URL
/// can carry the token percent-encoded once or twice. Only the `=` padding changes under URL
/// encoding, so the token without it is masked.
fn without_token(mut failure: AttemptFailure, token: &AccessToken) -> AttemptFailure {
    let body = token.as_str().trim_end_matches('=');
    let secret = if body.is_empty() {
        token.as_str()
    } else {
        body
    };
    failure.message = failure.message.replace(secret, CREDENTIAL_MASK);
    failure
}

/// Where the page ended up and what the site showed there.
#[derive(Debug, Default)]
#[cfg_attr(test, derive(Deserialize))]
#[cfg_attr(test, serde(default))]
struct PageFacts {
    /// The final page URL, without any access token value.
    url: String,
    title: String,
    /// Whether the site treats the session as logged out.
    guest: bool,
    /// The `error_code` of a `/404` or `website-login/error` redirect.
    error_code: Option<String>,
    /// A notice the site showed, such as `安全限制`.
    notice: Option<String>,
    /// The status of a Xiaohongshu API call that answered 461.
    blocked_status: Option<u16>,
    /// The network error code, such as `ERR_CONNECTION_CLOSED`, while the browser shows its own
    /// error page instead of the Xiaohongshu page.
    load_error: Option<String>,
}

/// Fails on the facts that end any Xiaohongshu page read: a logged-out session and risk control
/// are Auth; the site's own block pages are Parameter, with the message `blocked` builds from
/// the error code and the notice.
fn classify_blocks(
    facts: &PageFacts,
    blocked: impl FnOnce(&str, &str) -> String,
) -> Result<(), AttemptFailure> {
    let notice = facts.notice.as_deref().map(str::trim).unwrap_or_default();
    if facts.guest || LOGIN_NOTICES.iter().any(|login| notice.contains(login)) {
        return Err(AttemptFailure {
            kind: AttemptErrorKind::Auth,
            status: None,
            message: "Xiaohongshu treats the browser session as logged out; run `forager browser login xiaohongshu`, then retry".into(),
        });
    }
    if facts.blocked_status == Some(RISK_CONTROL_STATUS) {
        return Err(AttemptFailure {
            kind: AttemptErrorKind::Auth,
            status: Some(RISK_CONTROL_STATUS),
            message: "Xiaohongshu answered HTTP 461 and wants a verification; run `forager browser login xiaohongshu` and complete any check the site shows, then retry".into(),
        });
    }
    let code = facts.error_code.as_deref().map(str::trim);
    if code.is_some_and(|code| BLOCK_CODES.contains(&code))
        || BLOCK_NOTICES.iter().any(|block| notice.contains(block))
    {
        return Err(AttemptFailure {
            kind: AttemptErrorKind::Parameter,
            status: None,
            message: blocked(
                code.unwrap_or("no code"),
                if notice.is_empty() {
                    "no notice"
                } else {
                    notice
                },
            ),
        });
    }
    Ok(())
}

/// Fails a read that ended before it had what it needed: the browser never loaded the page by the
/// deadline (Network), the deadline passed on the expected page (Timeout, with
/// `timeout_message`), or the page logic stopped at another page (Runtime). Returns `None` for a
/// read that stopped on the expected page before the deadline; the caller knows why.
fn unfinished_read(
    facts: &PageFacts,
    timed_out: bool,
    on_expected_page: bool,
    timeout_message: impl FnOnce() -> String,
) -> Option<AttemptFailure> {
    if timed_out && let Some(failure) = load_failure(facts) {
        return Some(failure);
    }
    if !on_expected_page {
        return Some(unexpected_page(facts));
    }
    timed_out.then(|| AttemptFailure {
        kind: AttemptErrorKind::Timeout,
        status: None,
        message: timeout_message(),
    })
}

/// The browser still showed its own error page at the read deadline: the page never loaded, so
/// no response was due. The browser reloads its error page by itself, which is why the page
/// logic keeps waiting until the deadline.
fn load_failure(facts: &PageFacts) -> Option<AttemptFailure> {
    let code = facts.load_error.as_deref()?.trim();
    Some(AttemptFailure {
        kind: AttemptErrorKind::Network,
        status: None,
        message: format!(
            "the browser could not load the Xiaohongshu page ({code}) and had not loaded it again by the read deadline; check the network and retry"
        ),
    })
}

fn unexpected_page(facts: &PageFacts) -> AttemptFailure {
    runtime(format!(
        "the Xiaohongshu page logic stopped at an unexpected page: {}",
        describe(facts)
    ))
}

fn describe(facts: &PageFacts) -> String {
    format!("`{}` ({})", facts.title.trim(), facts.url)
}

/// The message of a note page the site blocked: the token may be stale, the note restricted or
/// removed, or the account rate-limited, and the page cannot tell which.
fn note_unavailable(requested: &XiaohongshuRef, code: &str, notice: &str) -> String {
    format!(
        "Xiaohongshu note unavailable: xiaohongshu:{requested} ({code}: {notice}); the access token may be stale, the note restricted or removed, or the account rate-limited"
    )
}

fn is_note_page(url: &str, requested: &XiaohongshuRef) -> bool {
    reqwest::Url::parse(url).is_ok_and(|url| {
        url.host_str() == Some(SITE_HOST)
            && url.path().trim_end_matches('/')
                == format!("/explore/{}", requested.note_id()).as_str()
    })
}

/// Returns a count as Xiaohongshu shows it, such as `1.2万`; the site writes counts as text and
/// occasionally as numbers.
fn count_text(value: Option<&serde_json::Value>) -> Option<String> {
    match value {
        Some(serde_json::Value::String(text)) => Some(text.clone()),
        Some(serde_json::Value::Number(number)) => Some(number.to_string()),
        _ => None,
    }
}

/// Converts milliseconds since the Unix epoch to an ISO 8601 timestamp in Beijing time.
fn beijing_time(milliseconds: i64) -> Option<String> {
    let beijing = FixedOffset::east_opt(BEIJING_SECONDS)?;
    DateTime::from_timestamp_millis(milliseconds).map(|time| {
        time.with_timezone(&beijing)
            .to_rfc3339_opts(SecondsFormat::Secs, false)
    })
}
