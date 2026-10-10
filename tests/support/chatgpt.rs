//! ChatGPT `GET /backend-api/conversations/<id>` responses in the shape recorded on 2026-10-10,
//! rebuilt from structure only: conversation, message, request, and widget session ids are
//! replaced, the usage receipt keeps no identity or metering detail, and report bodies and
//! sources are cut to what each case needs. No recorded response body is kept in the
//! repository.

use std::process::Output;

use serde_json::{Value, json};

use super::opencli::FakeOpenCli;

pub(crate) const CONTRACT: &str = "forager-chatgpt/1";
pub(crate) const CONVERSATION: &str = "6a0f1c2e-3b4d-4e5f-8a9b-0c1d2e3f4a5b";
pub(crate) const CONVERSATION_URL: &str =
    "https://chatgpt.com/c/6a0f1c2e-3b4d-4e5f-8a9b-0c1d2e3f4a5b";
pub(crate) const PLAN_TITLE: &str = "async-std 維護狀態與替代方案";

const PLAN_ID: &str = "plan-0001";
const CITE_START: char = '\u{e200}';
const CITE_SEPARATOR: char = '\u{e202}';
const CITE_END: char = '\u{e201}';

/// The plan of the recorded run: an id, a title, and steps with ids and text.
pub(crate) fn plan() -> Value {
    json!({
        "plan_id": PLAN_ID,
        "title": PLAN_TITLE,
        "steps": [
            {"id": "step-1", "text": "查閱 async-std 倉庫與公告"},
            {"id": "step-2", "text": "整理官方推薦的替代方案"},
            {"id": "step-3", "text": "比較 smol 與 tokio"},
            {"id": "step-4", "text": "撰寫摘要"}
        ]
    })
}

/// The current widget state with `status`, the recorded plan, the step statuses of that plan,
/// and the report message when one is given. Fields absent from a recorded state stay absent.
pub(crate) fn widget_state(status: &str, step_statuses: &[&str], report: Option<Value>) -> Value {
    let statuses = step_statuses
        .iter()
        .enumerate()
        .map(|(index, status)| (format!("step-{}", index + 1), json!(status)))
        .collect::<serde_json::Map<_, _>>();
    let mut state = json!({
        "status": status,
        "plan": plan(),
        "step_statuses_by_plan": {PLAN_ID: statuses},
        "last_updated_at": "2026-10-10T13:12:51.000000Z",
        "waiting_for_user_response_on_plan_until": "2026-10-10T13:12:48.000000Z"
    });
    if status != "waiting_for_user_response_on_plan" {
        state["research_started_at"] = json!("2026-10-10T13:12:51.000000Z");
    }
    if let Some(report) = report {
        state["report_message"] = report;
        state["research_stopped_at"] = json!("2026-10-10T13:16:33.000000Z");
    }
    state
}

/// The usage receipt of a run that stayed within the plan's limit, without identity or
/// metering detail.
pub(crate) fn receipt_within_limit() -> Value {
    json!({
        "allowed": true,
        "outcome": "rate_limit_not_reached",
        "rate_limit": {"limit_reached": false}
    })
}

/// The `api_tool.call_tool` tool message of a research. `tool_response_metadata` holds the
/// snapshot `venus_widget_state`, taken while the plan waited for confirmation, as an object.
/// ChatGPT writes `widget_state`, the current state as a JSON string, only once the research
/// starts, so a `state` that still waits on its plan leaves the message without one.
pub(crate) fn research_message_with(index: usize, state: &Value, receipt: &Value) -> Value {
    let mut sdk = json!({
        "tool_response_metadata": {
            "openai/asyncStatus": 7,
            "openai/widgetSessionId": "widget-session-placeholder",
            "venus_message_type": "initial_loading_message",
            "venus_widget_state": awaiting_state(),
            "usage_refund_receipt": receipt
        }
    });
    if state["status"] != "waiting_for_user_response_on_plan" {
        sdk["widget_state"] = json!(state.to_string());
    }
    json!({
        "id": format!("msg-{index:04}"),
        "author": {"role": "tool", "name": "api_tool.call_tool", "metadata": {}},
        "recipient": "all",
        "create_time": 1_791_637_908 + index,
        "content": {"content_type": "text", "parts": [""]},
        "metadata": {"chatgpt_sdk": sdk}
    })
}

pub(crate) fn research_message(index: usize, state: &Value) -> Value {
    research_message_with(index, state, &receipt_within_limit())
}

fn message(index: usize, role: &str, name: Option<&str>, text: &str) -> Value {
    json!({
        "id": format!("msg-{index:04}"),
        "author": {"role": role, "name": name, "metadata": {}},
        "recipient": "all",
        "create_time": 1_791_637_904 + index,
        "content": {"content_type": "text", "parts": [text]},
        "metadata": {}
    })
}

pub(crate) fn user_message(index: usize, text: &str) -> Value {
    message(index, "user", None, text)
}

pub(crate) fn assistant_message(index: usize, text: &str) -> Value {
    message(index, "assistant", None, text)
}

/// The messages of one research turn as the page loaded them, oldest first: the prompt, the
/// tool call, the research message, the `functions.exec` result, and the short notice that the
/// research started.
pub(crate) fn research_turn(first: usize, state: &Value) -> Vec<Value> {
    vec![
        message(first, "system", None, ""),
        user_message(first + 1, "用不超過 500 字總結 async-std 的維護狀態。"),
        message(
            first + 2,
            "assistant",
            None,
            "{\"path\": \"deep_research\"}",
        ),
        research_message(first + 3, state),
        message(first + 4, "tool", Some("functions.exec"), ""),
        assistant_message(first + 5, "已開始深度研究。"),
    ]
}

/// The conversation response body: the conversation id, the flat message list, and the page
/// cursors.
pub(crate) fn conversation_body(messages: &[Value]) -> String {
    conversation_body_of(CONVERSATION, messages)
}

pub(crate) fn conversation_body_of(conversation: &str, messages: &[Value]) -> String {
    json!({
        "conversation_id": conversation,
        "title": "async-std",
        "async_status": null,
        "messages": messages,
        "page_info": {"has_more_before": false, "before_cursor": null}
    })
    .to_string()
}

/// The `(safe URL, item title)` pairs that one citation marker cites.
pub(crate) type CitedUrls<'a> = Vec<(&'a str, Option<&'a str>)>;

/// A citation marker as the report body carries it.
pub(crate) fn marker(ids: &[&str]) -> String {
    let mut marker = format!("{CITE_START}cite");
    for id in ids {
        marker.push(CITE_SEPARATOR);
        marker.push_str(id);
    }
    marker.push(CITE_END);
    marker
}

/// A report body and its `grouped_webpages` references: `parts` alternate between text and the
/// references of one marker, each a list of `(safe URL, item title)` pairs. The references
/// carry the character span of their marker.
pub(crate) fn cited_body(parts: &[(&str, Option<CitedUrls<'_>>)]) -> (String, Vec<Value>) {
    let mut body = String::new();
    let mut references = Vec::new();
    for (index, (text, cited)) in parts.iter().enumerate() {
        body.push_str(text);
        let Some(cited) = cited else {
            continue;
        };
        let ids = (0..cited.len())
            .map(|position| format!("turn0search{}", index * 10 + position))
            .collect::<Vec<_>>();
        let ids = ids.iter().map(String::as_str).collect::<Vec<_>>();
        let marker = marker(&ids);
        let start = body.chars().count();
        body.push_str(&marker);
        let items = cited
            .iter()
            .filter_map(|(url, title)| {
                title.map(|title| json!({"title": title, "url": url, "attribution": "example", "snippet": "Snippet."}))
            })
            .collect::<Vec<_>>();
        references.push(json!({
            "matched_text": marker,
            "start_idx": start,
            "end_idx": start + marker.chars().count(),
            "type": "grouped_webpages",
            "items": items,
            "safe_urls": cited.iter().map(|(url, _)| *url).collect::<Vec<_>>()
        }));
    }
    (body, references)
}

/// The report of the recorded run, cut down: four distinct sources over three markers. Each
/// URL also appears with `utm_source=chatgpt.com`, one URL repeats across markers, one title
/// comes only from the search result groups, and one source has no title anywhere.
pub(crate) fn report_body_and_references() -> (String, Vec<Value>) {
    let (body, mut references) = cited_body(&[
        (
            "# async-std 的維護狀態\n\nasync-std 已於 2025 年停止維護",
            Some(vec![
                ("https://github.com/async-rs/async-std", None),
                (
                    "https://github.com/async-rs/async-std?utm_source=chatgpt.com",
                    Some("async-rs/async-std"),
                ),
                (
                    "https://blog.example.org/async-std-eol?utm_source=chatgpt.com",
                    None,
                ),
                ("https://blog.example.org/async-std-eol", None),
            ]),
        ),
        (
            "。官方建議改用 smol",
            Some(vec![
                (
                    "https://github.com/smol-rs/smol?utm_source=chatgpt.com",
                    Some("smol-rs/smol"),
                ),
                ("https://github.com/async-rs/async-std", None),
            ]),
        ),
        (
            "。\n\n| 替代 | 說明 |\n|---|---|\n| smol | 輕量",
            Some(vec![(
                "https://docs.rs/smol?lang=en&utm_source=chatgpt.com",
                None,
            )]),
        ),
        (" |\n", None),
    ]);
    let end = body.chars().count();
    references.push(json!({
        "matched_text": "",
        "start_idx": end,
        "end_idx": end,
        "type": "sources_footnote",
        "sources": [{"title": "async-rs/async-std", "url": "https://github.com/async-rs/async-std"}]
    }));
    (body, references)
}

/// The search result groups of the recorded report: they title one cited URL that no
/// reference item titles.
pub(crate) fn search_result_groups() -> Value {
    json!([{
        "type": "search_result_group",
        "domain": "blog.example.org",
        "entries": [{
            "type": "search_result",
            "url": "https://blog.example.org/async-std-eol?utm_source=chatgpt.com",
            "title": "async-std end of life",
            "snippet": "Snippet.",
            "ref_id": {"turn_index": 0, "ref_type": "search", "ref_index": 2}
        }]
    }])
}

/// The report message inside the widget state: the Markdown body and its references.
pub(crate) fn report_message(body: &str, references: &[Value]) -> Value {
    json!({
        "id": "report-0001",
        "author": {"role": "assistant", "metadata": {"real_author": "tool:web"}},
        "content": {"content_type": "text", "parts": [body]},
        "status": "finished_successfully",
        "channel": "final",
        "metadata": {
            "content_references": references,
            "search_result_groups": search_result_groups()
        }
    })
}

/// The widget state of the completed recorded run.
pub(crate) fn completed_state() -> Value {
    let (body, references) = report_body_and_references();
    widget_state(
        "completed",
        &["completed", "completed", "completed", "in_progress"],
        Some(report_message(&body, &references)),
    )
}

pub(crate) fn running_state() -> Value {
    widget_state(
        "researching",
        &["completed", "in_progress", "pending", "pending"],
        None,
    )
}

pub(crate) fn awaiting_state() -> Value {
    widget_state(
        "waiting_for_user_response_on_plan",
        &["pending", "pending", "pending", "pending"],
        None,
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

/// A fake `opencli` whose `report` answers the conversation with `messages`.
pub(crate) fn reporting(messages: &[Value]) -> FakeOpenCli {
    answering(report(&conversation_body(messages)))
}

/// A fake `opencli` whose `report` answers `data` as its page facts.
pub(crate) fn answering(data: Value) -> FakeOpenCli {
    FakeOpenCli::contract_by_command(CONTRACT, &[("report", data)])
}

/// A fake `opencli` whose `status` answers `data` as its page facts.
pub(crate) fn status_answering(data: Value) -> FakeOpenCli {
    FakeOpenCli::contract_by_command(CONTRACT, &[("status", data)])
}

/// The page facts of a `status` command at the ChatGPT home page.
pub(crate) fn status(signed_out: bool) -> Value {
    json!({
        "page": {"url": "https://chatgpt.com/", "signed_out": signed_out, "notice": null},
        "timed_out": false
    })
}

/// Points `chatgpt_browser` at `fake`.
pub(crate) fn config(fake: &FakeOpenCli) -> String {
    format!(
        "[providers.chatgpt_browser]\ncommand = {:?}\n",
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

/// The steps of the adapter's `start` command, in the order it reaches them.
pub(crate) const START_STEPS: [&str; 10] = [
    "chat_mode",
    "query",
    "deep_research",
    "mention",
    "sent",
    "conversation",
    "answered",
    "plan",
    "started",
    "reopened",
];

/// The page facts of a `start` command after `steps`, with the conversation response it read
/// after reopening the conversation. The page shows the conversation once the steps reach it,
/// and the home page before.
pub(crate) fn start_facts(steps: &[&str], response: Option<String>) -> Value {
    let created = steps.contains(&"conversation");
    json!({
        "page": {
            "url": if created { CONVERSATION_URL } else { "https://chatgpt.com/" },
            "signed_out": false,
            "notice": null
        },
        "steps": steps,
        "conversation": if created { json!(CONVERSATION) } else { Value::Null },
        "deep_research_missing": false,
        "quota_notice": null,
        "problem": null,
        "draft_cleared": null,
        "response": response.map_or(Value::Null, Value::String),
        "body_missing": false,
        "timed_out": false
    })
}

/// The page facts of a `start` command that reached every step and read the running research.
pub(crate) fn started_facts() -> Value {
    start_facts(
        &START_STEPS,
        Some(conversation_body(&research_turn(1, &running_state()))),
    )
}

/// A fake `opencli` whose `start` answers `data` as its page facts.
pub(crate) fn starting(data: Value) -> FakeOpenCli {
    FakeOpenCli::contract_by_command(CONTRACT, &[("start", data)])
}
