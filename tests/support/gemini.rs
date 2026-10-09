//! Gemini web responses in the shapes recorded on 2026-10-09, rebuilt from structure only:
//! conversation and turn ids are replaced, and progress, report bodies, and citations are cut to
//! what each case needs. No recorded response body is kept in the repository.

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

/// One model turn: ids, the prompt, and the candidate whose `[12]` is the rich content and
/// whose `[30]` holds the research document.
fn turn(index: usize, prompt: &str, reply: &str, rich: Value, document: Option<Value>) -> Value {
    let mut candidate = vec![Value::Null; 31];
    candidate[0] = json!(format!("rc_{index:04}"));
    candidate[1] = json!([reply]);
    candidate[12] = rich;
    candidate[30] = document.map_or(Value::Null, |document| json!([document]));
    json!([
        [format!("c_{CONVERSATION}"), format!("r_{index:04}")],
        [
            format!("c_{CONVERSATION}"),
            format!("r_{index:04}"),
            format!("rc_{index:04}")
        ],
        [[prompt], 1, null, 0],
        [[Value::Array(candidate)]]
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

/// Points `gemini_browser` at `fake`.
pub(crate) fn config(fake: &FakeOpenCli) -> String {
    format!(
        "[providers.gemini_browser]\ncommand = {:?}\n",
        fake.executable().display().to_string()
    )
}
