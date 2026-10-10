//! `forager gemini research start` against a fake `opencli` executable that answers `start`
//! envelopes shaped like the live Gemini page facts of 2026-10-09. These tests prove the
//! transport, the single attempt, the page-fact classification, and the decoding of the two
//! `StreamGenerate` responses; only the live acceptance run exercises the JavaScript adapter.
#![cfg(unix)]

mod support;

use std::process::Output;

use serde_json::{Value, json};

use support::RunEnvironment;
use support::gemini::{self, CONVERSATION_URL, failure, payload};
use support::opencli::FakeOpenCli;

const QUERY: &str = "Compare Rust crates that convert HTML to Markdown.";

fn start(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["gemini", "research", "start", QUERY];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

/// Starts a research against a fake that answers `start` with `data`.
fn run(data: Value) -> (FakeOpenCli, Output) {
    let fake = gemini::starting(data);
    let environment = RunEnvironment::new(&gemini::config(&fake));
    let output = start(&environment, &[]);
    (fake, output)
}

const FOREGROUND_SESSION: [&str; 8] = [
    "-f",
    "json",
    "--window",
    "foreground",
    "--site-session",
    "ephemeral",
    "--keep-tab",
    "false",
];

#[test]
fn a_started_research_returns_the_conversation_and_the_plan() {
    let (fake, output) = run(gemini::started_facts());

    assert_eq!(
        (output.status.code(), payload(&output)),
        (
            Some(0),
            json!({
                "route": "gemini_browser",
                "conversation_id": "a1b2c3d4e5f60718",
                "conversation_url": "https://gemini.google.com/app/a1b2c3d4e5f60718",
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
        String::from_utf8_lossy(&output.stderr)
    );
    let calls = fake.calls();
    let call = &calls[0];
    assert_eq!(
        (
            calls.len(),
            call[..4].to_vec(),
            call[call.len() - FOREGROUND_SESSION.len()..].to_vec()
        ),
        (
            1,
            ["forager-gemini", "start", "--query", QUERY]
                .map(String::from)
                .to_vec(),
            FOREGROUND_SESSION.map(String::from).to_vec()
        )
    );
}

#[test]
fn an_unconfirmed_plan_is_runtime_and_points_to_the_conversation() {
    let facts = gemini::start_facts(
        &gemini::START_STEPS[..5],
        Some(gemini::stream_body(&gemini::plan_candidate())),
        None,
    );

    let (fake, output) = run(facts);

    let (code, kind, message, url) = failure(&output);
    assert_eq!(
        (code, kind, url, fake.calls().len()),
        (Some(4), json!("runtime"), json!(CONVERSATION_URL), 1)
    );
    assert_eq!(
        message,
        format!(
            "Gemini proposed a research plan, but forager could not click its confirm button: open {CONVERSATION_URL} and click \"Start research\" there; forager does not click again or resend the question"
        )
    );
}

#[test]
fn an_unconfirmed_plan_carries_why_the_adapter_stopped() {
    let mut facts = gemini::start_facts(
        &gemini::START_STEPS[..5],
        Some(gemini::stream_body(&gemini::plan_candidate())),
        None,
    );
    facts["problem"] =
        json!("Gemini showed no \"Start research\" button (1 matching button hidden)");

    let (_fake, output) = run(facts);

    let (code, kind, message, _url) = failure(&output);
    assert_eq!((code, kind), (Some(4), json!("runtime")));
    assert_eq!(
        message,
        format!(
            "Gemini proposed a research plan, but forager could not click its confirm button (Gemini showed no \"Start research\" button (1 matching button hidden)): open {CONVERSATION_URL} and click \"Start research\" there; forager does not click again or resend the question"
        )
    );
}

#[test]
fn an_unconfirmed_plan_after_the_deadline_is_a_timeout() {
    let mut facts = gemini::start_facts(
        &gemini::START_STEPS[..5],
        Some(gemini::stream_body(&gemini::plan_candidate())),
        None,
    );
    facts["timed_out"] = json!(true);

    let (_fake, output) = run(facts);

    let (code, kind, message, url) = failure(&output);
    assert_eq!(
        (code, kind, url),
        (Some(4), json!("timeout"), json!(CONVERSATION_URL))
    );
    assert_eq!(
        message,
        format!(
            "Gemini proposed a research plan, but the read deadline passed before forager clicked its confirm button: open {CONVERSATION_URL} and click \"Start research\" there; forager does not click again or resend the question"
        )
    );
}

#[test]
fn a_text_reply_instead_of_a_plan_is_runtime_with_the_start_of_the_reply() {
    let reply = "I can't help with that request.\n\nTry asking something else.";
    let facts = gemini::start_facts(
        &gemini::START_STEPS[..5],
        Some(gemini::stream_body(&gemini::text_candidate(reply))),
        None,
    );

    let (_, output) = run(facts);

    assert_eq!(
        failure(&output),
        (
            Some(4),
            json!("runtime"),
            format!(
                "Gemini replied with text instead of a research plan, so no research started: `I can't help with that request. Try asking something else.`; see {CONVERSATION_URL}"
            ),
            json!(CONVERSATION_URL)
        )
    );
}

#[test]
fn an_exhausted_quota_is_quota_exhausted() {
    let error_code = gemini::start_facts(
        &gemini::START_STEPS[..5],
        Some(gemini::stream_error_body(1037)),
        None,
    );
    let mut notice = gemini::start_facts(&gemini::START_STEPS[..4], None, None);
    notice["page"]["url"] = json!("https://gemini.google.com/app");
    notice["quota_notice"] = json!("You've reached your Deep Research limit");

    let kinds = [error_code, notice].map(|facts| {
        let (code, kind, _, url) = failure(&run(facts).1);
        (code, kind, url)
    });

    assert_eq!(
        kinds,
        [
            (Some(4), json!("quota_exhausted"), json!(CONVERSATION_URL)),
            (Some(4), json!("quota_exhausted"), Value::Null)
        ]
    );
}

#[test]
fn page_text_about_limits_does_not_count_as_a_quota_notice_once_gemini_proposed_a_plan() {
    let mut facts = gemini::start_facts(
        &gemini::START_STEPS[..5],
        Some(gemini::stream_body(&gemini::plan_candidate())),
        None,
    );
    facts["quota_notice"] = json!("What usage limits apply to the Gemini API?");

    let (_, output) = run(facts);

    let (code, kind, message, url) = failure(&output);
    assert_eq!(
        (code, kind, url),
        (Some(4), json!("runtime"), json!(CONVERSATION_URL)),
        "{message}"
    );
    assert!(
        message.starts_with("Gemini proposed a research plan"),
        "{message}"
    );
}

#[test]
fn a_signed_out_browser_is_an_auth_failure() {
    let fake = FakeOpenCli::answering(
        "",
        "ok: false\nerror:\n  code: AUTH_REQUIRED\n  message: sign in to Gemini\n",
        77,
    );
    let environment = RunEnvironment::new(&gemini::config(&fake));

    let output = start(&environment, &[]);

    let (code, kind, _, url) = failure(&output);
    assert_eq!(
        (code, kind, url, fake.calls().len()),
        (Some(4), json!("auth"), Value::Null, 1)
    );
}

/// Points `gemini_browser` at `fake` with a 7-second attempt and allows three attempts, so a
/// retry would fit in the 30-second command deadline the tests pass.
fn retrying_config(fake: &FakeOpenCli) -> String {
    format!(
        "{}timeout = 7\n\n[retry]\nmax_attempts = 3\nmax_wait = 0\n",
        gemini::config(fake)
    )
}

const UNKNOWN_OUTCOME: &str = "forager does not know whether Gemini received the question; look for it in the Gemini history at https://gemini.google.com/app before starting again";

#[test]
fn a_timeout_or_network_failure_runs_one_attempt_and_leaves_the_outcome_unknown() {
    let failures = [
        (75, "TIMEOUT", "timed out"),
        (69, "BROWSER_CONNECT", "daemon unavailable"),
    ]
    .map(|(code, name, message)| {
        let fake = FakeOpenCli::answering(
            "",
            &format!("ok: false\nerror:\n  code: {name}\n  message: {message}\n"),
            code,
        );
        let environment = RunEnvironment::new(&retrying_config(&fake));
        let output = start(&environment, &["--timeout", "30"]);
        let (code, kind, message, url) = failure(&output);
        (
            code,
            kind,
            message.ends_with(UNKNOWN_OUTCOME),
            url,
            fake.calls().len(),
        )
    });

    assert_eq!(
        failures,
        [
            (Some(4), json!("timeout"), true, Value::Null, 1),
            (Some(4), json!("network"), true, Value::Null, 1)
        ]
    );
}

#[test]
fn a_hanging_start_is_killed_once_and_leaves_the_outcome_unknown() {
    let fake = FakeOpenCli::hanging();
    let environment = RunEnvironment::new(&retrying_config(&fake));

    let output = start(&environment, &["--timeout", "30"]);

    let (code, kind, message, url) = failure(&output);
    assert_eq!(
        (
            code,
            kind,
            message.ends_with(UNKNOWN_OUTCOME),
            url,
            fake.calls().len()
        ),
        (Some(4), json!("timeout"), true, Value::Null, 1),
        "{message}"
    );
}

#[test]
fn partial_facts_at_the_read_deadline_point_to_the_conversation() {
    let mut facts = gemini::start_facts(
        &gemini::START_STEPS[..6],
        Some(gemini::stream_body(&gemini::plan_candidate())),
        None,
    );
    facts["timed_out"] = json!(true);

    let (fake, output) = run(facts);

    assert_eq!(
        (failure(&output), fake.calls().len()),
        (
            (
                Some(4),
                json!("timeout"),
                format!(
                    "forager clicked \"Start research\", but the read deadline passed before Gemini answered the confirmation; the research may be running: read it with `forager gemini research result {CONVERSATION_URL}`"
                ),
                json!(CONVERSATION_URL)
            ),
            1
        )
    );
}

#[test]
fn a_confirmation_that_did_not_start_the_research_reports_where_the_structure_changed() {
    let facts = gemini::start_facts(
        &gemini::START_STEPS,
        Some(gemini::stream_body(&gemini::plan_candidate())),
        Some(gemini::stream_body(&gemini::plan_candidate())),
    );

    let (_, output) = run(facts);

    assert_eq!(
        failure(&output),
        (
            Some(4),
            json!("runtime"),
            format!(
                "the Gemini response structure has changed at StreamGenerate[4][0][12] field 69: status 2 after confirming the plan; see {CONVERSATION_URL}"
            ),
            json!(CONVERSATION_URL)
        )
    );
}

#[test]
fn another_page_after_starting_is_runtime() {
    let mut facts = gemini::started_facts();
    facts["page"]["url"] = json!("https://gemini.google.com/app");

    let (_, output) = run(facts);

    assert_eq!(
        failure(&output),
        (
            Some(4),
            json!("runtime"),
            format!(
                "the Gemini page shows `https://gemini.google.com/app` instead of the started conversation {CONVERSATION_URL}"
            ),
            json!(CONVERSATION_URL)
        )
    );
}

#[test]
fn markdown_shows_the_conversation_and_the_plan() {
    let fake = gemini::starting(gemini::started_facts());
    let environment = RunEnvironment::new(&gemini::config(&fake));

    let output = start(&environment, &["--format", "markdown"]);

    assert_eq!(
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned()
        ),
        (
            Some(0),
            format!(
                "# Gemini Deep Research started\n\nConversation: <{CONVERSATION_URL}>\n\n## Plan: Rust HTML to Markdown Crates\n\n1. **Research Websites** — Find maintained crates such as htmd and html2md.\n2. **Analyze Results** — Compare table, code block, and link fidelity.\n3. **Create Report** — Recommend one crate for a CLI tool.\n\nReady in a few mins\n\nRead where it stands with `forager gemini research result {CONVERSATION_URL}`.\n"
            )
        )
    );
}

#[test]
fn an_empty_question_exits_2_without_running_opencli() {
    let fake = gemini::starting(gemini::started_facts());
    let environment = RunEnvironment::new(&gemini::config(&fake));

    let output = environment.run(&["gemini", "research", "start", "  "]);

    assert_eq!((output.status.code(), fake.calls().len()), (Some(2), 0));
}

#[test]
fn an_adapter_that_stops_before_sending_says_why_and_that_nothing_was_sent() {
    let mut facts = gemini::start_facts(&gemini::START_STEPS[..1], None, None);
    facts["page"]["url"] = json!("https://gemini.google.com/app");
    facts["problem"] = json!("Deep Research did not show as selected");

    let (_, output) = run(facts);

    assert_eq!(
        failure(&output),
        (
            Some(4),
            json!("runtime"),
            "the forager-gemini adapter stopped before the question was sent: Deep Research did not show as selected (last step: tools_menu); nothing was sent".to_owned(),
            Value::Null
        )
    );
}

#[test]
fn a_missing_deep_research_tool_is_runtime_and_sends_nothing() {
    let mut facts = gemini::start_facts(&gemini::START_STEPS[..1], None, None);
    facts["page"]["url"] = json!("https://gemini.google.com/app");
    facts["deep_research_missing"] = json!(true);

    let (_, output) = run(facts);

    assert_eq!(
        failure(&output),
        (
            Some(4),
            json!("runtime"),
            "Gemini's tools menu offers no Deep Research; check that this Google account can use Deep Research in the Gemini web app; nothing was sent".to_owned(),
            Value::Null
        )
    );
}

#[test]
fn a_disabled_deep_research_tool_names_a_used_up_quota_and_sends_nothing() {
    let mut facts = gemini::start_facts(&gemini::START_STEPS[..1], None, None);
    facts["page"]["url"] = json!("https://gemini.google.com/app");
    facts["deep_research_disabled"] = json!(true);

    let (_, output) = run(facts);

    assert_eq!(
        failure(&output),
        (
            Some(4),
            json!("runtime"),
            "Gemini's tools menu shows Deep Research but does not let it be selected; the account's Deep Research quota may be used up, or Deep Research may be unavailable right now; nothing was sent".to_owned(),
            Value::Null
        )
    );
}

#[test]
fn a_start_that_opencli_never_ran_says_nothing_was_sent() {
    let missing_adapter = FakeOpenCli::answering(
        "",
        "ok: false\nerror:\n  code: ADAPTER_LOAD\n  message: cannot load forager-gemini/start\n",
        69,
    );
    let configs = [
        gemini::config(&missing_adapter),
        "[providers.gemini_browser]\ncommand = \"/nonexistent/forager-test/opencli\"\n".to_owned(),
    ];

    let failures = configs.map(|config| {
        let (_, kind, message, url) = failure(&start(&RunEnvironment::new(&config), &[]));
        (kind, message.ends_with("; nothing was sent"), url)
    });

    assert_eq!(
        failures,
        [
            (json!("runtime"), true, Value::Null),
            (json!("runtime"), true, Value::Null)
        ]
    );
}
