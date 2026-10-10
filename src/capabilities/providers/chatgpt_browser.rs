//! The `chatgpt_browser` provider: ChatGPT Deep Research conversations, read in the user's own
//! logged-in Chrome through the forager OpenCLI adapter `forager-chatgpt`.
//!
//! The JavaScript adapter only operates pages as a user would and reports what the page itself
//! received. This provider classifies the page facts, checks that the page and the response
//! show the requested conversation, and decodes the response. Only `forager chatgpt` commands
//! run it; no chain does. ADR 0024 lists the conditions every change here must keep.

use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::catalog::{ProviderId, ProviderTransport, registration};
use crate::config::ProcessRouteRuntimeConfig;
use crate::net::{AttemptFailure, RetryPolicy, truncate_message};
use crate::providers::execution::{ExecutionSettings, execute_anonymous};
use crate::providers::opencli::{self, CommandFailure, EnvelopeStatus, OpenCliCommand, Window};
use crate::rate_limit::RateLimiter;
use crate::types::{
    AttemptErrorKind, AttemptTarget, CHATGPT_RESEARCH_RESULT, ChatgptConversationId,
    ChatgptResearchResult, ChatgptResearchState, Deadline, ProviderError,
};

#[path = "chatgpt_browser_citations.rs"]
mod citations;
#[path = "chatgpt_browser_decode.rs"]
mod decode;

const ROUTE: ProviderId = ProviderId::ChatgptBrowser;
const STATUS_OPERATION: &str = "status";

pub(crate) struct ChatgptBrowser {
    config: ProcessRouteRuntimeConfig,
    limiter: RateLimiter,
    retry_policy: RetryPolicy,
    deadline: Deadline,
}

impl ChatgptBrowser {
    pub(crate) fn new(
        config: ProcessRouteRuntimeConfig,
        limiter: RateLimiter,
        retry_policy: RetryPolicy,
        deadline: Deadline,
    ) -> Self {
        Self {
            config,
            limiter,
            retry_policy,
            deadline,
        }
    }

    /// Opens the conversation in a background window and reads where its newest research
    /// stands from the conversation response the page itself receives. Reading changes
    /// nothing, so failed attempts follow the shared retry policy.
    pub(crate) async fn result(
        &self,
        conversation: &ChatgptConversationId,
    ) -> Result<ChatgptResearchResult, ProviderError> {
        let command = self.command(
            "report",
            vec![("conversation", conversation.as_str().to_owned())],
        );
        let command = &command;
        let execution = execute_anonymous(
            self.settings(CHATGPT_RESEARCH_RESULT, self.retry_policy),
            move |deadline| async move {
                let report: ReportData = self.read(command, deadline).await?;
                read_research(&report, conversation).map(|state| (None, state))
            },
        )
        .await?;
        Ok(ChatgptResearchResult {
            route: ROUTE.name(),
            conversation: conversation.clone(),
            state: execution.value,
            report_files: None,
            attempts: execution.attempts,
            diagnostic: execution.diagnostic,
        })
    }

    /// Opens the ChatGPT home page in a background window and checks that the browser is
    /// signed in, without switching modes or opening menus. The adapter contract and OpenCLI
    /// itself are checked on the way.
    pub(crate) async fn status(&self) -> Result<(), ProviderError> {
        let command = self.command(STATUS_OPERATION, Vec::new());
        let command = &command;
        execute_anonymous(
            self.settings(STATUS_OPERATION, single_attempt()),
            move |deadline| async move {
                let status: StatusData = self.read(command, deadline).await?;
                if status.page.signed_out {
                    return Err(signed_out());
                }
                if status.timed_out {
                    return Err(AttemptFailure {
                        kind: AttemptErrorKind::Timeout,
                        status: None,
                        message:
                            "the ChatGPT home page did not finish loading before the read deadline"
                                .into(),
                    });
                }
                Ok((None, ()))
            },
        )
        .await
        .map(|_| ())
    }

    /// A read-only adapter command, run in a background window.
    fn command(
        &self,
        command: &'static str,
        options: Vec<(&'static str, String)>,
    ) -> OpenCliCommand<'_> {
        let ProviderTransport::OpenCli(adapter) = registration(ROUTE).transport else {
            unreachable!("chatgpt_browser registers an OpenCLI transport");
        };
        OpenCliCommand {
            executable: &self.config.command,
            adapter,
            command,
            options,
            window: Window::Background,
        }
    }

    fn settings(&self, operation: &'static str, retry_policy: RetryPolicy) -> ExecutionSettings {
        ExecutionSettings {
            provider: ROUTE.name(),
            target: AttemptTarget::operation(operation),
            retry_policy,
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

    /// Runs one adapter command and returns the page facts it reported.
    async fn read<T: DeserializeOwned>(
        &self,
        command: &OpenCliCommand<'_>,
        deadline: Deadline,
    ) -> Result<T, CommandFailure> {
        let envelope = opencli::run::<T>(command, &self.limiter, deadline).await?;
        if envelope.status != EnvelopeStatus::Ok {
            return Err(runtime(
                "the forager-chatgpt adapter reported no results instead of page facts".into(),
            )
            .into());
        }
        Ok(envelope.data)
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
struct StatusData {
    page: PageFacts,
    timed_out: bool,
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

fn single_attempt() -> RetryPolicy {
    RetryPolicy::new(1, 1.0, Duration::ZERO)
}

fn runtime(message: String) -> AttemptFailure {
    AttemptFailure {
        kind: AttemptErrorKind::Runtime,
        status: None,
        message,
    }
}

fn signed_out() -> AttemptFailure {
    AttemptFailure {
        kind: AttemptErrorKind::Auth,
        status: None,
        message: "ChatGPT asks the browser to sign in; sign in to chatgpt.com in the Chrome that OpenCLI drives, then retry".into(),
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

impl From<decode::ShapeError> for AttemptFailure {
    fn from(error: decode::ShapeError) -> Self {
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
    // A delivered report outweighs a receipt; any other state of a refused research is moot.
    match research.state() {
        Ok(state @ ChatgptResearchState::Completed(_)) => Ok(state),
        _ if research.quota_exhausted() => Err(AttemptFailure {
            kind: AttemptErrorKind::QuotaExhausted,
            status: None,
            message: format!(
                "ChatGPT's usage receipt for {} says the account's Deep Research limit is reached; wait until it resets",
                conversation.url()
            ),
        }),
        state => state.map_err(AttemptFailure::from),
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
