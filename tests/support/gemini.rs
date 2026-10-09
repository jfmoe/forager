//! Gemini web responses in the shapes recorded on 2026-10-09, rebuilt from structure only:
//! conversation and turn ids are replaced, and progress, report bodies, and citations are cut to
//! what each case needs. No recorded response body is kept in the repository.

use std::fmt::Write as _;
use std::process::Output;

use serde_json::{Value, json};

use super::opencli::FakeOpenCli;

pub(crate) const CONTRACT: &str = "forager-gemini/1";
pub(crate) const CONVERSATION: &str = "a1b2c3d4e5f60718";
pub(crate) const CONVERSATION_URL: &str = "https://gemini.google.com/app/a1b2c3d4e5f60718";
const TASK_PLACEHOLDER: &str = "agency-placeholder-task-id";

/// A rich content block: a JSPB array whose high fields sit in the trailing object, keyed by
/// field number plus one.
fn rich_content(fields: &[(usize, Value)]) -> Value {
    let extension = fields
        .iter()
        .map(|(field, value)| ((field + 1).to_string(), value.clone()))
        .collect::<serde_json::Map<_, _>>();
    json!([null, ["rc_text"], null, Value::Object(extension)])
}

/// A JSPB array that holds `value` at index `field` itself.
fn indexed_field(field: usize, value: Value) -> Value {
    let mut array = vec![Value::Null; field];
    array.push(value);
    Value::Array(array)
}

/// The plan of the recorded run: title, steps, ETA, confirm label, confirm URL, modify label.
pub(crate) fn plan() -> Value {
    json!([
        "Rust HTML to Markdown Crates",
        [
            [
                1,
                "Research Websites",
                "Find maintained crates such as htmd and html2md."
            ],
            [
                2,
                "Analyze Results",
                "Compare table, code block, and link fidelity."
            ],
            [3, "Create Report", "Recommend one crate for a CLI tool."]
        ],
        "Ready in a few mins",
        ["Start research"],
        ["https://gemini.google.com/confirm", 0],
        ["Edit plan"]
    ])
}

/// A model candidate: its id, the reply text, the rich content in `[12]`, and the research
/// document in `[30]`.
fn candidate(index: usize, reply: &str, rich: Value, document: Option<Value>) -> Value {
    let mut candidate = vec![Value::Null; 31];
    candidate[0] = json!(format!("rc_{index:04}"));
    candidate[1] = json!([reply]);
    candidate[12] = rich;
    candidate[30] = document.map_or(Value::Null, |document| json!([document]));
    Value::Array(candidate)
}

/// One model turn: ids, the prompt, and its candidate.
fn turn(index: usize, prompt: &str, reply: &str, rich: Value, document: Option<Value>) -> Value {
    let candidate = candidate(index, reply, rich, document);
    json!([
        [format!("c_{CONVERSATION}"), format!("r_{index:04}")],
        [
            format!("c_{CONVERSATION}"),
            format!("r_{index:04}"),
            format!("rc_{index:04}")
        ],
        [[prompt], 1, null, 0],
        [[candidate]]
    ])
}

/// The plan turn after the first `StreamGenerate`: field 55 holds the plan, field 69 is 2.
pub(crate) fn plan_turn(index: usize) -> Value {
    turn(
        index,
        "Compare Rust crates that convert HTML to Markdown.",
        "Here's a research plan for that topic.",
        rich_content(&[(55, plan()), (69, json!(2))]),
        None,
    )
}

/// A research document as the candidate's `[30][0]` holds it: id, title, task id, body, the
/// citation container in `[5]`, and `[17]` mirroring the body next to another citation
/// container.
fn document(title: &str, body: &str, citations: Option<Value>) -> Value {
    let container = citations.map_or(Value::Null, |groups| indexed_field(43, groups));
    let mut document = vec![Value::Null; 18];
    document[0] = json!("im_0001");
    document[2] = json!(title);
    document[3] = json!(TASK_PLACEHOLDER);
    document[4] = json!(body);
    document[5] = container.clone();
    document[17] = json!([body, container]);
    Value::Array(document)
}

/// A progress item: a thought with its heading, or a visited source.
pub(crate) fn thought(heading: &str) -> Value {
    json!([
        null,
        null,
        null,
        null,
        null,
        [heading, "Details of the step."]
    ])
}

pub(crate) fn visited(url: &str) -> Value {
    json!([
        null,
        null,
        null,
        null,
        [
            null,
            null,
            ["https://www.google.com/s2/favicons", url, "Page", null, []]
        ]
    ])
}

/// The running turn after confirming: field 57 holds the progress items, field 69 is 3, and
/// the research document has a title but no body yet.
pub(crate) fn running_turn(index: usize, items: &[Value]) -> Value {
    let progress = json!([
        TASK_PLACEHOLDER,
        [
            null,
            null,
            null,
            null,
            ["Researching websites", null, items]
        ]
    ]);
    turn(
        index,
        "Start research",
        "I'm on it.",
        rich_content(&[(57, progress), (69, json!(3))]),
        Some(document("Rust HTML to Markdown Crates", "", None)),
    )
}

pub(crate) const REPORT_BODY: &str = "# Rust HTML to Markdown Crates\n\n| Crate | Tables |\n|---|---|\n| htmd | yes |\n\nhtmd keeps tables [cite: 2, 1]. html2md is older [cite: 3].\n";

/// One citation group: the marker text and the entries in marker order.
pub(crate) fn citation_group(marker: &str, entries: &[(&str, &str)]) -> Value {
    let entries = entries
        .iter()
        .map(|(url, title)| {
            json!([
                null,
                null,
                null,
                [[
                    "https://www.google.com/s2/favicons",
                    url,
                    title,
                    null,
                    null,
                    "Snippet."
                ]]
            ])
        })
        .collect::<Vec<_>>();
    json!([[marker], entries])
}

/// The completed turn: field 69 is 5 and the research document holds the body and citations.
pub(crate) fn completed_turn(index: usize, citations: Value) -> Value {
    turn(
        index,
        "Start research",
        "I've completed your research.",
        rich_content(&[(69, json!(5))]),
        Some(document(
            "Rust HTML to Markdown Crates",
            REPORT_BODY,
            Some(citations),
        )),
    )
}

/// The citations of the completed report: number 2 appears twice, and its first entry wins.
pub(crate) fn report_citations() -> Value {
    json!([
        citation_group(
            "[cite: 2, 1]",
            &[
                ("https://github.com/letmutex/htmd", "htmd on GitHub"),
                ("https://crates.io/crates/htmd", "htmd on crates.io"),
            ],
        ),
        citation_group(
            "[cite: 3, 2]",
            &[
                ("https://crates.io/crates/html2md", "html2md"),
                ("https://example.com/later-duplicate", "Later duplicate"),
            ],
        ),
    ])
}

/// A research turn whose status field holds `status`, or is absent, next to a research
/// document holding `body`.
pub(crate) fn research_turn(index: usize, status: Option<u64>, body: &str) -> Value {
    let fields = status.map_or_else(Vec::new, |status| vec![(69, json!(status))]);
    turn(
        index,
        "Start research",
        "I'm on it.",
        rich_content(&fields),
        Some(document("Rust HTML to Markdown Crates", body, None)),
    )
}

/// A plain chat turn without a plan or a research document.
pub(crate) fn plain_turn(index: usize) -> Value {
    turn(
        index,
        "Thanks, what about html2text?",
        "html2text renders plain text instead of Markdown.",
        rich_content(&[]),
        None,
    )
}

/// The `hNvQHb` batchexecute response body: the `)]}'` guard, then length-prefixed chunks whose
/// `wrb.fr` envelope carries the turn list as a JSON string.
pub(crate) fn hnvqhb_body(turns: &[Value]) -> String {
    let inner = json!([turns, null, null, []]).to_string();
    let envelope = json!([
        ["wrb.fr", "hNvQHb", inner, null, null, null, "generic"],
        ["di", 87],
        ["af.httprm", 86, "-2814466390532545021", 3]
    ])
    .to_string();
    let trailer = json!([["e", 4, null, null, envelope.len()]]).to_string();
    format!(
        ")]}}'\n\n{}\n{envelope}\n{}\n{trailer}\n",
        envelope.len(),
        trailer.len()
    )
}

/// The page facts of a `report` command that read `body` at the conversation page.
pub(crate) fn report(body: &str) -> Value {
    json!({
        "page": {"url": CONVERSATION_URL, "signed_out": false, "notice": null},
        "response": body,
        "body_missing": false,
        "timed_out": false
    })
}

/// The page facts of a `report` command that received no conversation response at `url`.
pub(crate) fn unread(url: &str) -> Value {
    json!({
        "page": {"url": url, "signed_out": false, "notice": null},
        "response": null,
        "body_missing": false,
        "timed_out": false
    })
}

/// A fake `opencli` whose `report` answers the conversation with `turns`.
pub(crate) fn reporting(turns: &[Value]) -> FakeOpenCli {
    answering(report(&hnvqhb_body(turns)))
}

/// A fake `opencli` whose `report` answers `data` as its page facts.
pub(crate) fn answering(data: Value) -> FakeOpenCli {
    FakeOpenCli::contract_by_command(CONTRACT, &[("report", data)])
}

/// A fake `opencli` whose `status` answers `data` as its page facts.
pub(crate) fn status_answering(data: Value) -> FakeOpenCli {
    FakeOpenCli::contract_by_command(CONTRACT, &[("status", data)])
}

/// The page facts of a `status` command at the Gemini app.
pub(crate) fn status(signed_out: bool) -> Value {
    json!({
        "page": {"url": "https://gemini.google.com/app", "signed_out": signed_out, "notice": null},
        "timed_out": false
    })
}

/// The candidate of the first `StreamGenerate`: field 55 holds the plan, field 69 is 2.
pub(crate) fn plan_candidate() -> Value {
    candidate(
        1,
        "Here's a research plan for that topic.",
        rich_content(&[(55, plan()), (69, json!(2))]),
        None,
    )
}

/// The candidate of the confirming `StreamGenerate`: field 69 is 3 and the research document
/// has a title but no body yet.
pub(crate) fn started_candidate() -> Value {
    candidate(
        2,
        "I'm on it.",
        rich_content(&[(69, json!(3))]),
        Some(document("Rust HTML to Markdown Crates", "", None)),
    )
}

/// A reply candidate with text only: no plan and no research document.
pub(crate) fn text_candidate(reply: &str) -> Value {
    candidate(1, reply, rich_content(&[]), None)
}

/// A `StreamGenerate` response body: the `)]}'` guard, then length-prefixed frames whose
/// `wrb.fr` parts carry growing snapshots of the reply. The first snapshot holds only the start
/// of the reply text; the last one holds `candidate`.
pub(crate) fn stream_body(candidate: &Value) -> String {
    let ids = json!([format!("c_{CONVERSATION}"), "r_0001"]);
    let partial = json!([null, ids, null, null, [[candidate[0], ["Here"]]]]);
    let complete = json!([null, ids, null, null, [candidate]]);
    frames(&[
        json!([["wrb.fr", null, partial.to_string()]]),
        json!([["wrb.fr", null, complete.to_string()]]),
        json!([["di", 4120], ["af.httprm", 4119, "-1", 21]]),
    ])
}

/// A `StreamGenerate` response body whose only part carries Gemini's error `code` at
/// `[5][2][0][1][0]`.
pub(crate) fn stream_error_body(code: u64) -> String {
    frames(&[json!([[
        "wrb.fr",
        null,
        null,
        null,
        null,
        [
            3,
            null,
            [[
                "type.googleapis.com/assistant.boq.bard.application.BardErrorInfo",
                [code]
            ]]
        ]
    ]])])
}

fn frames(frames: &[Value]) -> String {
    let mut body = ")]}'\n".to_owned();
    for frame in frames {
        let text = frame.to_string();
        let _ = write!(body, "\n{}\n{text}", text.len());
    }
    body.push('\n');
    body
}

/// Every step of a `start` command, in the order the adapter reaches them.
pub(crate) const START_STEPS: [&str; 7] = [
    "tools_menu",
    "deep_research",
    "query",
    "sent",
    "answered",
    "confirmed",
    "started",
];

/// The page facts of a `start` command at the conversation page after `steps`, with the
/// `StreamGenerate` bodies it read.
pub(crate) fn start_facts(steps: &[&str], plan: Option<String>, confirm: Option<String>) -> Value {
    let [plan, confirm] = [plan, confirm].map(|body| body.map_or(Value::Null, Value::String));
    json!({
        "page": {"url": CONVERSATION_URL, "signed_out": false, "notice": null},
        "steps": steps,
        "deep_research_missing": false,
        "deep_research_disabled": false,
        "quota_notice": null,
        "plan_response": plan,
        "confirm_response": confirm,
        "timed_out": false
    })
}

/// The page facts of a `start` command that reached every step.
pub(crate) fn started_facts() -> Value {
    start_facts(
        &START_STEPS,
        Some(stream_body(&plan_candidate())),
        Some(stream_body(&started_candidate())),
    )
}

/// A fake `opencli` whose `start` answers `data` as its page facts.
pub(crate) fn starting(data: Value) -> FakeOpenCli {
    FakeOpenCli::contract_by_command(CONTRACT, &[("start", data)])
}

/// Points `gemini_browser` at `fake`.
pub(crate) fn config(fake: &FakeOpenCli) -> String {
    format!(
        "[providers.gemini_browser]\ncommand = {:?}\n",
        fake.executable().display().to_string()
    )
}

/// The JSON stdout of a command.
pub(crate) fn payload(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "parse JSON stdout: {error}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// Exit code, error kind, message, and conversation URL of a failure payload.
pub(crate) fn failure(output: &Output) -> (Option<i32>, Value, String, Value) {
    let payload = payload(output);
    (
        output.status.code(),
        payload["error_kind"].clone(),
        payload["message"].as_str().unwrap_or_default().to_owned(),
        payload["conversation_url"].clone(),
    )
}
