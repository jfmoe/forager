//! Classification of the page facts of the adapter's `start` command: how far starting a Deep
//! Research got, and what the conversation response the page received after reopening the new
//! conversation says about it.
//!
//! Starting creates a conversation and spends the account's Deep Research quota, so every
//! failure names the conversation once forager knows it, and a failure that leaves the outcome
//! unknown points to the Gemini history instead of suggesting another start.

use std::cell::OnceCell;

use serde::Deserialize;

use super::{CONVERSATION_RPC, PageFacts, decode, runtime, signed_out, signed_out_then};
use crate::net::{AttemptFailure, truncate_message};
use crate::types::{AttemptErrorKind, GeminiConversationId, GeminiPlan, GeminiResearchState};

/// How much of a reply that is no plan a failure quotes.
const REPLY_HEAD_CHARS: usize = 200;

macro_rules! unknown_outcome {
    () => {
        "forager does not know whether Gemini received the question; look for it in the Gemini history at https://gemini.google.com/app before starting again"
    };
}

/// Appended to a failure that leaves unknown whether Gemini received the question.
pub(super) const UNKNOWN_OUTCOME: &str = unknown_outcome!();
/// The failure of an attempt that outlived its deadline without returning page facts.
pub(super) const TIMEOUT_MESSAGE: &str = concat!("OpenCLI command timed out; ", unknown_outcome!());

/// The steps of `start`, in the order the adapter reaches them.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(try_from = "String")]
enum Step {
    /// The tools menu rendered its items.
    ToolsMenu,
    /// Deep Research is selected: its deselect button shows.
    DeepResearch,
    /// The composer holds the question.
    Query,
    /// The send button was clicked, once.
    Sent,
    /// The page moved to the new conversation's `/app/<id>`.
    Conversation,
    /// The plan card showed its "Start research" button.
    Answered,
    /// The plan's confirm button was clicked, once.
    Confirmed,
    /// The conversation was opened again and its response read.
    Reopened,
}

impl Step {
    const ALL: [Self; 8] = [
        Self::ToolsMenu,
        Self::DeepResearch,
        Self::Query,
        Self::Sent,
        Self::Conversation,
        Self::Answered,
        Self::Confirmed,
        Self::Reopened,
    ];

    /// The name the adapter reports the step by.
    const fn name(self) -> &'static str {
        match self {
            Self::ToolsMenu => "tools_menu",
            Self::DeepResearch => "deep_research",
            Self::Query => "query",
            Self::Sent => "sent",
            Self::Conversation => "conversation",
            Self::Answered => "answered",
            Self::Confirmed => "confirmed",
            Self::Reopened => "reopened",
        }
    }
}

impl TryFrom<String> for Step {
    type Error = String;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        Self::ALL
            .into_iter()
            .find(|step| step.name() == name)
            .ok_or_else(|| format!("unknown start step `{name}`"))
    }
}

/// The page facts of a `start` command.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "mirrors the independent flags of the adapter's JSON envelope"
)]
pub(super) struct StartData {
    page: PageFacts,
    /// The steps the adapter completed.
    steps: Vec<Step>,
    /// The conversation the page moved to after sending, as its URL showed it.
    conversation: Option<String>,
    /// Whether the tools menu rendered, but without a Deep Research entry.
    deep_research_missing: bool,
    /// Whether the tools menu shows Deep Research, but disabled.
    deep_research_disabled: bool,
    /// A notice that the Deep Research quota is used up, as the page words it.
    quota_notice: Option<String>,
    /// Why the adapter stopped before the deadline, in its own words.
    problem: Option<String>,
    /// The body of the conversation response the page received after reopening it.
    response: Option<String>,
    /// Whether the conversation response completed but its body was gone when read.
    body_missing: bool,
    timed_out: bool,
}

impl StartData {
    fn reached(&self, step: Step) -> bool {
        self.steps.contains(&step)
    }

    /// Why the adapter stopped, in its own words, when it said.
    fn reason(&self) -> Option<String> {
        self.problem
            .as_deref()
            .map(str::trim)
            .filter(|reason| !reason.is_empty())
            .map(truncate_message)
    }

    fn last_step(&self) -> &'static str {
        self.steps.iter().max().map_or("none", |step| step.name())
    }

    /// The conversation the question created, once the adapter sent it.
    fn conversation(&self) -> Option<GeminiConversationId> {
        if !self.reached(Step::Sent) {
            return None;
        }
        self.conversation
            .as_deref()
            .and_then(|id| GeminiConversationId::parse(id).ok())
            .or_else(|| GeminiConversationId::from_url(&self.page.url))
    }
}

/// Classifies the page facts of a `start` command and returns the started conversation and its
/// plan. `created` receives the conversation as soon as the facts name it, so a failure can
/// still point to it.
pub(super) fn read_start(
    facts: &StartData,
    created: &OnceCell<GeminiConversationId>,
) -> Result<(GeminiConversationId, GeminiPlan), AttemptFailure> {
    let conversation = facts.conversation();
    if let Some(conversation) = &conversation {
        let _ = created.set(conversation.clone());
    }
    if facts.page.signed_out {
        return Err(match &conversation {
            Some(conversation) => signed_out_then(&format!(
                "read the research with `forager gemini research result {}` before starting again",
                conversation.url()
            )),
            None => signed_out(),
        });
    }
    // Once Gemini answered, page text that reads like a quota notice (a question or a reply
    // about limits) never overrides what the conversation says.
    if let Some(notice) = facts.quota_notice.as_deref().map(str::trim)
        && !notice.is_empty()
        && facts.reached(Step::Sent)
        && !facts.reached(Step::Answered)
    {
        return Err(failure(
            AttemptErrorKind::QuotaExhausted,
            format!(
                "Gemini says `{}`: the Deep Research quota is used up; wait until it refreshes",
                truncate_message(notice)
            ),
            conversation.as_ref(),
        ));
    }
    if !facts.reached(Step::Sent) {
        return Err(unsent(facts));
    }
    let Some(conversation) = conversation else {
        let (kind, problem) = stopped(facts, "Gemini opened the new conversation");
        return Err(failure(kind, problem, None));
    };
    match facts.response.as_deref() {
        Some(body) => read_started(facts, body, &conversation).map(|plan| (conversation, plan)),
        None => Err(unread(facts, &conversation.url())),
    }
}

/// Judges the conversation response read after starting: the newest research has to be running
/// or completed, whether forager's click or Gemini itself started it.
fn read_started(
    facts: &StartData,
    body: &str,
    conversation: &GeminiConversationId,
) -> Result<GeminiPlan, AttemptFailure> {
    let url = conversation.url();
    if GeminiConversationId::from_url(&facts.page.url).as_ref() != Some(conversation) {
        return Err(runtime(format!(
            "the Gemini page shows `{}` instead of the started conversation {url}",
            truncate_message(&facts.page.url)
        )));
    }
    let payload = decode::batchexecute_payload(body, CONVERSATION_RPC)?;
    match decode::research_state(&payload)? {
        None => Err(runtime(match decode::newest_reply(&payload) {
            Some(reply) => format!(
                "Gemini replied with text instead of a research plan, so no research started: `{}`; answer it at {url}, then read it with `forager gemini research result {url}`",
                reply_head(reply)
            ),
            None => format!(
                "the conversation {url} holds neither a Deep Research nor a reply; open it to see what Gemini did before starting again"
            ),
        })),
        Some(GeminiResearchState::Running(_) | GeminiResearchState::Completed(_)) => {
            decode::newest_plan(&payload)?
                .ok_or_else(|| super::changed_structure("hNvQHb: no turn holds the research plan"))
        }
        Some(GeminiResearchState::AwaitingConfirmation(_)) if facts.reached(Step::Confirmed) => {
            Err(runtime(format!(
                "forager clicked \"Start research\", but the research had not started when forager read the conversation again; read it with `forager gemini research result {url}` before starting again; forager does not click again or resend the question"
            )))
        }
        Some(GeminiResearchState::AwaitingConfirmation(_)) => Err(unconfirmed(facts, &url)),
    }
}

/// Fails a `start` that sent the question and knows the conversation, but read no conversation
/// response.
fn unread(facts: &StartData, url: &str) -> AttemptFailure {
    let result_hint = format!("read it with `forager gemini research result {url}`");
    if facts.reached(Step::Confirmed) {
        let (kind, problem) = stopped(facts, "forager read the conversation again");
        return AttemptFailure {
            kind,
            status: None,
            message: format!(
                "forager clicked \"Start research\", but {problem}; the research may be running: {result_hint}"
            ),
        };
    }
    if facts.reached(Step::Answered) {
        return unconfirmed(facts, url);
    }
    let (kind, problem) = stopped(facts, "Gemini answered the question");
    AttemptFailure {
        kind,
        status: None,
        message: format!("{problem}; {result_hint} before starting again"),
    }
}

/// Fails a plan forager did not confirm, which waits for the user on the conversation page.
fn unconfirmed(facts: &StartData, url: &str) -> AttemptFailure {
    let (kind, problem) = if facts.timed_out {
        (
            AttemptErrorKind::Timeout,
            "the read deadline passed before forager clicked its confirm button".to_owned(),
        )
    } else {
        let reason = facts
            .reason()
            .map(|reason| format!(" ({reason})"))
            .unwrap_or_default();
        (
            AttemptErrorKind::Runtime,
            format!("forager could not click its confirm button{reason}"),
        )
    };
    AttemptFailure {
        kind,
        status: None,
        message: format!(
            "Gemini proposed a research plan, but {problem}: open {url} and click \"Start research\" there; forager does not click again or resend the question"
        ),
    }
}

/// Fails a `start` that stopped before it sent the question, so nothing reached Gemini.
fn unsent(facts: &StartData) -> AttemptFailure {
    if facts.deep_research_missing {
        return runtime(
            "Gemini's tools menu offers no Deep Research; check that this Google account can use Deep Research in the Gemini web app; nothing was sent".into(),
        );
    }
    if facts.deep_research_disabled {
        return runtime(
            "Gemini's tools menu shows Deep Research but does not let it be selected; the account's Deep Research quota may be used up, or Deep Research may be unavailable right now; nothing was sent".into(),
        );
    }
    let (kind, problem) = stopped(facts, "the question was sent");
    AttemptFailure {
        kind,
        status: None,
        message: format!(
            "{problem} (last step: {}); nothing was sent",
            facts.last_step()
        ),
    }
}

/// The kind and wording of an adapter that stopped before `awaited`.
fn stopped(facts: &StartData, awaited: &str) -> (AttemptErrorKind, String) {
    if facts.body_missing {
        return (
            AttemptErrorKind::Runtime,
            "the conversation response completed, but its body was missing or truncated when the forager-gemini adapter read it".to_owned(),
        );
    }
    if facts.timed_out {
        return (
            AttemptErrorKind::Timeout,
            format!("the read deadline passed before {awaited}"),
        );
    }
    let mut problem = format!("the forager-gemini adapter stopped before {awaited}");
    if let Some(reason) = facts.reason() {
        problem = format!("{problem}: {reason}");
    }
    (AttemptErrorKind::Runtime, problem)
}

fn reply_head(reply: &str) -> String {
    reply
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(REPLY_HEAD_CHARS)
        .collect()
}

/// A failure after the question may have reached Gemini. While forager does not know the
/// conversation, it points to the Gemini history; the shared `start` points every failure
/// to a known conversation.
fn failure(
    kind: AttemptErrorKind,
    problem: String,
    at: Option<&GeminiConversationId>,
) -> AttemptFailure {
    let message = if at.is_some() {
        problem
    } else {
        format!("{problem}; {UNKNOWN_OUTCOME}")
    };
    AttemptFailure {
        kind,
        status: None,
        message,
    }
}
