//! The `chatgpt_browser` provider: ChatGPT Deep Research conversations, read in the user's own
//! logged-in Chrome through the forager OpenCLI adapter `forager-chatgpt`.
//!
//! The JavaScript adapter only operates pages as a user would and reports what the page itself
//! received. The shared delegated research flow runs the commands; this provider classifies the
//! page facts, checks that the page and the response show the requested conversation, and
//! decodes the response. Only `forager chatgpt` commands run it; no chain does. ADR 0024 lists
//! the conditions every change here must keep.

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
    AttemptErrorKind, CHATGPT_RESEARCH_RESULT, CHATGPT_RESEARCH_START, ChatgptConversationId,
    ChatgptPlan, ChatgptResearchFailure, ChatgptResearchResult, ChatgptResearchStarted,
    ChatgptResearchState, Deadline, ProviderError,
};

#[path = "chatgpt_browser_citations.rs"]
mod citations;
#[path = "chatgpt_browser_decode.rs"]
mod decode;
#[path = "chatgpt_browser_shape.rs"]
mod shape;
#[path = "chatgpt_browser_start.rs"]
mod start;

const ROUTE: ProviderId = ProviderId::ChatgptBrowser;

pub(crate) struct ChatgptBrowser {
    research: DelegatedResearch<Chatgpt>,
}

impl ChatgptBrowser {
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

    /// Reads where the conversation's newest research stands from the conversation response
    /// the page itself receives.
    pub(crate) async fn result(
        &self,
        conversation: &ChatgptConversationId,
    ) -> Result<ChatgptResearchResult, ProviderError> {
        let execution = self.research.result(conversation).await?;
        Ok(ChatgptResearchResult {
            route: ROUTE.name(),
            conversation: conversation.clone(),
            state: execution.value,
            report_files: None,
            attempts: execution.attempts,
            diagnostic: execution.diagnostic,
        })
    }

    /// Starts a Deep Research: switches the composer to chat mode, selects Deep Research, sends
    /// `query` once, keeps the page open through the plan's countdown, after which ChatGPT
    /// starts the research by itself, and reopens the conversation to check that the research
    /// runs.
    pub(crate) async fn start(
        &self,
        query: &str,
    ) -> Result<ChatgptResearchStarted, ChatgptResearchFailure> {
        match self.research.start(query).await {
            Ok(execution) => {
                let (conversation, plan) = execution.value;
                Ok(ChatgptResearchStarted {
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
            }) => Err(ChatgptResearchFailure {
                error,
                conversation,
            }),
        }
    }

    /// Opens the ChatGPT home page and checks that the browser is signed in, without switching
    /// modes or opening menus.
    pub(crate) async fn status(&self) -> Result<(), ProviderError> {
        self.research.status().await
    }
}

struct Chatgpt;

impl ResearchSite for Chatgpt {
    const ROUTE: ProviderId = ROUTE;
    const START_OPERATION: &'static str = CHATGPT_RESEARCH_START;
    const RESULT_OPERATION: &'static str = CHATGPT_RESEARCH_RESULT;
    const STATUS_TIMEOUT_MESSAGE: &'static str =
        "the ChatGPT home page did not finish loading before the read deadline";
    const UNKNOWN_OUTCOME: &'static str = start::UNKNOWN_OUTCOME;
    const START_TIMEOUT_MESSAGE: &'static str = start::TIMEOUT_MESSAGE;

    type Conversation = ChatgptConversationId;
    type Report = ReportData;
    type State = ChatgptResearchState;
    type StartFacts = start::StartData;
    type Plan = ChatgptPlan;
    type StatusPage = PageFacts;

    fn read_status_page(page: &PageFacts) -> Result<(), AttemptFailure> {
        if page.signed_out {
            return Err(signed_out());
        }
        Ok(())
    }

    fn read_research(
        report: &ReportData,
        conversation: &ChatgptConversationId,
    ) -> Result<ChatgptResearchState, AttemptFailure> {
        read_research(report, conversation)
    }

    fn read_start(
        facts: &start::StartData,
        created: &OnceCell<ChatgptConversationId>,
    ) -> Result<(ChatgptConversationId, ChatgptPlan), AttemptFailure> {
        start::read_start(facts, created)
    }
}

impl ResearchConversation for ChatgptConversationId {
    fn id(&self) -> &str {
        self.as_str()
    }

    fn url(&self) -> String {
        Self::url(self)
    }
}

/// Where the page ended up and what ChatGPT showed there.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct PageFacts {
    url: String,
    /// Whether the page asks the user to sign in.
    signed_out: bool,
    /// A notice that the conversation cannot be opened, as the page words it.
    notice: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ReportData {
    page: PageFacts,
    /// The body of the conversation response the page received after it opened the
    /// conversation.
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

fn signed_out() -> AttemptFailure {
    signed_out_then("retry")
}

/// A signed-out failure whose message ends with what to do after signing in.
fn signed_out_then(next: &str) -> AttemptFailure {
    AttemptFailure {
        kind: AttemptErrorKind::Auth,
        status: None,
        message: format!(
            "ChatGPT asks the browser to sign in; sign in to chatgpt.com in the Chrome that OpenCLI drives, then {next}"
        ),
    }
}

fn unavailable(conversation: &ChatgptConversationId, reason: &str) -> AttemptFailure {
    AttemptFailure {
        kind: AttemptErrorKind::Parameter,
        status: None,
        message: format!(
            "the ChatGPT conversation {} is unavailable: {reason}; it may not exist or may belong to another account",
            conversation.url()
        ),
    }
}

fn changed_structure(problem: &str) -> AttemptFailure {
    runtime(format!(
        "the ChatGPT response structure has changed at {problem}"
    ))
}

impl From<shape::ShapeError> for AttemptFailure {
    fn from(error: shape::ShapeError) -> Self {
        changed_structure(&error.to_string())
    }
}

/// Classifies the page facts of a `report` command, then decodes the conversation response.
/// A signed-out page comes first: it hides whether the conversation exists.
fn read_research(
    report: &ReportData,
    conversation: &ChatgptConversationId,
) -> Result<ChatgptResearchState, AttemptFailure> {
    let page = &report.page;
    if page.signed_out {
        return Err(signed_out());
    }
    if let Some(notice) = page.notice.as_deref().map(str::trim)
        && !notice.is_empty()
    {
        return Err(unavailable(
            conversation,
            &format!("the page says `{}`", truncate_message(notice)),
        ));
    }
    let shown = ChatgptConversationId::from_url(&page.url);
    let Some(body) = report.response.as_deref() else {
        return Err(unread_conversation(report, shown.as_ref(), conversation));
    };
    if shown.as_ref() != Some(conversation) {
        return Err(other_conversation(&page.url, conversation));
    }
    let response = decode::Conversation::parse(body)?;
    if let Some(refusal) = response.refusal() {
        return Err(unavailable(
            conversation,
            &format!("ChatGPT answered `{}`", truncate_message(&refusal)),
        ));
    }
    let answered = response.conversation_id()?;
    if answered != conversation.as_str() {
        return Err(runtime(format!(
            "the ChatGPT response is for the conversation `{}` instead of the requested conversation {}",
            truncate_message(answered),
            conversation.url()
        )));
    }
    let Some(research) = response.newest_research()? else {
        return Err(AttemptFailure {
            kind: AttemptErrorKind::Parameter,
            status: None,
            message: format!(
                "the ChatGPT conversation {} is not a Deep Research conversation: none of the {} messages the page loaded holds a Deep Research",
                conversation.url(),
                response.message_count()
            ),
        });
    };
    research
        .standing()
        .map_err(|error| standing_failure(error, conversation))
}

/// The failure of a research whose standing could not be read.
fn standing_failure(
    error: decode::StandingError,
    conversation: &ChatgptConversationId,
) -> AttemptFailure {
    match error {
        decode::StandingError::QuotaExhausted => AttemptFailure {
            kind: AttemptErrorKind::QuotaExhausted,
            status: None,
            message: format!(
                "ChatGPT's usage receipt for {} says the account's Deep Research limit is reached; wait until it resets",
                conversation.url()
            ),
        },
        decode::StandingError::Shape(error) => error.into(),
    }
}

/// Fails a read that returned no conversation response.
fn unread_conversation(
    report: &ReportData,
    shown: Option<&ChatgptConversationId>,
    conversation: &ChatgptConversationId,
) -> AttemptFailure {
    let on_home = reqwest::Url::parse(&report.page.url)
        .is_ok_and(|url| url.host_str() == Some("chatgpt.com") && url.path() == "/");
    if on_home {
        return unavailable(conversation, "ChatGPT opened its home page instead");
    }
    if shown != Some(conversation) {
        return other_conversation(&report.page.url, conversation);
    }
    if report.body_missing {
        return runtime(
            "the conversation response completed, but its body was missing or truncated when the forager-chatgpt adapter read it".into(),
        );
    }
    if report.timed_out {
        return AttemptFailure {
            kind: AttemptErrorKind::Timeout,
            status: None,
            message: "the ChatGPT page received no conversation response before the read deadline"
                .into(),
        };
    }
    runtime("the forager-chatgpt adapter returned no conversation response".into())
}

fn other_conversation(url: &str, conversation: &ChatgptConversationId) -> AttemptFailure {
    runtime(format!(
        "the ChatGPT page shows `{}` instead of the requested conversation {}",
        truncate_message(url),
        conversation.url()
    ))
}
