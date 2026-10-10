//! Pure decoding of the conversation response the ChatGPT page itself received: the flat message
//! list, the newest Deep Research message, and the state in its `widget_state`. Field positions
//! follow the live responses of 2026-10-10 (`docs/research/2026-10-10-chatgpt-deep-research.md`).

use std::borrow::Cow;

use serde_json::Value;

use super::citations;
use crate::types::{
    ChatgptPlan, ChatgptPlanStep, ChatgptProgress, ChatgptResearchState, DelegatedReport,
};

const RESEARCH_TOOL: &str = "api_tool.call_tool";
const AWAITING_CONFIRMATION: &str = "waiting_for_user_response_on_plan";
const RESEARCHING: &str = "researching";
const COMPLETED: &str = "completed";

/// Where a response stopped matching the shape forager decodes.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub(super) struct ShapeError(String);

impl ShapeError {
    pub(super) fn at(path: &str, problem: &str) -> Self {
        Self(format!("{path}: {problem}"))
    }
}

/// A JSON value and its path from the root of the response, for shape errors.
#[derive(Clone)]
pub(super) struct Node<'a> {
    pub(super) value: &'a Value,
    path: String,
}

impl<'a> Node<'a> {
    pub(super) fn root(value: &'a Value, path: &str) -> Self {
        Self {
            value,
            path: path.to_owned(),
        }
    }

    pub(super) fn path(&self) -> &str {
        &self.path
    }

    /// The non-null member `name`, or `None`.
    pub(super) fn get(&self, name: &str) -> Option<Self> {
        self.value
            .get(name)
            .filter(|value| !value.is_null())
            .map(|value| Self {
                value,
                path: format!("{}.{name}", self.path),
            })
    }

    pub(super) fn field(&self, name: &str) -> Result<Self, ShapeError> {
        self.get(name)
            .ok_or_else(|| self.error(&format!("no `{name}`")))
    }

    pub(super) fn items(&self) -> Result<impl Iterator<Item = Node<'a>>, ShapeError> {
        let path = self.path.clone();
        let items = self
            .value
            .as_array()
            .ok_or_else(|| self.error("not an array"))?;
        Ok(items.iter().enumerate().map(move |(index, value)| Node {
            value,
            path: format!("{path}[{index}]"),
        }))
    }

    pub(super) fn text(&self) -> Result<&'a str, ShapeError> {
        self.value
            .as_str()
            .ok_or_else(|| self.error("not a string"))
    }

    pub(super) fn index(&self) -> Result<usize, ShapeError> {
        self.value
            .as_u64()
            .and_then(|number| usize::try_from(number).ok())
            .ok_or_else(|| self.error("not a whole number"))
    }

    pub(super) fn error(&self, problem: &str) -> ShapeError {
        ShapeError::at(&self.path, problem)
    }
}

/// A decoded conversation response: an object holding the conversation id and its messages.
pub(super) struct Conversation {
    root: Value,
}

impl Conversation {
    /// Parses a conversation response body.
    ///
    /// # Errors
    ///
    /// Fails when the body is no JSON object.
    pub(super) fn parse(body: &str) -> Result<Self, ShapeError> {
        let root: Value = serde_json::from_str(body)
            .map_err(|error| ShapeError(format!("conversation: not JSON: {error}")))?;
        if !root.is_object() {
            return Err(ShapeError("conversation: not an object".into()));
        }
        Ok(Self { root })
    }

    fn node(&self) -> Node<'_> {
        Node::root(&self.root, "conversation")
    }

    /// The reason ChatGPT gave instead of the conversation, such as `conversation_not_found`,
    /// when the response carries a `detail` and no messages.
    pub(super) fn refusal(&self) -> Option<String> {
        if self.root.get("messages").is_some() {
            return None;
        }
        match self.root.get("detail")? {
            Value::String(detail) => Some(detail.clone()),
            detail => Some(detail.to_string()),
        }
    }

    pub(super) fn conversation_id(&self) -> Result<&str, ShapeError> {
        self.node().field("conversation_id")?.text()
    }

    /// The number of messages the page loaded.
    pub(super) fn message_count(&self) -> usize {
        self.root
            .get("messages")
            .and_then(Value::as_array)
            .map_or(0, Vec::len)
    }

    /// The newest Deep Research message, or `None` when no loaded message is one. Messages are
    /// listed oldest first.
    ///
    /// # Errors
    ///
    /// Fails when the response has no message list.
    pub(super) fn newest_research(&self) -> Result<Option<Research<'_>>, ShapeError> {
        let messages = self.node().field("messages")?;
        Ok(messages
            .items()?
            .filter_map(|message| research(&message))
            .last())
    }

    /// The text of the newest assistant message that has any, such as a clarifying question
    /// or a refusal, or `None` when no loaded assistant message has text.
    pub(super) fn newest_reply(&self) -> Option<String> {
        self.root
            .get("messages")?
            .as_array()?
            .iter()
            .rev()
            .filter(|message| {
                message.pointer("/author/role").and_then(Value::as_str) == Some("assistant")
            })
            .find_map(|message| {
                let text = message
                    .pointer("/content/parts")?
                    .as_array()?
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("\n");
                (!text.trim().is_empty()).then_some(text)
            })
    }
}

/// A Deep Research message: a tool message from `api_tool.call_tool` that carries a widget
/// state.
pub(super) struct Research<'a> {
    sdk: Node<'a>,
    widget_state: Node<'a>,
}

fn research<'a>(message: &Node<'a>) -> Option<Research<'a>> {
    let author = message.get("author")?;
    let is_research_tool = author.get("role")?.value.as_str() == Some("tool")
        && author.get("name")?.value.as_str() == Some(RESEARCH_TOOL);
    let sdk = message.get("metadata")?.get("chatgpt_sdk")?;
    let widget_state = sdk.get("widget_state")?;
    is_research_tool.then_some(Research { sdk, widget_state })
}

impl Research<'_> {
    /// Whether the usage receipt of the research says that the account's Deep Research limit
    /// refused it.
    pub(super) fn quota_exhausted(&self) -> bool {
        let Some(receipt) = self
            .sdk
            .get("tool_response_metadata")
            .and_then(|metadata| metadata.get("usage_refund_receipt"))
        else {
            return false;
        };
        let receipt = match receipt.value {
            Value::String(text) => match serde_json::from_str::<Value>(text) {
                Ok(value) => Cow::Owned(value),
                Err(_) => return false,
            },
            value => Cow::Borrowed(value),
        };
        receipt.get("allowed").and_then(Value::as_bool) == Some(false)
            || receipt
                .pointer("/rate_limit/limit_reached")
                .and_then(Value::as_bool)
                == Some(true)
    }

    /// Judges where the research stands from `widget_state.status`, the current state. The
    /// statuses in `tool_response_metadata` are snapshots taken when the tool was called.
    ///
    /// # Errors
    ///
    /// Fails with the location where the state stops matching the recorded shape: an unknown
    /// status, a status that contradicts the report, or a missing field.
    pub(super) fn state(&self) -> Result<ChatgptResearchState, ShapeError> {
        let value = self.widget_state()?;
        let state = Node::root(&value, self.widget_state.path());
        let status_node = state.field("status")?;
        let status = status_node.text()?;
        let report = state.get("report_message");
        match status {
            AWAITING_CONFIRMATION if report.is_none() => {
                decode_plan(&state.field("plan")?).map(ChatgptResearchState::AwaitingConfirmation)
            }
            RESEARCHING if report.is_none() => {
                let plan = state.field("plan")?;
                Ok(ChatgptResearchState::Running {
                    plan_title: plan.field("title")?.text()?.to_owned(),
                    progress: decode_progress(&state, &plan)?,
                })
            }
            AWAITING_CONFIRMATION | RESEARCHING => {
                Err(status_node.error(&format!("status `{status}` with a report message")))
            }
            COMPLETED => {
                let body = match &report {
                    Some(report) => report_body(report)?,
                    None => "",
                };
                match report {
                    Some(report) if !body.is_empty() => {
                        decode_report(&report, body, &state.field("plan")?)
                            .map(ChatgptResearchState::Completed)
                    }
                    _ => Err(status_node.error("status `completed` without a report body")),
                }
            }
            other => Err(status_node.error(&format!("unknown status `{other}`"))),
        }
    }

    /// The plan the research follows, whatever its status.
    ///
    /// # Errors
    ///
    /// Fails with the location where the plan stops matching the recorded shape.
    pub(super) fn plan(&self) -> Result<ChatgptPlan, ShapeError> {
        let value = self.widget_state()?;
        decode_plan(&Node::root(&value, self.widget_state.path()).field("plan")?)
    }

    /// The widget state, which the message carries as a JSON string.
    fn widget_state(&self) -> Result<Value, ShapeError> {
        let text = self.widget_state.text()?;
        serde_json::from_str(text)
            .map_err(|error| self.widget_state.error(&format!("not JSON: {error}")))
    }
}

fn report_body<'a>(report: &Node<'a>) -> Result<&'a str, ShapeError> {
    let parts = report.field("content")?.field("parts")?;
    match parts.items()?.next() {
        Some(part) => part.text(),
        None => Ok(""),
    }
}

fn decode_plan(plan: &Node<'_>) -> Result<ChatgptPlan, ShapeError> {
    let steps = plan
        .field("steps")?
        .items()?
        .enumerate()
        .map(|(position, step)| {
            Ok(ChatgptPlanStep {
                index: position + 1,
                text: step.field("text")?.text()?.to_owned(),
            })
        })
        .collect::<Result<_, ShapeError>>()?;
    Ok(ChatgptPlan {
        title: plan.field("title")?.text()?.to_owned(),
        steps,
    })
}

/// Counts the steps of the current plan that `step_statuses_by_plan` marks completed. The last
/// step can stay `in_progress` after the research completed, so progress never decides the
/// status.
fn decode_progress(state: &Node<'_>, plan: &Node<'_>) -> Result<ChatgptProgress, ShapeError> {
    let plan_id = plan.field("plan_id")?.text()?;
    let statuses = state.field("step_statuses_by_plan")?.field(plan_id)?;
    let statuses = statuses
        .value
        .as_object()
        .ok_or_else(|| statuses.error("not an object"))?;
    let mut steps_completed = 0;
    for status in statuses.values() {
        let status = status.as_str().ok_or_else(|| {
            ShapeError(format!(
                "{}.step_statuses_by_plan.{plan_id}: a step status that is not a string",
                state.path()
            ))
        })?;
        if status == COMPLETED {
            steps_completed += 1;
        }
    }
    Ok(ChatgptProgress {
        steps_completed,
        steps_total: plan.field("steps")?.items()?.count(),
    })
}

/// The report with its citation markers rewritten. The title is the body's first level-one
/// heading, or the plan title.
fn decode_report(
    report: &Node<'_>,
    body: &str,
    plan: &Node<'_>,
) -> Result<DelegatedReport, ShapeError> {
    let metadata = report.field("metadata")?;
    let body_path = format!("{}.content.parts[0]", report.path());
    let (body, sources) = citations::rewrite(body, &body_path, &metadata)?;
    let title = match first_heading(&body) {
        Some(heading) => heading.to_owned(),
        None => plan.field("title")?.text()?.to_owned(),
    };
    Ok(DelegatedReport {
        title,
        body,
        sources,
    })
}

fn first_heading(body: &str) -> Option<&str> {
    body.lines()
        .find_map(|line| line.strip_prefix("# "))
        .map(str::trim)
        .filter(|heading| !heading.is_empty())
}
