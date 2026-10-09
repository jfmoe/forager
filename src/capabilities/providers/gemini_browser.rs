//! The `gemini_browser` provider: Gemini Deep Research conversations, started and read in the
//! user's own logged-in Chrome through the forager OpenCLI adapter `forager-gemini`.
//!
//! The JavaScript adapter only operates pages as a user would and reports what the page itself
//! received. This provider classifies the page facts, checks that the page shows the expected
//! conversation, and decodes the responses. Only `forager gemini` commands run it; no chain
//! does.

use std::cell::OnceCell;
use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::catalog::{ProviderId, ProviderTransport, registration};
use crate::config::ProcessRouteRuntimeConfig;
use crate::net::{AttemptFailure, RetryPolicy, truncate_message};
use crate::providers::execution::{ExecutionSettings, execute_anonymous};
use crate::providers::opencli::{self, EnvelopeStatus, OpenCliCommand, Window};
use crate::rate_limit::RateLimiter;
use crate::types::{
    AttemptErrorKind, AttemptTarget, Deadline, GEMINI_RESEARCH_RESULT, GEMINI_RESEARCH_START,
    GeminiConversationId, GeminiResearchFailure, GeminiResearchResult, GeminiResearchStarted,
    GeminiResearchState, ProviderError,
};

#[path = "gemini_browser_decode.rs"]
mod decode;
#[path = "gemini_browser_start.rs"]
mod start;

const ROUTE: ProviderId = ProviderId::GeminiBrowser;
const CONVERSATION_RPC: &str = "hNvQHb";
const STATUS_OPERATION: &str = "status";

pub(crate) struct GeminiBrowser {
    config: ProcessRouteRuntimeConfig,
    limiter: RateLimiter,
    retry_policy: RetryPolicy,
    deadline: Deadline,
}

impl GeminiBrowser {
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
    /// stands from the first conversation response the page receives. Reading changes
    /// nothing, so failed attempts follow the shared retry policy.
    pub(crate) async fn result(
        &self,
        conversation: &GeminiConversationId,
    ) -> Result<GeminiResearchResult, ProviderError> {
        let command = self.command(
            "report",
            vec![("conversation", conversation.as_str().to_owned())],
            Window::Background,
        );
        let command = &command;
        let execution = execute_anonymous(
            self.settings(GEMINI_RESEARCH_RESULT, self.retry_policy),
            move |deadline| async move {
                let report: ReportData = self.read(command, deadline).await?;
                read_research(&report, conversation).map(|state| (None, state))
            },
        )
        .await?;
        Ok(GeminiResearchResult {
            route: ROUTE.name(),
            conversation: conversation.clone(),
            state: execution.value,
            attempts: execution.attempts,
            diagnostic: execution.diagnostic,
        })
    }

    /// Starts a Deep Research in a foreground window, where the tools menu renders: selects
    /// Deep Research, sends `query` once, and confirms the plan Gemini proposes.
    ///
    /// Starting creates a conversation and spends the account's Deep Research quota, so it runs
    /// exactly one attempt whatever the retry configuration (ADR 0023). A failure carries the
    /// conversation URL once forager knows the conversation.
    pub(crate) async fn start(
        &self,
        query: &str,
    ) -> Result<GeminiResearchStarted, GeminiResearchFailure> {
        let command = self.command(
            "start",
            vec![("query", query.to_owned())],
            Window::Foreground,
        );
        let command = &command;
        let created = OnceCell::new();
        let created_ref = &created;
        let mut settings = self.settings(GEMINI_RESEARCH_START, single_attempt());
        settings.timeout_message = start::TIMEOUT_MESSAGE;
        let execution = execute_anonymous(settings, move |deadline| async move {
            let facts = self
                .read(command, deadline)
                .await
                .map_err(start::unknown_outcome)?;
            start::read_start(&facts, created_ref).map(|started| (None, started))
        })
        .await;
        match execution {
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
            Err(mut error) => {
                let conversation_url = created.get().map(GeminiConversationId::url);
                if let Some(url) = &conversation_url
                    && !error.message.contains(url.as_str())
                {
                    error.message = format!("{}; see {url}", error.message);
                }
                Err(GeminiResearchFailure {
                    error,
                    conversation_url,
                })
            }
        }
    }

    /// Opens the Gemini app in a background window and checks that the browser is signed in.
    /// The adapter contract and OpenCLI itself are checked on the way.
    pub(crate) async fn status(&self) -> Result<(), ProviderError> {
        let command = self.command(STATUS_OPERATION, Vec::new(), Window::Background);
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
                        message: "the Gemini app did not finish loading before the read deadline"
                            .into(),
                    });
                }
                Ok((None, ()))
            },
        )
        .await
        .map(|_| ())
    }

    fn command(
        &self,
        command: &'static str,
        options: Vec<(&'static str, String)>,
        window: Window,
    ) -> OpenCliCommand<'_> {
        let ProviderTransport::OpenCli(adapter) = registration(ROUTE).transport else {
            unreachable!("gemini_browser registers an OpenCLI transport");
        };
        OpenCliCommand {
            executable: &self.config.command,
            adapter,
            command,
            options,
            window,
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
    ) -> Result<T, AttemptFailure> {
        let envelope = opencli::run::<T>(command, &self.limiter, deadline).await?;
        if envelope.status != EnvelopeStatus::Ok {
            return Err(runtime(
                "the forager-gemini adapter reported no results instead of page facts".into(),
            ));
        }
        Ok(envelope.data)
    }
}

/// Where the page ended up and what Gemini showed there.
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
    /// The body of the first conversation response after the page opened the conversation.
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
        message: "Gemini asks the browser to sign in; sign in to gemini.google.com in the Chrome that OpenCLI drives, then retry".into(),
    }
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

/// Classifies the page facts of a `report` command, then decodes the conversation response.
/// A signed-out page comes first: it hides whether the conversation exists.
fn read_research(
    report: &ReportData,
    conversation: &GeminiConversationId,
) -> Result<GeminiResearchState, AttemptFailure> {
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
    let shown = GeminiConversationId::from_url(&page.url);
    let Some(body) = report.response.as_deref() else {
        return Err(unread_conversation(report, shown.as_ref(), conversation));
    };
    if shown.as_ref() != Some(conversation) {
        return Err(other_conversation(&page.url, conversation));
    }
    let payload = decode::batchexecute_payload(body, CONVERSATION_RPC)
        .map_err(|error| changed_structure(&error.0))?;
    match decode::research_state(&payload).map_err(|error| changed_structure(&error.0))? {
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
