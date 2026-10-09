//! Pure decoding of the responses the Gemini page itself received: batchexecute frames, JSPB
//! fields, and the research state of a conversation's turns. Field positions follow the live
//! responses of 2026-10-09 (`docs/research/2026-10-09-gemini-deep-research.md`).

use std::collections::BTreeMap;

use serde_json::Value;

use crate::types::{
    GeminiPlan, GeminiPlanStep, GeminiProgress, GeminiReport, GeminiResearchState, GeminiSource,
};

// Rich content fields of a candidate, numbered from zero as the page's JSPB does.
const PLAN_FIELD: usize = 55;
const PROGRESS_FIELD: usize = 57;
const STATUS_FIELD: usize = 69;
const CITATIONS_FIELD: usize = 43;
const AWAITING_CONFIRMATION: u64 = 2;
const RUNNING: u64 = 3;
const COMPLETED: u64 = 5;

/// Where a response stopped matching the shape forager decodes.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub(super) struct ShapeError(String);

/// A JSON value and its path from the root of the response, for shape errors.
#[derive(Clone)]
struct Node<'a> {
    value: &'a Value,
    path: String,
}

impl<'a> Node<'a> {
    fn root(value: &'a Value, path: &str) -> Self {
        Self {
            value,
            path: path.to_owned(),
        }
    }

    /// The non-null element at `index`, or `None`.
    fn get(&self, index: usize) -> Option<Self> {
        self.value
            .as_array()?
            .get(index)
            .filter(|value| !value.is_null())
            .map(|value| Self {
                value,
                path: format!("{}[{index}]", self.path),
            })
    }

    fn at(&self, index: usize) -> Result<Self, ShapeError> {
        self.get(index)
            .ok_or_else(|| self.error(&format!("no element [{index}]")))
    }

    /// Field `number` of a JSPB array: the element at that index, or the entry keyed by the
    /// number plus one in the trailing object that holds the high fields.
    fn field(&self, number: usize) -> Option<Self> {
        let array = self.value.as_array()?;
        let extension = array.last().and_then(Value::as_object);
        let direct = array
            .get(number)
            .filter(|value| !(value.is_null() || value.is_object() && number + 1 == array.len()));
        if let Some(value) = direct {
            return Some(Self {
                value,
                path: format!("{}[{number}]", self.path),
            });
        }
        let key = (number + 1).to_string();
        extension?
            .get(&key)
            .filter(|value| !value.is_null())
            .map(|value| Self {
                value,
                path: format!("{} field {number}", self.path),
            })
    }

    fn array(&self) -> Result<&'a [Value], ShapeError> {
        self.value
            .as_array()
            .map(Vec::as_slice)
            .ok_or_else(|| self.error("not an array"))
    }

    fn items(&self) -> Result<impl Iterator<Item = Node<'a>>, ShapeError> {
        let path = self.path.clone();
        Ok(self
            .array()?
            .iter()
            .enumerate()
            .map(move |(index, value)| Node {
                value,
                path: format!("{path}[{index}]"),
            }))
    }

    fn text(&self) -> Result<&'a str, ShapeError> {
        self.value
            .as_str()
            .ok_or_else(|| self.error("not a string"))
    }

    /// The text at `index`, or `None` when the element is absent.
    fn optional_text(&self, index: usize) -> Result<Option<&'a str>, ShapeError> {
        self.get(index).map(|text| text.text()).transpose()
    }

    /// The status field of a rich content block and its value.
    fn status(&self) -> Result<(Self, u64), ShapeError> {
        let status = self
            .field(STATUS_FIELD)
            .ok_or_else(|| self.error(&format!("no status field {STATUS_FIELD}")))?;
        let value = status.number()?;
        Ok((status, value))
    }

    fn number(&self) -> Result<u64, ShapeError> {
        self.value
            .as_u64()
            .ok_or_else(|| self.error("not a whole number"))
    }

    fn error(&self, problem: &str) -> ShapeError {
        ShapeError(format!("{}: {problem}", self.path))
    }
}

/// Returns the `wrb.fr` envelopes of a response body, in order: the `)]}'` guard, then
/// length-prefixed JSON chunks, each an array of envelopes. `label` names the response in
/// errors.
fn envelopes(body: &str, label: &str) -> Result<Vec<Vec<Value>>, ShapeError> {
    let chunks = body
        .trim_start()
        .strip_prefix(")]}'")
        .ok_or_else(|| ShapeError(format!("{label}: the response lacks the `)]}}'` guard")))?;
    let mut found = Vec::new();
    // The length prefixes parse as numbers between the chunks, so every array is a chunk.
    for chunk in serde_json::Deserializer::from_str(chunks).into_iter::<Value>() {
        let chunk =
            chunk.map_err(|error| ShapeError(format!("{label}: a chunk is not JSON: {error}")))?;
        let Value::Array(envelopes) = chunk else {
            continue;
        };
        found.extend(envelopes.into_iter().filter_map(|envelope| match envelope {
            Value::Array(fields) if fields.first().and_then(Value::as_str) == Some("wrb.fr") => {
                Some(fields)
            }
            _ => None,
        }));
    }
    Ok(found)
}

fn inner_payload(fields: &[Value], label: &str) -> Result<Value, ShapeError> {
    let payload = fields
        .get(2)
        .and_then(Value::as_str)
        .ok_or_else(|| ShapeError(format!("{label}: the envelope carries no payload")))?;
    serde_json::from_str(payload)
        .map_err(|error| ShapeError(format!("{label}: the payload is not JSON: {error}")))
}

/// Returns the payload of the `rpc` call in a batchexecute response body, whose `wrb.fr`
/// envelopes carry each payload as a JSON string after the rpc id.
///
/// # Errors
///
/// Fails when the body is no batchexecute response or carries no payload for `rpc`.
pub(super) fn batchexecute_payload(body: &str, rpc: &str) -> Result<Value, ShapeError> {
    let fields = envelopes(body, rpc)?
        .into_iter()
        .find(|fields| fields.get(1).and_then(Value::as_str) == Some(rpc))
        .ok_or_else(|| ShapeError(format!("{rpc}: the response has no `wrb.fr` envelope")))?;
    inner_payload(&fields, rpc)
}

const STREAM: &str = "StreamGenerate";

/// What one `StreamGenerate` response says about the turn it answered.
#[derive(Debug, Default)]
pub(super) struct StreamReply {
    /// The id of the conversation, without its `c_` prefix.
    pub(super) conversation: Option<String>,
    /// The error code Gemini answered instead of a reply, such as 1037 for an exhausted quota.
    pub(super) error_code: Option<u64>,
    /// The newest snapshot of the reply's first candidate: each envelope repeats the reply so
    /// far, so the last one is the most complete.
    pub(super) candidate: Option<Value>,
}

/// Reads a `StreamGenerate` response body. Each envelope carries a snapshot of the reply as a
/// JSON string at `[2]`, whose `[1][0]` is the conversation and whose `[4]` lists the
/// candidates; an envelope that carries an error has its code at `[5][2][0][1][0]`.
///
/// # Errors
///
/// Fails when the body is no stream of envelopes or a snapshot is not JSON.
pub(super) fn stream_reply(body: &str) -> Result<StreamReply, ShapeError> {
    let mut reply = StreamReply::default();
    for fields in envelopes(body, STREAM)? {
        let error_code = fields
            .get(5)
            .and_then(|error| error.pointer("/2/0/1/0"))
            .and_then(Value::as_u64);
        reply.error_code = reply.error_code.or(error_code);
        if fields.get(2).is_none_or(Value::is_null) {
            continue;
        }
        let snapshot = inner_payload(&fields, STREAM)?;
        if let Some(conversation) = snapshot
            .pointer("/1/0")
            .and_then(Value::as_str)
            .and_then(|id| id.strip_prefix("c_"))
        {
            reply.conversation = Some(conversation.to_owned());
        }
        if let Some(candidate) = snapshot.pointer("/4/0") {
            reply.candidate = Some(candidate.clone());
        }
    }
    Ok(reply)
}

/// How Gemini answered the question of a new Deep Research.
pub(super) enum PlanReply {
    /// A research plan that waits for confirmation.
    Plan(GeminiPlan),
    /// Plain reply text instead of a plan, such as a refusal.
    Text(String),
}

/// Judges the reply candidate of the first `StreamGenerate`: a plan with status 2, or text.
///
/// # Errors
///
/// Fails with the location where a plan stops matching the recorded shape.
pub(super) fn plan_reply(candidate: &Value) -> Result<PlanReply, ShapeError> {
    let candidate = Node::root(candidate, "StreamGenerate[4][0]");
    let rich = candidate.get(12);
    let Some((rich, plan)) = rich.and_then(|rich| rich.field(PLAN_FIELD).map(|plan| (rich, plan)))
    else {
        let text = candidate
            .get(1)
            .and_then(|reply| reply.get(0))
            .and_then(|text| text.value.as_str())
            .unwrap_or_default();
        return Ok(PlanReply::Text(text.to_owned()));
    };
    let (status, value) = rich.status()?;
    match value {
        AWAITING_CONFIRMATION => decode_plan(&plan).map(PlanReply::Plan),
        other => Err(status.error(&format!("a plan with status {other}"))),
    }
}

/// Checks that the reply candidate of the confirming `StreamGenerate` started the research:
/// status 3 next to a research document.
///
/// # Errors
///
/// Fails with the location where the candidate stops matching a started research.
pub(super) fn research_started(candidate: &Value) -> Result<(), ShapeError> {
    let candidate = Node::root(candidate, "StreamGenerate[4][0]");
    let (status, value) = candidate.at(12)?.status()?;
    match value {
        RUNNING => {
            let task = candidate.at(30)?.at(0)?.at(3)?;
            if task.text()?.is_empty() {
                return Err(task.error("an empty research task id"));
            }
            Ok(())
        }
        other => Err(status.error(&format!("status {other} after confirming the plan"))),
    }
}

/// A turn that holds a research plan or a research document.
struct ResearchTurn<'a> {
    rich: Option<Node<'a>>,
    plan: Option<Node<'a>>,
    document: Option<Node<'a>>,
}

/// Judges the newest research turn of a `hNvQHb` payload, whose turns are newest first. Returns
/// `None` when no loaded turn holds a plan or a research document. An inconsistent newest
/// research turn fails: an older completed report never stands in for it.
///
/// # Errors
///
/// Fails with the location where the payload stops matching the recorded shape.
pub(super) fn research_state(payload: &Value) -> Result<Option<GeminiResearchState>, ShapeError> {
    let turns = Node::root(payload, "hNvQHb").at(0)?;
    for turn in turns.items()? {
        if let Some(research) = research_turn(&turn) {
            return judge(&research).map(Some);
        }
    }
    Ok(None)
}

fn research_turn<'a>(turn: &Node<'a>) -> Option<ResearchTurn<'a>> {
    let candidate = turn.get(3)?.get(0)?.get(0)?;
    let rich = candidate.get(12);
    let plan = rich.as_ref().and_then(|rich| rich.field(PLAN_FIELD));
    let document = candidate
        .get(30)
        .and_then(|documents| documents.get(0))
        .filter(|document| {
            document
                .get(3)
                .and_then(|task| task.value.as_str().map(|task| !task.is_empty()))
                .unwrap_or(false)
        });
    (plan.is_some() || document.is_some()).then_some(ResearchTurn {
        rich,
        plan,
        document,
    })
}

fn judge(turn: &ResearchTurn<'_>) -> Result<GeminiResearchState, ShapeError> {
    let rich = turn
        .rich
        .as_ref()
        .ok_or_else(|| ShapeError("the research turn has no rich content".into()))?;
    let (status_node, status) = rich.status()?;
    let body = match &turn.document {
        Some(document) => document.optional_text(4)?,
        None => None,
    }
    .unwrap_or_default();
    match status {
        AWAITING_CONFIRMATION => {
            let plan = turn
                .plan
                .as_ref()
                .ok_or_else(|| rich.error("status 2 without a plan"))?;
            decode_plan(plan).map(GeminiResearchState::AwaitingConfirmation)
        }
        RUNNING if body.is_empty() => {
            decode_progress(rich.field(PROGRESS_FIELD)).map(GeminiResearchState::Running)
        }
        RUNNING => Err(status_node.error("status 3 with a report body")),
        COMPLETED => match &turn.document {
            Some(document) if !body.is_empty() => {
                decode_report(document, body).map(GeminiResearchState::Completed)
            }
            _ => Err(status_node.error("status 5 without a report body")),
        },
        other => Err(status_node.error(&format!("unknown status {other}"))),
    }
}

fn decode_plan(plan: &Node<'_>) -> Result<GeminiPlan, ShapeError> {
    let steps = plan
        .at(1)?
        .items()?
        .map(|step| {
            Ok(GeminiPlanStep {
                index: step.at(0)?.number()?,
                label: step.at(1)?.text()?.to_owned(),
                description: step.optional_text(2)?.unwrap_or_default().to_owned(),
            })
        })
        .collect::<Result<_, ShapeError>>()?;
    Ok(GeminiPlan {
        title: plan.at(0)?.text()?.to_owned(),
        steps,
        eta_text: plan.optional_text(2)?.map(ToOwned::to_owned),
    })
}

/// Counts the progress items: a thought has `[5] = [heading, text]`, a visited source has
/// `[4][2] = [favicon, url, title, …]`. The newest thought is the last one listed.
fn decode_progress(progress: Option<Node<'_>>) -> Result<GeminiProgress, ShapeError> {
    let Some(items) = progress.and_then(|progress| progress.get(1)?.get(4)?.get(2)) else {
        return Ok(GeminiProgress::default());
    };
    let mut decoded = GeminiProgress::default();
    for item in items.items()? {
        if let Some(thought) = item.get(5) {
            decoded.thoughts += 1;
            decoded.latest_thought = Some(thought.at(0)?.text()?.to_owned());
        } else if item.get(4).and_then(|source| source.get(2)).is_some() {
            decoded.sources_visited += 1;
        }
    }
    Ok(decoded)
}

fn decode_report(document: &Node<'_>, body: &str) -> Result<GeminiReport, ShapeError> {
    let container = document
        .get(17)
        .and_then(|mirror| mirror.get(1))
        .or_else(|| document.get(5));
    let sources = match container.and_then(|container| container.field(CITATIONS_FIELD)) {
        Some(groups) => decode_sources(&groups)?,
        None => Vec::new(),
    };
    Ok(GeminiReport {
        title: document.at(2)?.text()?.to_owned(),
        body: body.to_owned(),
        sources,
    })
}

/// Pairs each citation group's numbers, read from its marker such as `[cite: 2, 3, 4]`, with
/// its entries in marker order. The first entry of a number wins; sources come out by number.
fn decode_sources(groups: &Node<'_>) -> Result<Vec<GeminiSource>, ShapeError> {
    let mut sources = BTreeMap::new();
    for group in groups.items()? {
        let marker = group.at(0)?.at(0)?;
        let numbers = citation_numbers(marker.text()?)
            .ok_or_else(|| marker.error("not a `[cite: N, …]` marker"))?;
        for (number, entry) in numbers.into_iter().zip(group.at(1)?.items()?) {
            if sources.contains_key(&number) {
                continue;
            }
            let link = entry.at(3)?.at(0)?;
            let title = link.optional_text(2)?.unwrap_or_default().to_owned();
            sources.insert(
                number,
                GeminiSource {
                    id: number,
                    title,
                    url: link.at(1)?.text()?.to_owned(),
                },
            );
        }
    }
    Ok(sources.into_values().collect())
}

fn citation_numbers(marker: &str) -> Option<Vec<u64>> {
    marker
        .trim()
        .strip_prefix("[cite:")?
        .strip_suffix(']')?
        .split(',')
        .map(|number| number.trim().parse().ok())
        .collect()
}
