//! Classification of the page facts of the adapter's `start` command: how far starting a Deep
//! Research got, and what the conversation response the page received after reopening the new
//! conversation says about it.
//!
//! Starting creates a conversation and spends the account's Deep Research quota, so every
//! failure names the conversation once forager knows it, and a failure that leaves the outcome
//! unknown points to the ChatGPT history instead of suggesting another start.

use std::cell::OnceCell;

use serde::Deserialize;

use super::decode::Conversation;
use super::{PageFacts, runtime, signed_out};
use crate::net::{AttemptFailure, truncate_message};
use crate::providers::opencli::CommandFailure;
use crate::types::{AttemptErrorKind, ChatgptConversationId, ChatgptPlan, ChatgptResearchState};

/// How much of a reply that is no plan a failure quotes.
const REPLY_HEAD_CHARS: usize = 200;

macro_rules! unknown_outcome {
    () => {
        "forager does not know whether ChatGPT received the question; look for it in the ChatGPT history at https://chatgpt.com/ before starting again"
    };
}

/// Appended to a failure that leaves unknown whether ChatGPT received the question.
const UNKNOWN_OUTCOME: &str = unknown_outcome!();
/// The failure of an attempt that outlived its deadline without returning page facts.
pub(super) const TIMEOUT_MESSAGE: &str = concat!("OpenCLI command timed out; ", unknown_outcome!());

/// The steps of `start`, in the order the adapter reaches them.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(try_from = "String")]
enum Step {
    /// The composer is in chat mode, where the Deep Research entry lives.
    ChatMode,
    /// The composer holds the question.
    Query,
    /// Deep Research was chosen in the "Add files and more" menu.
    DeepResearch,
    /// The composer holds the question and the Deep Research mention.
    Mention,
    /// The send button was clicked, once.
    Sent,
    /// The page moved to the new conversation's `/c/<uuid>`.
    Conversation,
    /// ChatGPT answered: a plan card, or a reply.
    Answered,
    /// The plan card showed; the adapter then keeps the page open through its countdown, after
    /// which ChatGPT starts the research by itself.
    Plan,
    /// The conversation was opened again and its response read.
    Reopened,
}

impl Step {
    const ALL: [Self; 9] = [
        Self::ChatMode,
        Self::Query,
        Self::DeepResearch,
        Self::Mention,
        Self::Sent,
        Self::Conversation,
        Self::Answered,
        Self::Plan,
        Self::Reopened,
    ];

    /// The name the adapter reports the step by.
    const fn name(self) -> &'static str {
        match self {
            Self::ChatMode => "chat_mode",
            Self::Query => "query",
            Self::DeepResearch => "deep_research",
            Self::Mention => "mention",
            Self::Sent => "sent",
            Self::Conversation => "conversation",
            Self::Answered => "answered",
            Self::Plan => "plan",
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
pub(super) struct StartData {
    page: PageFacts,
    /// The steps the adapter completed.
    steps: Vec<Step>,
    /// The conversation the page moved to after sending, as its URL showed it.
    conversation: Option<String>,
    /// Whether the chat-mode menu rendered, but without a Deep Research entry.
    deep_research_missing: bool,
    /// A usage notice that appeared after sending, as the page words it.
    quota_notice: Option<String>,
    /// Why the adapter stopped before the deadline, in its own words.
    problem: Option<String>,
    /// Whether the adapter emptied the composer after stopping before it sent; `None` when
    /// nothing needed clearing.
    draft_cleared: Option<bool>,
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
    fn conversation(&self) -> Option<ChatgptConversationId> {
        if !self.reached(Step::Sent) {
            return None;
        }
        self.conversation
            .as_deref()
            .and_then(|id| ChatgptConversationId::parse(id).ok())
            .or_else(|| ChatgptConversationId::from_url(&self.page.url))
    }
}

/// Classifies the page facts of a `start` command and returns the started conversation and its
/// plan. `created` receives the conversation as soon as the facts name it, so a failure can
/// still point to it.
pub(super) fn read_start(
    facts: &StartData,
    created: &OnceCell<ChatgptConversationId>,
) -> Result<(ChatgptConversationId, ChatgptPlan), AttemptFailure> {
    if facts.page.signed_out {
        return Err(signed_out());
    }
    let conversation = facts.conversation();
    if let Some(conversation) = &conversation {
        let _ = created.set(conversation.clone());
    }
    if let Some(notice) = facts.quota_notice.as_deref().map(str::trim)
        && !notice.is_empty()
        && facts.reached(Step::Sent)
        && !facts.reached(Step::Answered)
    {
        return Err(failure(
            AttemptErrorKind::QuotaExhausted,
            format!(
                "ChatGPT says `{}`: the Deep Research quota is used up; wait until it resets",
                truncate_message(notice)
            ),
            conversation.as_ref(),
        ));
    }
    if !facts.reached(Step::Sent) {
        return Err(unsent(facts));
    }
    let Some(conversation) = conversation else {
        let (kind, problem) = stopped(facts, "ChatGPT opened the new conversation");
        return Err(failure(kind, problem, None));
    };
    match facts.response.as_deref() {
        Some(body) => read_started(facts, body, &conversation).map(|plan| (conversation, plan)),
        None => Err(unread(facts, &conversation.url())),
    }
}

/// Judges the conversation response read after starting: the newest research has to be running
/// or completed.
fn read_started(
    facts: &StartData,
    body: &str,
    conversation: &ChatgptConversationId,
) -> Result<ChatgptPlan, AttemptFailure> {
    let url = conversation.url();
    if ChatgptConversationId::from_url(&facts.page.url).as_ref() != Some(conversation) {
        return Err(runtime(format!(
            "the ChatGPT page shows `{}` instead of the started conversation {url}",
            truncate_message(&facts.page.url)
        )));
    }
    let response = Conversation::parse(body)?;
    if let Some(refusal) = response.refusal() {
        return Err(runtime(format!(
            "ChatGPT answered `{}` instead of the started conversation {url}",
            truncate_message(&refusal)
        )));
    }
    let answered = response.conversation_id()?;
    if answered != conversation.as_str() {
        return Err(runtime(format!(
            "the ChatGPT response is for the conversation `{}` instead of the started conversation {url}",
            truncate_message(answered)
        )));
    }
    let Some(research) = response.newest_research()? else {
        return Err(runtime(match response.newest_reply() {
            Some(reply) => format!(
                "ChatGPT replied with text instead of a research plan, so no research started: `{}`; answer it at {url}, then read it with `forager chatgpt research result {url}`",
                reply_head(&reply)
            ),
            None => format!(
                "the conversation {url} holds neither a Deep Research nor a reply; open it to see what ChatGPT did before starting again"
            ),
        }));
    };
    match research.state() {
        Ok(ChatgptResearchState::Running { .. } | ChatgptResearchState::Completed(_)) => {
            Ok(research.plan()?)
        }
        _ if research.quota_exhausted() => Err(AttemptFailure {
            kind: AttemptErrorKind::QuotaExhausted,
            status: None,
            message: format!(
                "ChatGPT's usage receipt for {url} says the account's Deep Research limit is reached; wait until it resets"
            ),
        }),
        Ok(ChatgptResearchState::AwaitingConfirmation(_)) => {
            let after = if facts.reached(Step::Plan) {
                " after the plan's countdown"
            } else {
                ""
            };
            Err(runtime(format!(
                "ChatGPT proposed a research plan, but the research had not started when forager read the conversation again{after}: {}",
                not_started_hint(&url)
            )))
        }
        Err(error) => Err(error.into()),
    }
}

/// Fails a `start` that sent the question and knows the conversation, but read no conversation
/// response.
fn unread(facts: &StartData, url: &str) -> AttemptFailure {
    let result_hint = format!("read it with `forager chatgpt research result {url}`");
    let (kind, message) = if facts.reached(Step::Plan) {
        let (kind, problem) = stopped(facts, "forager read the conversation again");
        (
            kind,
            format!(
                "ChatGPT proposed a research plan, but {problem}: {}",
                not_started_hint(url)
            ),
        )
    } else if facts.reached(Step::Answered) {
        let (kind, problem) = stopped(facts, "forager read the reply");
        (
            kind,
            format!(
                "ChatGPT answered without showing a research plan, but {problem}; open {url} to read the reply, answer it there if it asks, then {result_hint}"
            ),
        )
    } else {
        let (kind, problem) = stopped(facts, "ChatGPT answered the question");
        (
            kind,
            format!("{problem}; {result_hint} before starting again"),
        )
    };
    AttemptFailure {
        kind,
        status: None,
        message,
    }
}

fn not_started_hint(url: &str) -> String {
    format!(
        "the plan's countdown may still start the research, so read it with `forager chatgpt research result {url}` before starting again; forager does not resend the question"
    )
}

/// Fails a `start` that stopped before it sent the question, so nothing reached ChatGPT.
fn unsent(facts: &StartData) -> AttemptFailure {
    let draft = match facts.draft_cleared {
        Some(true) => "; the composer draft was cleared",
        Some(false) => {
            "; forager could not clear the composer, so remove the leftover draft on chatgpt.com by hand"
        }
        None => "",
    };
    if facts.deep_research_missing {
        return runtime(format!(
            "ChatGPT's \"Add files and more\" menu in chat mode offers no Deep Research; check that this account can use Deep Research in the ChatGPT web app; nothing was sent{draft}"
        ));
    }
    let (kind, problem) = stopped(facts, "the question was sent");
    AttemptFailure {
        kind,
        status: None,
        message: format!(
            "{problem} (last step: {}); nothing was sent{draft}",
            facts.last_step()
        ),
    }
}

/// The kind and wording of an adapter that stopped before `awaited`.
fn stopped(facts: &StartData, awaited: &str) -> (AttemptErrorKind, String) {
    if facts.body_missing {
        return (
            AttemptErrorKind::Runtime,
            "the conversation response completed, but its body was missing or truncated when the forager-chatgpt adapter read it".to_owned(),
        );
    }
    if facts.timed_out {
        return (
            AttemptErrorKind::Timeout,
            format!("the read deadline passed before {awaited}"),
        );
    }
    let mut problem = format!("the forager-chatgpt adapter stopped before {awaited}");
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

/// A failure after the question may have reached ChatGPT. While forager does not know the
/// conversation, it points to the ChatGPT history; `ChatgptBrowser::start` points every failure
/// to a known conversation.
fn failure(
    kind: AttemptErrorKind,
    problem: String,
    at: Option<&ChatgptConversationId>,
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

/// Words a failure of the `start` command itself, which leaves no page facts. A command OpenCLI
/// never ran sent nothing, and neither did a browser that asks to sign in; any other failure
/// may have stopped after ChatGPT received the question.
pub(super) fn command_failed(failure: CommandFailure) -> AttemptFailure {
    let CommandFailure {
        attempt,
        may_have_run,
    } = failure;
    if may_have_run && attempt.kind == AttemptErrorKind::Auth {
        return attempt;
    }
    let hint = if may_have_run {
        UNKNOWN_OUTCOME
    } else {
        "nothing was sent"
    };
    AttemptFailure {
        message: format!("{}; {hint}", attempt.message),
        ..attempt
    }
}
