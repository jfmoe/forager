//! Classification of the page facts of the adapter's `start` command: how far starting a Deep
//! Research got, and what the two `StreamGenerate` responses the page received say about it.
//!
//! Starting creates a conversation and spends the account's Deep Research quota, so every
//! failure names the conversation once forager knows it, and a failure that leaves the outcome
//! unknown points to the Gemini history instead of suggesting another start.

use std::cell::OnceCell;

use serde::Deserialize;

use super::decode::{self, PlanReply, StreamReply};
use super::{PageFacts, changed_structure, runtime, signed_out};
use crate::net::{AttemptFailure, truncate_message};
use crate::providers::opencli::CommandFailure;
use crate::types::{AttemptErrorKind, GeminiConversationId, GeminiPlan};

/// The `StreamGenerate` error code of an exhausted usage limit.
const USAGE_LIMIT_EXCEEDED: u64 = 1037;
/// How much of a reply that is no plan a failure quotes.
const REPLY_HEAD_CHARS: usize = 200;

macro_rules! unknown_outcome {
    () => {
        "forager does not know whether Gemini received the question; look for it in the Gemini history at https://gemini.google.com/app before starting again"
    };
}

/// Appended to a failure that leaves unknown whether Gemini received the question.
const UNKNOWN_OUTCOME: &str = unknown_outcome!();
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
    /// The first `StreamGenerate` completed.
    Answered,
    /// The plan's confirm button was clicked, once.
    Confirmed,
    /// The second `StreamGenerate` completed.
    Started,
}

impl Step {
    const ALL: [Self; 7] = [
        Self::ToolsMenu,
        Self::DeepResearch,
        Self::Query,
        Self::Sent,
        Self::Answered,
        Self::Confirmed,
        Self::Started,
    ];

    /// The name the adapter reports the step by.
    const fn name(self) -> &'static str {
        match self {
            Self::ToolsMenu => "tools_menu",
            Self::DeepResearch => "deep_research",
            Self::Query => "query",
            Self::Sent => "sent",
            Self::Answered => "answered",
            Self::Confirmed => "confirmed",
            Self::Started => "started",
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
    /// Whether the tools menu rendered, but without a Deep Research entry.
    deep_research_missing: bool,
    /// Whether the tools menu shows Deep Research, but disabled.
    deep_research_disabled: bool,
    /// A notice that the Deep Research quota is used up, as the page words it.
    quota_notice: Option<String>,
    /// Why the adapter stopped before the deadline, in its own words.
    problem: Option<String>,
    /// The body of the first `StreamGenerate`, which answers the question.
    plan_response: Option<String>,
    /// The body of the second `StreamGenerate`, which answers the confirmation.
    confirm_response: Option<String>,
    timed_out: bool,
}

impl StartData {
    fn reached(&self, step: Step) -> bool {
        self.steps.contains(&step)
    }

    fn last_step(&self) -> &'static str {
        self.steps.iter().max().map_or("none", |step| step.name())
    }
}

/// Classifies the page facts of a `start` command and returns the started conversation and its
/// plan. `created` receives the conversation as soon as the facts name it, so a failure can
/// still point to it.
pub(super) fn read_start(
    facts: &StartData,
    created: &OnceCell<GeminiConversationId>,
) -> Result<(GeminiConversationId, GeminiPlan), AttemptFailure> {
    if facts.page.signed_out {
        return Err(signed_out());
    }
    let [plan_reply, confirm_reply] = [&facts.plan_response, &facts.confirm_response]
        .map(|body| body.as_deref().map(decode::stream_reply).transpose());
    let shown = GeminiConversationId::from_url(&facts.page.url);
    let conversation = plan_reply
        .as_ref()
        .ok()
        .and_then(Option::as_ref)
        .and_then(|reply| reply.conversation.as_deref())
        .and_then(|id| GeminiConversationId::parse(id).ok())
        .or_else(|| shown.clone());
    if let Some(conversation) = &conversation {
        let _ = created.set(conversation.clone());
    }
    let (plan_reply, confirm_reply) = (plan_reply?, confirm_reply?);
    let at = conversation.as_ref();
    check_gemini_errors(facts, [plan_reply.as_ref(), confirm_reply.as_ref()], at)?;
    if !facts.reached(Step::Sent) {
        return Err(unsent(facts));
    }
    if !facts.reached(Step::Answered) {
        let (kind, problem) = stopped(facts, "Gemini answered the question");
        return Err(failure(kind, problem, at));
    }
    let plan = read_plan(plan_reply.as_ref(), at)?;
    let Some(conversation) = conversation.clone() else {
        return Err(changed_structure(
            "StreamGenerate[1][0]: no conversation id",
        ));
    };
    let url = conversation.url();
    if !facts.reached(Step::Confirmed) {
        return Err(runtime(format!(
            "Gemini proposed a research plan, but forager could not click its confirm button: open {url} and click \"Start research\" there; forager does not click again or resend the question"
        )));
    }
    if !facts.reached(Step::Started) {
        let (kind, problem) = stopped(facts, "Gemini answered the confirmation");
        return Err(AttemptFailure {
            kind,
            status: None,
            message: format!(
                "forager clicked \"Start research\", but {problem}; the research may be running: read it with `forager gemini research result {url}`"
            ),
        });
    }
    let candidate = confirm_reply
        .as_ref()
        .ok_or_else(|| missing_body("the confirmation", at))?
        .candidate
        .as_ref()
        .ok_or_else(|| changed_structure("StreamGenerate[4]: no reply candidate"))?;
    decode::research_started(candidate)?;
    if shown.as_ref() != Some(&conversation) {
        return Err(runtime(format!(
            "the Gemini page shows `{}` instead of the started conversation {url}",
            truncate_message(&facts.page.url)
        )));
    }
    Ok((conversation, plan))
}

/// Fails on an error Gemini answered in a response, or on a quota notice the page showed while
/// Gemini had not answered the question. Once Gemini answered, page text that reads like a
/// quota notice (a question or a reply about limits) never overrides what the answer says.
fn check_gemini_errors(
    facts: &StartData,
    replies: [Option<&StreamReply>; 2],
    at: Option<&GeminiConversationId>,
) -> Result<(), AttemptFailure> {
    if let Some(code) = replies
        .into_iter()
        .flatten()
        .find_map(|reply| reply.error_code)
    {
        return Err(if code == USAGE_LIMIT_EXCEEDED {
            failure(
                AttemptErrorKind::QuotaExhausted,
                format!(
                    "Gemini answered with error {code}: the account's usage limit is exhausted; wait until it refreshes"
                ),
                at,
            )
        } else {
            failure(
                AttemptErrorKind::Runtime,
                format!("Gemini answered with error code {code}"),
                at,
            )
        });
    }
    if let Some(notice) = facts.quota_notice.as_deref().map(str::trim)
        && !notice.is_empty()
        && !facts.reached(Step::Answered)
    {
        return Err(failure(
            AttemptErrorKind::QuotaExhausted,
            format!(
                "Gemini says `{}`: the Deep Research quota is used up; wait until it refreshes",
                truncate_message(notice)
            ),
            at,
        ));
    }
    Ok(())
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
    if facts.timed_out {
        (
            AttemptErrorKind::Timeout,
            format!("the read deadline passed before {awaited}"),
        )
    } else {
        let mut problem = format!("the forager-gemini adapter stopped before {awaited}");
        if let Some(reason) = facts.problem.as_deref().map(str::trim)
            && !reason.is_empty()
        {
            problem = format!("{problem}: {}", truncate_message(reason));
        }
        (AttemptErrorKind::Runtime, problem)
    }
}

fn read_plan(
    reply: Option<&StreamReply>,
    at: Option<&GeminiConversationId>,
) -> Result<GeminiPlan, AttemptFailure> {
    let candidate = reply
        .ok_or_else(|| missing_body("the question", at))?
        .candidate
        .as_ref()
        .ok_or_else(|| changed_structure("StreamGenerate[4]: no reply candidate"))?;
    match decode::plan_reply(candidate)? {
        PlanReply::Plan(plan) => Ok(plan),
        PlanReply::Text(text) => {
            let head = text.split_whitespace().collect::<Vec<_>>().join(" ");
            let head = head.chars().take(REPLY_HEAD_CHARS).collect::<String>();
            Err(failure(
                AttemptErrorKind::Runtime,
                format!(
                    "Gemini replied with text instead of a research plan, so no research started: `{head}`"
                ),
                at,
            ))
        }
    }
}

fn missing_body(answered: &str, at: Option<&GeminiConversationId>) -> AttemptFailure {
    failure(
        AttemptErrorKind::Runtime,
        format!(
            "Gemini's answer to {answered} completed, but its body was missing or truncated when the forager-gemini adapter read it"
        ),
        at,
    )
}

/// A failure after the question may have reached Gemini. While forager does not know the
/// conversation, it points to the Gemini history; `GeminiBrowser::start` points every failure
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

/// Words a failure of the `start` command itself, which leaves no page facts. A command OpenCLI
/// never ran sent nothing, and neither did a browser that asks to sign in; any other failure
/// may have stopped after Gemini received the question.
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
