//! The `xiaohongshu_browser` route: Xiaohongshu search and note pages, read in the user's own
//! logged-in Chrome through the forager OpenCLI adapter `forager-xhs`.
//!
//! The JavaScript adapter only navigates, hovers, clicks, and scrolls, and reports what the page
//! itself requested, received, and rendered. This route classifies the page facts, checks that
//! the responses answer the request, and decodes them into items.

use std::time::Duration;

use serde::Deserialize;

use crate::catalog::{PlatformOperation, ProviderId, ProviderTransport, registration};
use crate::config::ProcessRouteRuntimeConfig;
use crate::net::{AttemptFailure, RetryPolicy};
use crate::providers::execution::ExecutionSettings;
use crate::providers::opencli::OpenCliCommand;
use crate::rate_limit::RateLimiter;
use crate::redact::CREDENTIAL_MASK;
use crate::types::{AccessToken, AttemptErrorKind, AttemptTarget, Deadline, Platform};

const ROUTE: ProviderId = ProviderId::XiaohongshuBrowser;
const LOGIN_NOTICES: [&str; 2] = ["登录后查看", "登录"];
const BLOCK_NOTICES: [&str; 2] = ["安全限制", "访问链接异常"];
const BLOCK_CODES: [&str; 2] = ["300031", "300017"];
const RISK_CONTROL_STATUS: u16 = 461;

#[path = "xiaohongshu_browser_note.rs"]
mod note;
#[path = "xiaohongshu_browser_search.rs"]
mod search;

pub(crate) use note::fetch_support;
pub(crate) use search::search_support;

pub(crate) struct XiaohongshuBrowser {
    config: ProcessRouteRuntimeConfig,
    limiter: RateLimiter,
    deadline: Deadline,
}

impl XiaohongshuBrowser {
    pub(crate) fn new(
        config: ProcessRouteRuntimeConfig,
        limiter: RateLimiter,
        deadline: Deadline,
    ) -> Self {
        Self {
            config,
            limiter,
            deadline,
        }
    }

    fn command(
        &self,
        command: &'static str,
        options: Vec<(&'static str, String)>,
    ) -> OpenCliCommand<'_> {
        let ProviderTransport::OpenCli(adapter) = registration(ROUTE).transport else {
            unreachable!("xiaohongshu_browser registers an OpenCLI transport");
        };
        OpenCliCommand {
            executable: &self.config.command,
            adapter,
            command,
            options,
        }
    }

    // The route never retries: repeating a blocked page pushes the account further into risk
    // control.
    fn settings(&self, operation: PlatformOperation) -> ExecutionSettings {
        ExecutionSettings {
            provider: ROUTE.name(),
            target: AttemptTarget::platform(Platform::Xiaohongshu.as_str(), operation.as_str()),
            retry_policy: RetryPolicy::new(1, 1.0, Duration::ZERO),
            deadline: self.deadline,
            attempt_timeout: Duration::from_secs(self.config.timeout_seconds),
            verbose: false,
            timeout_message: "OpenCLI command timed out",
            model: None,
            transport: Some("process"),
            endpoint_host: None,
            breaker_event: None,
        }
    }
}

fn runtime(message: String) -> AttemptFailure {
    AttemptFailure {
        kind: AttemptErrorKind::Runtime,
        status: None,
        message,
    }
}

/// Masks the access token of the request wherever an attempt message repeats it: OpenCLI
/// stderr can echo the `--xsec-token` argument, a decoding error can quote page facts, and a
/// redirect URL can carry the token percent-encoded once or twice. Only the `=` padding changes
/// under URL encoding, so the token without it is masked.
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
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
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
    /// The network error code, such as `ERR_CONNECTION_CLOSED`, while Chrome shows its own
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
            message: "Xiaohongshu treats the browser session as logged out; log in to xiaohongshu.com in the Chrome that OpenCLI drives, then retry".into(),
        });
    }
    if facts.blocked_status == Some(RISK_CONTROL_STATUS) {
        return Err(AttemptFailure {
            kind: AttemptErrorKind::Auth,
            status: Some(RISK_CONTROL_STATUS),
            message: "Xiaohongshu answered HTTP 461 and wants a verification; open xiaohongshu.com in Chrome, complete any check it shows, then retry".into(),
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

/// Fails a read whose deadline passed while Chrome still showed its own error page: the page
/// never loaded, so no response was due. Chrome reloads its error page by itself, which is why
/// the adapter keeps waiting until the deadline.
fn load_failure(facts: &PageFacts) -> Option<AttemptFailure> {
    let code = facts.load_error.as_deref()?.trim();
    Some(AttemptFailure {
        kind: AttemptErrorKind::Network,
        status: None,
        message: format!(
            "Chrome could not load the Xiaohongshu page ({code}) and had not loaded it again by the read deadline; check the network and retry"
        ),
    })
}

fn describe(facts: &PageFacts) -> String {
    format!("`{}` ({})", facts.title.trim(), facts.url)
}
