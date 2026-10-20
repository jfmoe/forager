//! The `gemini_browser` provider: Gemini Deep Research conversations, started and read in the
//! user's own logged-in Chrome through the forager OpenCLI adapter `forager-gemini`.
//!
//! The JavaScript adapter only operates pages as a user would and reports what the page itself
//! received. The shared delegated research flow runs the commands; this provider classifies the
//! page facts, checks that the page shows the expected conversation, and decodes the responses.
//! Only `forager gemini` commands run it; no chain does.

use std::cell::OnceCell;

use serde::Deserialize;

use crate::catalog::ProviderId;
use crate::config::ProcessRouteRuntimeConfig;
use crate::net::{AttemptFailure, RetryPolicy, truncate_message};
use crate::providers::delegated_research::{
    DelegatedResearch, ResearchConversation, ResearchSite, StartFailure,
};
use crate::rate_limit::RateLimiter;
use crate::types::{
    AttemptErrorKind, Deadline, GEMINI_RESEARCH_RESULT, GEMINI_RESEARCH_START,
    GeminiConversationId, GeminiPlan, GeminiResearchFailure, GeminiResearchResult,
    GeminiResearchStarted, GeminiResearchState, ProviderError,
};

#[path = "gemini_browser_decode.rs"]
mod decode;
#[path = "gemini_browser_start.rs"]
mod start;

const ROUTE: ProviderId = ProviderId::GeminiBrowser;
const CONVERSATION_RPC: &str = "hNvQHb";

pub(crate) struct GeminiBrowser {
    research: DelegatedResearch<Gemini>,
}

impl GeminiBrowser {
    pub(crate) fn new(
        config: ProcessRouteRuntimeConfig,
        limiter: RateLimiter,
        retry_policy: RetryPolicy,
        deadline: Deadline,
    ) -> Self {
        Self {
            research: DelegatedResearch::new(config, limiter, retry_policy, deadline),
        }
    }

    /// Reads where the conversation's newest research stands from the first conversation
    /// response the page receives.
    pub(crate) async fn result(
        &self,
        conversation: &GeminiConversationId,
    ) -> Result<GeminiResearchResult, ProviderError> {
        let execution = self.research.result(conversation).await?;
        Ok(GeminiResearchResult {
            route: ROUTE.name(),
            conversation: conversation.clone(),
            state: execution.value,
            report_files: None,
            attempts: execution.attempts,
            diagnostic: execution.diagnostic,
        })
    }

    /// Starts a Deep Research: selects Deep Research, sends `query` once, and confirms the plan
    /// Gemini proposes.
    pub(crate) async fn start(
        &self,
        query: &str,
    ) -> Result<GeminiResearchStarted, GeminiResearchFailure> {
        match self.research.start(query).await {
            Ok(execution) => {
                let (conversation, plan) = execution.value;
                Ok(GeminiResearchStarted {
                    route: ROUTE.name(),
                    conversation,
                    plan,
                    attempts: execution.attempts,
                    diagnostic: execution.diagnostic,
                })
            }
            Err(StartFailure {
                error,
                conversation,
            }) => Err(GeminiResearchFailure {
                error,
                conversation,
            }),
        }
    }

    /// Opens the Gemini app and checks that the browser is signed in.
    pub(crate) async fn status(&self) -> Result<(), ProviderError> {
        self.research.status().await
    }
}

struct Gemini;

impl ResearchSite for Gemini {
    const ROUTE: ProviderId = ROUTE;
    const START_OPERATION: &'static str = GEMINI_RESEARCH_START;
    const RESULT_OPERATION: &'static str = GEMINI_RESEARCH_RESULT;
    const STATUS_TIMEOUT_MESSAGE: &'static str =
        "the Gemini app did not finish loading before the read deadline";
    const UNKNOWN_OUTCOME: &'static str = start::UNKNOWN_OUTCOME;
    const START_TIMEOUT_MESSAGE: &'static str = start::TIMEOUT_MESSAGE;

    type Conversation = GeminiConversationId;
    type Report = ReportData;
    type State = GeminiResearchState;
    type StartFacts = start::StartData;
    type Plan = GeminiPlan;
    type StatusPage = PageFacts;

    fn read_status_page(page: &PageFacts) -> Result<(), AttemptFailure> {
        page_shows_gemini(page, "retry")
    }

    fn read_research(
        report: &ReportData,
        conversation: &GeminiConversationId,
    ) -> Result<GeminiResearchState, AttemptFailure> {
        read_research(report, conversation)
    }

    fn read_start(
        facts: &start::StartData,
        created: &OnceCell<GeminiConversationId>,
    ) -> Result<(GeminiConversationId, GeminiPlan), AttemptFailure> {
        start::read_start(facts, created)
    }
}

impl ResearchConversation for GeminiConversationId {
    fn id(&self) -> &str {
        self.as_str()
    }

    fn url(&self) -> String {
        Self::url(self)
    }
}

/// Where the page ended up and what Gemini showed there.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct PageFacts {
    url: String,
    /// Whether the page asks the user to sign in.
    signed_out: bool,
    /// Whether Google redirected the page to its unusual-traffic page (`www.google.com/sorry`),
    /// which it shows after a burst of page loads.
    unusual_traffic: bool,
    /// A notice that the conversation cannot be opened, as the page words it.
    notice: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ReportData {
    page: PageFacts,
    /// The body of the first conversation response after the page opened the conversation.
    response: Option<String>,
    /// Whether the conversation response completed but its body was gone when read.
    body_missing: bool,
    timed_out: bool,
}

fn runtime(message: String) -> AttemptFailure {
    AttemptFailure {
        kind: AttemptErrorKind::Runtime,
        status: None,
        message,
    }
}

/// Fails a page that shows something in place of Gemini: a sign-in prompt, or Google's
/// unusual-traffic page, whose check only the user passes. Both are Auth, which is never
/// retried, since another page load only prolongs the block. A failure message ends with
/// `next`, what to do once the page is cleared.
fn page_shows_gemini(page: &PageFacts, next: &str) -> Result<(), AttemptFailure> {
    let problem = if page.signed_out {
        "Gemini asks the browser to sign in; sign in to gemini.google.com in the Chrome that OpenCLI drives"
    } else if page.unusual_traffic {
        "Google shows its unusual-traffic page in place of Gemini, as it does after many page loads in a short time; pass its check in the Chrome that OpenCLI drives, or wait until it lifts"
    } else {
        return Ok(());
    };
    Err(AttemptFailure {
        kind: AttemptErrorKind::Auth,
        status: None,
        message: format!("{problem}, then {next}"),
    })
}

fn unavailable(conversation: &GeminiConversationId, reason: &str) -> AttemptFailure {
    AttemptFailure {
        kind: AttemptErrorKind::Parameter,
        status: None,
        message: format!(
            "the Gemini conversation {} is unavailable: {reason}; it may not exist or may belong to another account",
            conversation.url()
        ),
    }
}

fn changed_structure(problem: &str) -> AttemptFailure {
    runtime(format!(
        "the Gemini response structure has changed at {problem}"
    ))
}

impl From<decode::ShapeError> for AttemptFailure {
    fn from(error: decode::ShapeError) -> Self {
        changed_structure(&error.to_string())
    }
}

/// Classifies the page facts of a `report` command, then decodes the conversation response.
/// A page in place of Gemini comes first: it hides whether the conversation exists.
fn read_research(
    report: &ReportData,
    conversation: &GeminiConversationId,
) -> Result<GeminiResearchState, AttemptFailure> {
    let page = &report.page;
    page_shows_gemini(page, "retry")?;
    if let Some(notice) = page.notice.as_deref().map(str::trim)
        && !notice.is_empty()
    {
        return Err(unavailable(
            conversation,
            &format!("the page says `{}`", truncate_message(notice)),
        ));
    }
    let shown = GeminiConversationId::from_url(&page.url);
    let Some(body) = report.response.as_deref() else {
        return Err(unread_conversation(report, shown.as_ref(), conversation));
    };
    if shown.as_ref() != Some(conversation) {
        return Err(other_conversation(&page.url, conversation));
    }
    let payload = decode::batchexecute_payload(body, CONVERSATION_RPC)?;
    match decode::research_state(&payload)? {
        Some(state) => Ok(state),
        None => Err(AttemptFailure {
            kind: AttemptErrorKind::Parameter,
            status: None,
            message: format!(
                "the Gemini conversation {} is not a Deep Research conversation: none of the {} turns the page loaded holds a research plan or report",
                conversation.url(),
                turn_count(&payload)
            ),
        }),
    }
}

fn turn_count(payload: &serde_json::Value) -> usize {
    payload
        .get(0)
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len)
}

/// Fails a read that returned no conversation response.
fn unread_conversation(
    report: &ReportData,
    shown: Option<&GeminiConversationId>,
    conversation: &GeminiConversationId,
) -> AttemptFailure {
    let on_app_home = reqwest::Url::parse(&report.page.url).is_ok_and(|url| {
        url.host_str() == Some("gemini.google.com") && url.path().trim_end_matches('/') == "/app"
    });
    if on_app_home {
        return unavailable(conversation, "Gemini opened its home page instead");
    }
    if shown != Some(conversation) {
        return other_conversation(&report.page.url, conversation);
    }
    if report.body_missing {
        return runtime(
            "the conversation response completed, but its body was missing or truncated when the forager-gemini adapter read it".into(),
        );
    }
    if report.timed_out {
        return AttemptFailure {
            kind: AttemptErrorKind::Timeout,
            status: None,
            message: "the Gemini page received no conversation response before the read deadline"
                .into(),
        };
    }
    runtime("the forager-gemini adapter returned no conversation response".into())
}

fn other_conversation(url: &str, conversation: &GeminiConversationId) -> AttemptFailure {
    runtime(format!(
        "the Gemini page shows `{}` instead of the requested conversation {}",
        truncate_message(url),
        conversation.url()
    ))
}
