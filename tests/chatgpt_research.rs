//! `forager chatgpt research result` against a fake `opencli` executable that answers `report`
//! envelopes shaped like the live ChatGPT page facts of 2026-10-10. These tests prove the
//! transport, the page-fact classification, the response decoding, the citation rewriting, the
//! research selection, and the report delivery; only the live acceptance run exercises the
//! JavaScript adapter.
#![cfg(unix)]

mod support;

use std::fs;
use std::path::Path;
use std::process::Output;

use serde_json::{Value, json};

use support::RunEnvironment;
use support::chatgpt::{self, CONVERSATION, CONVERSATION_URL, failure, payload};
use support::opencli::FakeOpenCli;

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn result(environment: &RunEnvironment, conversation: &str, arguments: &[&str]) -> Output {
    let mut command = vec!["chatgpt", "research", "result", conversation];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

/// Reads `CONVERSATION` from a fake that answers `report` with `data`.
fn read(data: Value) -> (FakeOpenCli, Output) {
    let fake = chatgpt::answering(data);
    let environment = RunEnvironment::new(&chatgpt::config(&fake));
    let output = result(&environment, CONVERSATION, &[]);
    (fake, output)
}

/// Reads `CONVERSATION` from a fake whose conversation response lists `messages`.
fn read_messages(messages: &[Value]) -> Output {
    read(chatgpt::report(&chatgpt::conversation_body(messages))).1
}

fn report_dir(directory: &Path) -> String {
    directory.join("reports").display().to_string()
}

/// Removes the default report directory that a completed read wrote.
fn remove_report_directory(output: &Output) {
    if let Some(path) = payload(output)["report_path"].as_str()
        && let Some(directory) = Path::new(path).parent()
    {
        let _ = fs::remove_dir_all(directory);
    }
}

/// The recorded report after rewriting: every marker is a list of source numbers, and the
/// sources list follows the body.
const REPORT_MARKDOWN: &str = "# async-std 的維護狀態\n\nasync-std 已於 2025 年停止維護[1, 2]。官方建議改用 smol[3, 1]。\n\n| 替代 | 說明 |\n|---|---|\n| smol | 輕量[4] |\n\n## Sources\n\n- [1] async-rs/async-std <https://github.com/async-rs/async-std>\n- [2] async-std end of life <https://blog.example.org/async-std-eol>\n- [3] smol-rs/smol <https://github.com/smol-rs/smol>\n- [4] <https://docs.rs/smol?lang=en>\n";

fn report_sources() -> Value {
    json!([
        {"id": 1, "title": "async-rs/async-std", "url": "https://github.com/async-rs/async-std"},
        {"id": 2, "title": "async-std end of life", "url": "https://blog.example.org/async-std-eol"},
        {"id": 3, "title": "smol-rs/smol", "url": "https://github.com/smol-rs/smol"},
        {"id": 4, "title": null, "url": "https://docs.rs/smol?lang=en"}
    ])
}

const BACKGROUND_SESSION: [&str; 8] = [
    "-f",
    "json",
    "--window",
    "background",
    "--site-session",
    "ephemeral",
    "--keep-tab",
    "false",
];

#[test]
fn a_plan_awaiting_confirmation_returns_the_plan() {
    let fake = chatgpt::reporting(&chatgpt::research_turn(1, &chatgpt::awaiting_state()));
    let environment = RunEnvironment::new(&chatgpt::config(&fake));

    let output = result(&environment, CONVERSATION_URL, &[]);

    assert_eq!(
        (output.status.code(), payload(&output)),
        (
            Some(0),
            json!({
                "route": "chatgpt_browser",
                "conversation_id": "6a0f1c2e-3b4d-4e5f-8a9b-0c1d2e3f4a5b",
                "conversation_url": "https://chatgpt.com/c/6a0f1c2e-3b4d-4e5f-8a9b-0c1d2e3f4a5b",
                "status": "awaiting_confirmation",
                "plan": {
                    "title": "async-std 維護狀態與替代方案",
                    "steps": [
                        {"index": 1, "text": "查閱 async-std 倉庫與公告"},
                        {"index": 2, "text": "整理官方推薦的替代方案"},
                        {"index": 3, "text": "比較 smol 與 tokio"},
                        {"index": 4, "text": "撰寫摘要"}
                    ]
                }
            })
        ),
        "stderr: {}",
        stderr(&output)
    );
    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    let call = &calls[0];
    assert_eq!(
        (
            call[..4].to_vec(),
            call[call.len() - BACKGROUND_SESSION.len()..].to_vec()
        ),
        (
            ["forager-chatgpt", "report", "--conversation", CONVERSATION]
                .map(String::from)
                .to_vec(),
            BACKGROUND_SESSION.map(String::from).to_vec()
        )
    );
}

#[test]
fn a_running_research_reports_its_plan_title_and_progress() {
    let output = read_messages(&chatgpt::research_turn(1, &chatgpt::running_state()));

    assert_eq!(
        (output.status.code(), payload(&output)),
        (
            Some(0),
            json!({
                "route": "chatgpt_browser",
                "conversation_id": "6a0f1c2e-3b4d-4e5f-8a9b-0c1d2e3f4a5b",
                "conversation_url": "https://chatgpt.com/c/6a0f1c2e-3b4d-4e5f-8a9b-0c1d2e3f4a5b",
                "status": "running",
                "plan_title": "async-std 維護狀態與替代方案",
                "progress": {"steps_completed": 1, "steps_total": 4}
            })
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_completed_report_is_written_with_numbered_citations_and_clean_sources() {
    let fake = chatgpt::reporting(&chatgpt::research_turn(1, &chatgpt::completed_state()));
    let environment = RunEnvironment::new(&chatgpt::config(&fake));
    let directory = tempfile::tempdir().expect("create report directory");
    let reports = report_dir(directory.path());

    let output = result(&environment, CONVERSATION, &["--report-dir", &reports]);

    let report_path = format!("{reports}/chatgpt-{CONVERSATION}.md");
    let sources_path = format!("{reports}/chatgpt-{CONVERSATION}.sources.json");
    assert_eq!(
        (output.status.code(), payload(&output)),
        (
            Some(0),
            json!({
                "route": "chatgpt_browser",
                "conversation_id": "6a0f1c2e-3b4d-4e5f-8a9b-0c1d2e3f4a5b",
                "conversation_url": "https://chatgpt.com/c/6a0f1c2e-3b4d-4e5f-8a9b-0c1d2e3f4a5b",
                "status": "completed",
                "title": "async-std 的維護狀態",
                "report_path": report_path,
                "sources_path": sources_path,
                "content_len": 108,
                "source_count": 4
            })
        ),
        "stderr: {}",
        stderr(&output)
    );
    let sources: Value =
        serde_json::from_slice(&fs::read(&sources_path).expect("read sources")).expect("JSON");
    assert_eq!(
        (
            fs::read_to_string(&report_path).expect("read report"),
            sources
        ),
        (REPORT_MARKDOWN.to_owned(), report_sources())
    );
}

#[test]
fn the_current_widget_state_decides_over_the_snapshot_taken_at_the_tool_call() {
    let mut message = chatgpt::research_message(4, &chatgpt::completed_state());
    message["metadata"]["chatgpt_sdk"]["tool_response_metadata"] = json!({
        "openai/asyncStatus": 7,
        "venus_message_type": "initial_loading_message",
        "venus_widget_state": {"status": "waiting_for_user_response_on_plan"}
    });

    let output = read_messages(&[chatgpt::user_message(1, "研究 async-std。"), message]);

    remove_report_directory(&output);
    assert_eq!(
        (output.status.code(), payload(&output)["status"].clone()),
        (Some(0), json!("completed")),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_completed_report_goes_to_a_new_temporary_directory_by_default() {
    let output = read_messages(&chatgpt::research_turn(1, &chatgpt::completed_state()));

    let payload = payload(&output);
    let report_path = Path::new(payload["report_path"].as_str().expect("report path"));
    let contents = fs::read_to_string(report_path).expect("read report");
    let directory = report_path.parent().expect("report directory");
    let in_temp = directory.starts_with(std::env::temp_dir().join("forager-chatgpt"));
    fs::remove_dir_all(directory).expect("remove report directory");
    assert_eq!(
        (output.status.code(), contents.as_str(), in_temp),
        (Some(0), REPORT_MARKDOWN, true),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn content_prints_a_completed_report_and_writes_no_file() {
    let fake = chatgpt::reporting(&chatgpt::research_turn(1, &chatgpt::completed_state()));
    let environment = RunEnvironment::new(&chatgpt::config(&fake));
    let directory = tempfile::tempdir().expect("create report directory");
    let reports = report_dir(directory.path());

    let output = result(
        &environment,
        CONVERSATION,
        &["--format", "content", "--report-dir", &reports],
    );

    assert_eq!(
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            Path::new(&reports).exists()
        ),
        (Some(0), format!("{REPORT_MARKDOWN}\n"), false),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn content_renders_an_unfinished_research_as_json() {
    let fake = chatgpt::reporting(&chatgpt::research_turn(1, &chatgpt::running_state()));
    let environment = RunEnvironment::new(&chatgpt::config(&fake));

    let output = result(&environment, CONVERSATION, &["--format", "content"]);

    assert_eq!(
        (output.status.code(), payload(&output)["status"].clone()),
        (Some(0), json!("running"))
    );
}

#[test]
fn a_receipt_replaces_the_payload_that_output_receives() {
    let fake = chatgpt::reporting(&chatgpt::research_turn(1, &chatgpt::awaiting_state()));
    let environment = RunEnvironment::new(&chatgpt::config(&fake));
    let directory = tempfile::tempdir().expect("create output directory");
    let output_path = directory.path().join("result.json");
    let output_arg = output_path.display().to_string();

    let output = result(
        &environment,
        CONVERSATION,
        &["--output", &output_arg, "--receipt"],
    );

    let written: Value =
        serde_json::from_slice(&fs::read(&output_path).expect("read output")).expect("JSON");
    let receipt = payload(&output);
    assert_eq!(
        (
            output.status.code(),
            written["status"].clone(),
            receipt["output_path"].clone()
        ),
        (Some(0), json!("awaiting_confirmation"), json!(output_arg)),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn an_unwritable_report_fails_with_runtime_and_no_inline_report() {
    let fake = chatgpt::reporting(&chatgpt::research_turn(1, &chatgpt::completed_state()));
    let environment = RunEnvironment::new(&chatgpt::config(&fake));
    let directory = tempfile::tempdir().expect("create directory");
    let blocker = directory.path().join("file");
    fs::write(&blocker, "").expect("write blocking file");
    let blocker = blocker.display().to_string();

    let output = result(&environment, CONVERSATION, &["--report-dir", &blocker]);

    let (code, kind, message, url) = failure(&output);
    assert_eq!(
        (code, kind, url),
        (Some(4), json!("runtime"), json!(CONVERSATION_URL)),
        "stderr: {}",
        stderr(&output)
    );
    assert!(
        message.starts_with("cannot write the ChatGPT report to "),
        "{message}"
    );
}

#[test]
fn an_unwritable_output_exits_3_after_the_report_is_read() {
    let fake = chatgpt::reporting(&chatgpt::research_turn(1, &chatgpt::awaiting_state()));
    let environment = RunEnvironment::new(&chatgpt::config(&fake));
    let directory = tempfile::tempdir().expect("create directory");
    let output_arg = directory
        .path()
        .join("missing/result.json")
        .display()
        .to_string();

    let output = result(&environment, CONVERSATION, &["--output", &output_arg]);

    let payload = payload(&output);
    assert_eq!(
        (
            output.status.code(),
            payload["status"].clone(),
            payload["output_status"].clone()
        ),
        (Some(3), json!("awaiting_confirmation"), json!("failed"))
    );
}

#[test]
fn the_newest_research_decides_and_later_chat_does_not_hide_it() {
    let mut completed_under_chat = chatgpt::research_turn(1, &chatgpt::completed_state());
    completed_under_chat.push(chatgpt::user_message(7, "smol 和 tokio 哪個更輕？"));
    completed_under_chat.push(chatgpt::assistant_message(8, "smol 更輕。"));
    let mut running_over_completed = chatgpt::research_turn(1, &chatgpt::completed_state());
    running_over_completed.extend(chatgpt::research_turn(7, &chatgpt::running_state()));

    let completed = read_messages(&completed_under_chat);
    let running = read_messages(&running_over_completed);

    remove_report_directory(&completed);
    assert_eq!(
        (
            payload(&completed)["status"].clone(),
            payload(&running)["status"].clone()
        ),
        (json!("completed"), json!("running"))
    );
}

#[test]
fn a_conversation_without_research_is_a_parameter_failure() {
    let output = read_messages(&[
        chatgpt::user_message(1, "smol 和 tokio 哪個更輕？"),
        chatgpt::assistant_message(2, "smol 更輕。"),
    ]);

    assert_eq!(
        failure(&output),
        (
            Some(4),
            json!("parameter"),
            "the ChatGPT conversation https://chatgpt.com/c/6a0f1c2e-3b4d-4e5f-8a9b-0c1d2e3f4a5b is not a Deep Research conversation: none of the 2 messages the page loaded holds a Deep Research".to_owned(),
            json!(CONVERSATION_URL)
        )
    );
}

#[test]
fn an_unavailable_conversation_is_a_parameter_failure() {
    let mut noticed = chatgpt::unread(CONVERSATION_URL);
    noticed["page"]["notice"] = json!("Unable to load conversation");
    let (_, noticed) = read(noticed);
    let (_, sent_home) = read(chatgpt::unread("https://chatgpt.com/"));
    let (_, refused) = read(chatgpt::report(
        &json!({"detail": "conversation_not_found"}).to_string(),
    ));

    let kinds = [&noticed, &sent_home, &refused].map(|output| {
        let (code, kind, message, _) = failure(output);
        (
            code,
            kind == json!("parameter"),
            message.contains("is unavailable"),
        )
    });
    assert_eq!(kinds, [(Some(4), true, true); 3]);
}

#[test]
fn a_signed_out_browser_is_an_auth_failure_before_any_notice() {
    let mut signed_out = chatgpt::unread("https://chatgpt.com/");
    signed_out["page"]["signed_out"] = json!(true);
    signed_out["page"]["notice"] = json!("Unable to load conversation");

    let (_, output) = read(signed_out);

    let (code, kind, message, url) = failure(&output);
    assert_eq!(
        (code, kind, message.contains("sign in"), url),
        (Some(4), json!("auth"), true, json!(CONVERSATION_URL))
    );
}

#[test]
fn a_state_that_contradicts_the_report_reports_where_the_structure_changed() {
    let mut researching_with_report = chatgpt::completed_state();
    researching_with_report["status"] = json!("researching");
    let mut completed_without_report = chatgpt::running_state();
    completed_without_report["status"] = json!("completed");
    let mut unknown_status = chatgpt::running_state();
    unknown_status["status"] = json!("paused");
    let mut without_plan = chatgpt::running_state();
    without_plan.as_object_mut().expect("state").remove("plan");

    let failures = [
        researching_with_report,
        completed_without_report,
        unknown_status,
        without_plan,
    ]
    .map(|state| {
        let (code, kind, message, _) = failure(&read_messages(&chatgpt::research_turn(1, &state)));
        (code, kind, message)
    });

    let location = "the ChatGPT response structure has changed at conversation.messages[3].metadata.chatgpt_sdk.widget_state";
    assert_eq!(
        failures,
        [
            (
                Some(4),
                json!("runtime"),
                format!("{location}.status: status `researching` with a report message")
            ),
            (
                Some(4),
                json!("runtime"),
                format!("{location}.status: status `completed` without a report body")
            ),
            (
                Some(4),
                json!("runtime"),
                format!("{location}.status: unknown status `paused`")
            ),
            (Some(4), json!("runtime"), format!("{location}: no `plan`")),
        ]
    );
}

#[test]
fn a_citation_marker_left_after_rewriting_is_a_structure_change() {
    let (body, references) = chatgpt::report_body_and_references();
    let body = format!("{body}未標註的引用{}\n", chatgpt::marker(&["turn9search9"]));
    let state = chatgpt::widget_state(
        "completed",
        &["completed", "completed", "completed", "completed"],
        Some(chatgpt::report_message(&body, &references)),
    );
    let fake = chatgpt::reporting(&chatgpt::research_turn(1, &state));
    let environment = RunEnvironment::new(&chatgpt::config(&fake));
    let directory = tempfile::tempdir().expect("create report directory");
    let reports = report_dir(directory.path());

    let output = result(&environment, CONVERSATION, &["--report-dir", &reports]);

    let (code, kind, message, _) = failure(&output);
    assert_eq!(
        (code, kind, message, Path::new(&reports).exists()),
        (
            Some(4),
            json!("runtime"),
            "the ChatGPT response structure has changed at conversation.messages[3].metadata.chatgpt_sdk.widget_state.report_message.content.parts[0]: an unresolved citation marker at character 114 after rewriting".to_owned(),
            false
        )
    );
}

#[test]
fn a_usage_receipt_over_the_limit_is_quota_exhausted() {
    let refused = chatgpt::research_message_with(
        4,
        &chatgpt::awaiting_state(),
        &json!({"allowed": false, "outcome": "rate_limit_reached", "rate_limit": {"limit_reached": true}}),
    );

    let output = read_messages(&[chatgpt::user_message(1, "研究 async-std。"), refused]);

    let (code, kind, _, url) = failure(&output);
    assert_eq!(
        (code, kind, url),
        (Some(4), json!("quota_exhausted"), json!(CONVERSATION_URL))
    );
}

#[test]
fn another_conversation_on_the_page_or_in_the_response_is_runtime_and_writes_nothing() {
    let other = "7b1f2d3e-4c5d-4e6f-9a0b-1c2d3e4f5a6b";
    let other_url = format!("https://chatgpt.com/c/{other}");
    let messages = chatgpt::research_turn(1, &chatgpt::completed_state());
    let mut on_page = chatgpt::report(&chatgpt::conversation_body(&messages));
    on_page["page"]["url"] = json!(other_url);
    let in_response = chatgpt::report(&chatgpt::conversation_body_of(other, &messages));

    let outcomes = [on_page, in_response].map(|data| {
        let fake = chatgpt::answering(data);
        let environment = RunEnvironment::new(&chatgpt::config(&fake));
        let directory = tempfile::tempdir().expect("create report directory");
        let reports = report_dir(directory.path());
        let output = result(&environment, CONVERSATION, &["--report-dir", &reports]);
        let (code, kind, message, _) = failure(&output);
        (code, kind, message, Path::new(&reports).exists())
    });

    assert_eq!(
        outcomes,
        [
            (
                Some(4),
                json!("runtime"),
                format!(
                    "the ChatGPT page shows `{other_url}` instead of the requested conversation {CONVERSATION_URL}"
                ),
                false
            ),
            (
                Some(4),
                json!("runtime"),
                format!(
                    "the ChatGPT response is for the conversation `{other}` instead of the requested conversation {CONVERSATION_URL}"
                ),
                false
            ),
        ]
    );
}

#[test]
fn every_conversation_form_is_accepted_and_normalized() {
    let fake = chatgpt::reporting(&chatgpt::research_turn(1, &chatgpt::awaiting_state()));
    let environment = RunEnvironment::new(&chatgpt::config(&fake));

    let ids = [
        "https://chatgpt.com/c/6A0F1C2E-3B4D-4E5F-8A9B-0C1D2E3F4A5B",
        "https://chatgpt.com/g/g-p-0123456789abcdef-rust/c/6a0f1c2e-3b4d-4e5f-8a9b-0c1d2e3f4a5b",
        "6A0F1C2E-3b4d-4e5f-8a9b-0c1d2e3f4a5b",
    ]
    .map(|conversation| {
        payload(&result(&environment, conversation, &[]))["conversation_id"].clone()
    });

    let requested = fake
        .calls()
        .iter()
        .map(|call| call[3].clone())
        .collect::<Vec<_>>();
    assert_eq!(
        (ids, requested),
        (
            [
                json!(CONVERSATION),
                json!(CONVERSATION),
                json!(CONVERSATION)
            ],
            vec![CONVERSATION.to_owned(); 3]
        )
    );
}

#[test]
fn an_unrecognized_conversation_exits_2_without_running_opencli() {
    let fake = chatgpt::reporting(&chatgpt::research_turn(1, &chatgpt::awaiting_state()));
    let environment = RunEnvironment::new(&chatgpt::config(&fake));

    let outputs = [
        "https://example.com/c/6a0f1c2e-3b4d-4e5f-8a9b-0c1d2e3f4a5b",
        "https://chatgpt.com/share/6a0f1c2e-3b4d-4e5f-8a9b-0c1d2e3f4a5b",
        "6a0f1c2e3b4d4e5f8a9b0c1d2e3f4a5b",
        "6a0f1c2e-3b4d-4e5f-8a9b-0c1d2e3f4a5z",
    ]
    .map(|conversation| result(&environment, conversation, &[]).status.code());

    assert_eq!((outputs, fake.calls().len()), ([Some(2); 4], 0));
}

#[test]
fn an_outdated_adapter_carries_the_install_hint() {
    let fake = FakeOpenCli::contract_by_command(
        "forager-chatgpt/0",
        &[("report", chatgpt::report(&chatgpt::conversation_body(&[])))],
    );
    let environment = RunEnvironment::new(&chatgpt::config(&fake));

    let output = result(&environment, CONVERSATION, &[]);

    let (code, kind, message, url) = failure(&output);
    assert_eq!(
        (code, kind, url),
        (Some(4), json!("runtime"), json!(CONVERSATION_URL))
    );
    assert!(
        message.contains("copy the `opencli/forager-chatgpt` directory"),
        "{message}"
    );
}

#[test]
fn a_missing_adapter_carries_the_install_hint() {
    let fake = FakeOpenCli::answering("", "error: unknown command 'forager-chatgpt'\n", 2);
    let environment = RunEnvironment::new(&chatgpt::config(&fake));

    let output = result(&environment, CONVERSATION, &[]);

    let (code, kind, message, _) = failure(&output);
    assert_eq!(
        (
            code,
            kind,
            message.contains("copy the `opencli/forager-chatgpt` directory")
        ),
        (Some(4), json!("runtime"), true),
        "{message}"
    );
}
