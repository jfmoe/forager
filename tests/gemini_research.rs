//! `forager gemini research result` against a fake `opencli` executable that answers `report`
//! envelopes shaped like the live Gemini page facts of 2026-10-09. These tests prove the
//! transport, the page-fact classification, the response decoding, the turn selection, and the
//! report delivery; only the live acceptance run exercises the JavaScript adapter.
#![cfg(unix)]

mod support;

use std::fs;
use std::path::Path;
use std::process::Output;

use serde_json::{Value, json};

use support::RunEnvironment;
use support::gemini::{self, CONVERSATION, CONVERSATION_URL, failure, payload};
use support::opencli::FakeOpenCli;

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn result(environment: &RunEnvironment, conversation: &str, arguments: &[&str]) -> Output {
    let mut command = vec!["gemini", "research", "result", conversation];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

/// Reads `conversation` from a fake that answers `report` with `data`.
fn read(data: Value) -> (FakeOpenCli, Output) {
    let fake = gemini::answering(data);
    let environment = RunEnvironment::new(&gemini::config(&fake));
    let output = result(&environment, CONVERSATION, &[]);
    (fake, output)
}

/// Reads `conversation` from a fake that answers `report` with `turns`.
fn read_turns(turns: &[Value]) -> Output {
    read(gemini::report(&gemini::hnvqhb_body(turns))).1
}

fn report_dir(directory: &Path) -> String {
    directory.join("reports").display().to_string()
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

const REPORT_MARKDOWN: &str = "# Rust HTML to Markdown Crates\n\n| Crate | Tables |\n|---|---|\n| htmd | yes |\n\nhtmd keeps tables [cite: 2, 1]. html2md is older [cite: 3].\n\n## Sources\n\n- [1] htmd on crates.io <https://crates.io/crates/htmd>\n- [2] htmd on GitHub <https://github.com/letmutex/htmd>\n- [3] html2md <https://crates.io/crates/html2md>\n";

fn report_sources() -> Value {
    json!([
        {"id": 1, "title": "htmd on crates.io", "url": "https://crates.io/crates/htmd"},
        {"id": 2, "title": "htmd on GitHub", "url": "https://github.com/letmutex/htmd"},
        {"id": 3, "title": "html2md", "url": "https://crates.io/crates/html2md"}
    ])
}

#[test]
fn a_plan_awaiting_confirmation_returns_the_plan() {
    let fake = gemini::reporting(&[gemini::plan_turn(1)]);
    let environment = RunEnvironment::new(&gemini::config(&fake));

    let output = result(&environment, CONVERSATION_URL, &[]);

    assert_eq!(
        (output.status.code(), payload(&output)),
        (
            Some(0),
            json!({
                "route": "gemini_browser",
                "conversation_id": "a1b2c3d4e5f60718",
                "conversation_url": "https://gemini.google.com/app/a1b2c3d4e5f60718",
                "status": "awaiting_confirmation",
                "plan": {
                    "title": "Rust HTML to Markdown Crates",
                    "steps": [
                        {"index": 1, "label": "Research Websites", "description": "Find maintained crates such as htmd and html2md."},
                        {"index": 2, "label": "Analyze Results", "description": "Compare table, code block, and link fidelity."},
                        {"index": 3, "label": "Create Report", "description": "Recommend one crate for a CLI tool."}
                    ],
                    "eta_text": "Ready in a few mins"
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
            ["forager-gemini", "report", "--conversation", CONVERSATION]
                .map(String::from)
                .to_vec(),
            BACKGROUND_SESSION.map(String::from).to_vec()
        )
    );
}

#[test]
fn a_running_research_reports_its_progress() {
    let output = read_turns(&[gemini::running_turn(
        2,
        &[
            gemini::visited("https://crates.io/crates/htmd"),
            gemini::thought("Comparing table support"),
            gemini::visited("https://crates.io/crates/html2md"),
            gemini::thought("Checking link fidelity"),
            gemini::visited("https://github.com/letmutex/htmd"),
        ],
    )]);

    assert_eq!(
        (output.status.code(), payload(&output)),
        (
            Some(0),
            json!({
                "route": "gemini_browser",
                "conversation_id": "a1b2c3d4e5f60718",
                "conversation_url": "https://gemini.google.com/app/a1b2c3d4e5f60718",
                "status": "running",
                "progress": {
                    "sources_visited": 3,
                    "thoughts": 2,
                    "latest_thought": "Checking link fidelity"
                }
            })
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_completed_report_is_written_with_its_sources() {
    let fake = gemini::reporting(&[gemini::completed_turn(2, gemini::report_citations())]);
    let environment = RunEnvironment::new(&gemini::config(&fake));
    let directory = tempfile::tempdir().expect("create report directory");
    let reports = report_dir(directory.path());

    let output = result(&environment, CONVERSATION, &["--report-dir", &reports]);

    let report_path = format!("{reports}/gemini-{CONVERSATION}.md");
    let sources_path = format!("{reports}/gemini-{CONVERSATION}.sources.json");
    assert_eq!(
        (output.status.code(), payload(&output)),
        (
            Some(0),
            json!({
                "route": "gemini_browser",
                "conversation_id": "a1b2c3d4e5f60718",
                "conversation_url": "https://gemini.google.com/app/a1b2c3d4e5f60718",
                "status": "completed",
                "title": "Rust HTML to Markdown Crates",
                "report_path": report_path,
                "sources_path": sources_path,
                "content_len": gemini::REPORT_BODY.chars().count(),
                "source_count": 3
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
fn a_completed_report_goes_to_a_new_temporary_directory_by_default() {
    let output = read_turns(&[gemini::completed_turn(2, gemini::report_citations())]);

    let payload = payload(&output);
    let report_path = Path::new(payload["report_path"].as_str().expect("report path"));
    let contents = fs::read_to_string(report_path).expect("read report");
    let directory = report_path.parent().expect("report directory");
    let in_temp = directory.starts_with(std::env::temp_dir().join("forager-gemini"));
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
    let fake = gemini::reporting(&[gemini::completed_turn(2, gemini::report_citations())]);
    let environment = RunEnvironment::new(&gemini::config(&fake));
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
    let fake = gemini::reporting(&[gemini::running_turn(2, &[])]);
    let environment = RunEnvironment::new(&gemini::config(&fake));

    let output = result(&environment, CONVERSATION, &["--format", "content"]);

    assert_eq!(
        (output.status.code(), payload(&output)["status"].clone()),
        (Some(0), json!("running"))
    );
}

#[test]
fn a_receipt_replaces_the_payload_that_output_receives() {
    let fake = gemini::reporting(&[gemini::plan_turn(1)]);
    let environment = RunEnvironment::new(&gemini::config(&fake));
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
    let fake = gemini::reporting(&[gemini::completed_turn(2, gemini::report_citations())]);
    let environment = RunEnvironment::new(&gemini::config(&fake));
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
        message.starts_with("cannot write the Gemini report to "),
        "{message}"
    );
}

#[test]
fn an_unwritable_output_exits_3_after_the_report_is_read() {
    let fake = gemini::reporting(&[gemini::plan_turn(1)]);
    let environment = RunEnvironment::new(&gemini::config(&fake));
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
fn the_newest_research_turn_decides_and_later_chat_does_not_hide_it() {
    let completed_under_chat = read_turns(&[
        gemini::plain_turn(3),
        gemini::completed_turn(2, gemini::report_citations()),
        gemini::plan_turn(1),
    ]);
    let running_over_completed = read_turns(&[
        gemini::running_turn(4, &[]),
        gemini::completed_turn(2, gemini::report_citations()),
    ]);

    let completed = payload(&completed_under_chat);
    let running = payload(&running_over_completed);
    if let Some(path) = completed["report_path"].as_str()
        && let Some(directory) = Path::new(path).parent()
    {
        let _ = fs::remove_dir_all(directory);
    }
    assert_eq!(
        (completed["status"].clone(), running["status"].clone()),
        (json!("completed"), json!("running"))
    );
}

#[test]
fn a_conversation_without_research_is_a_parameter_failure() {
    let output = read_turns(&[gemini::plain_turn(2), gemini::plain_turn(1)]);

    assert_eq!(
        failure(&output),
        (
            Some(4),
            json!("parameter"),
            "the Gemini conversation https://gemini.google.com/app/a1b2c3d4e5f60718 is not a Deep Research conversation: none of the 2 turns the page loaded holds a research plan or report".to_owned(),
            json!(CONVERSATION_URL)
        )
    );
}

#[test]
fn an_unavailable_conversation_is_a_parameter_failure() {
    let mut noticed = gemini::unread(CONVERSATION_URL);
    noticed["page"]["notice"] = json!("Conversation not found");
    let (_, noticed) = read(noticed);
    let (_, sent_home) = read(gemini::unread("https://gemini.google.com/app"));

    let kinds = [&noticed, &sent_home].map(|output| {
        let (code, kind, message, _) = failure(output);
        (code, kind, message.contains("is unavailable"))
    });
    assert_eq!(
        kinds,
        [
            (Some(4), json!("parameter"), true),
            (Some(4), json!("parameter"), true)
        ]
    );
}

#[test]
fn a_signed_out_browser_is_an_auth_failure_before_any_notice() {
    let mut signed_out = gemini::unread("https://gemini.google.com/app");
    signed_out["page"]["signed_out"] = json!(true);
    signed_out["page"]["notice"] = json!("Conversation not found");

    let (_, output) = read(signed_out);

    let (code, kind, message, url) = failure(&output);
    assert_eq!(
        (code, kind, message.contains("sign in"), url),
        (Some(4), json!("auth"), true, json!(CONVERSATION_URL))
    );
}

#[test]
fn a_status_that_contradicts_the_body_reports_where_the_structure_changed() {
    let running_with_body = read_turns(&[gemini::research_turn(2, Some(3), "# Report\n")]);
    let completed_without_body = read_turns(&[gemini::research_turn(2, Some(5), "")]);
    let without_status = read_turns(&[gemini::research_turn(2, None, "")]);

    let failures = [&running_with_body, &completed_without_body, &without_status].map(failure);
    assert_eq!(
        failures.map(|(code, kind, message, _)| (code, kind, message)),
        [
            (
                Some(4),
                json!("runtime"),
                "the Gemini response structure has changed at hNvQHb[0][0][3][0][0][12] field 69: status 3 with a report body".to_owned()
            ),
            (
                Some(4),
                json!("runtime"),
                "the Gemini response structure has changed at hNvQHb[0][0][3][0][0][12] field 69: status 5 without a report body".to_owned()
            ),
            (
                Some(4),
                json!("runtime"),
                "the Gemini response structure has changed at hNvQHb[0][0][3][0][0][12]: no status field 69".to_owned()
            ),
        ]
    );
}

#[test]
fn another_conversation_on_the_page_is_runtime_and_writes_nothing() {
    let other = "https://gemini.google.com/app/ffffffffffffffff";
    let mut shown = gemini::report(&gemini::hnvqhb_body(&[gemini::completed_turn(
        2,
        gemini::report_citations(),
    )]));
    shown["page"]["url"] = json!(other);
    let fake = gemini::answering(shown);
    let environment = RunEnvironment::new(&gemini::config(&fake));
    let directory = tempfile::tempdir().expect("create report directory");
    let reports = report_dir(directory.path());

    let output = result(&environment, CONVERSATION, &["--report-dir", &reports]);

    let (code, kind, message, _) = failure(&output);
    assert_eq!(
        (code, kind, message, Path::new(&reports).exists()),
        (
            Some(4),
            json!("runtime"),
            format!(
                "the Gemini page shows `{other}` instead of the requested conversation {CONVERSATION_URL}"
            ),
            false
        )
    );
}

#[test]
fn an_unrecognized_conversation_exits_2_without_running_opencli() {
    let fake = gemini::reporting(&[gemini::plan_turn(1)]);
    let environment = RunEnvironment::new(&gemini::config(&fake));

    let outputs = [
        "https://example.com/app/a1b2c3d4e5f60718",
        "c_a1b2c3d4e5f60718",
    ]
    .map(|conversation| result(&environment, conversation, &[]).status.code());

    assert_eq!((outputs, fake.calls().len()), ([Some(2), Some(2)], 0));
}

#[test]
fn an_outdated_adapter_carries_the_install_hint() {
    let fake = FakeOpenCli::contract_by_command(
        "forager-gemini/0",
        &[("report", gemini::report(&gemini::hnvqhb_body(&[])))],
    );
    let environment = RunEnvironment::new(&gemini::config(&fake));

    let output = result(&environment, CONVERSATION, &[]);

    let (code, kind, message, url) = failure(&output);
    assert_eq!(
        (code, kind, url),
        (Some(4), json!("runtime"), json!(CONVERSATION_URL))
    );
    assert!(
        message.contains("copy the `opencli/forager-gemini` directory"),
        "{message}"
    );
}
