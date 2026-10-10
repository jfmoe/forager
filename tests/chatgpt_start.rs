//! `forager chatgpt research start` against a fake `opencli` executable that answers `start`
//! envelopes shaped like the live ChatGPT page facts of 2026-10-10. These tests prove the
//! transport, the single attempt, the page-fact classification, and the decoding of the
//! conversation response read after starting; only the live acceptance run exercises the
//! JavaScript adapter.
#![cfg(unix)]

mod support;

use std::process::Output;

use serde_json::{Value, json};

use support::RunEnvironment;
use support::chatgpt::{self, CONVERSATION_URL, START_STEPS, failure, payload};
use support::opencli::FakeOpenCli;

const QUERY: &str = "用不超過 500 字總結 async-std 的維護狀態。";

fn start(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["chatgpt", "research", "start", QUERY];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

/// Starts a research against a fake that answers `start` with `data`.
fn run(data: Value) -> (FakeOpenCli, Output) {
    let fake = chatgpt::starting(data);
    let environment = RunEnvironment::new(&chatgpt::config(&fake));
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
    let (fake, output) = run(chatgpt::started_facts());

    assert_eq!(
        (output.status.code(), payload(&output)),
        (
            Some(0),
            json!({
                "route": "chatgpt_browser",
                "conversation_id": "6a0f1c2e-3b4d-4e5f-8a9b-0c1d2e3f4a5b",
                "conversation_url": "https://chatgpt.com/c/6a0f1c2e-3b4d-4e5f-8a9b-0c1d2e3f4a5b",
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
            ["forager-chatgpt", "start", "--query", QUERY]
                .map(String::from)
                .to_vec(),
            FOREGROUND_SESSION.map(String::from).to_vec()
        )
    );
}

const NOT_STARTED_HINT: &str = "the plan's countdown may still start the research, so read it with `forager chatgpt research result https://chatgpt.com/c/6a0f1c2e-3b4d-4e5f-8a9b-0c1d2e3f4a5b` before starting again; forager does not resend the question";

#[test]
fn a_plan_still_waiting_after_its_countdown_is_runtime_and_says_to_read_the_result_first() {
    let facts = chatgpt::start_facts(
        &START_STEPS,
        Some(chatgpt::conversation_body(&chatgpt::research_turn(
            1,
            &chatgpt::awaiting_state(),
        ))),
    );

    let (fake, output) = run(facts);

    assert_eq!(
        (failure(&output), fake.calls().len()),
        (
            (
                Some(4),
                json!("runtime"),
                format!(
                    "ChatGPT proposed a research plan, but the research had not started when forager read the conversation again after the plan's countdown: {NOT_STARTED_HINT}"
                ),
                json!(CONVERSATION_URL)
            ),
            1
        )
    );
}

#[test]
fn a_text_reply_instead_of_a_plan_is_runtime_with_the_start_of_the_reply() {
    let reply = "你想聚焦哪個方面？\n\n維護狀態、替代方案，還是遷移成本？";
    let facts = chatgpt::start_facts(
        &[
            "chat_mode",
            "query",
            "deep_research",
            "mention",
            "sent",
            "conversation",
            "answered",
            "reopened",
        ],
        Some(chatgpt::conversation_body(&[
            chatgpt::user_message(1, QUERY),
            chatgpt::assistant_message(2, reply),
        ])),
    );

    let (_, output) = run(facts);

    assert_eq!(
        failure(&output),
        (
            Some(4),
            json!("runtime"),
            format!(
                "ChatGPT replied with text instead of a research plan, so no research started: `你想聚焦哪個方面？ 維護狀態、替代方案，還是遷移成本？`; answer it at {CONVERSATION_URL}, then read it with `forager chatgpt research result {CONVERSATION_URL}`"
            ),
            json!(CONVERSATION_URL)
        )
    );
}

#[test]
fn a_usage_receipt_over_the_limit_is_quota_exhausted() {
    let receipt = json!({
        "allowed": false,
        "outcome": "rate_limit_reached",
        "rate_limit": {"limit_reached": true}
    });
    let messages = [
        chatgpt::user_message(1, QUERY),
        chatgpt::research_message_with(2, &chatgpt::awaiting_state(), &receipt),
    ];
    let facts = chatgpt::start_facts(&START_STEPS, Some(chatgpt::conversation_body(&messages)));

    let (_, output) = run(facts);

    let (code, kind, _, url) = failure(&output);
    assert_eq!(
        (code, kind, url),
        (Some(4), json!("quota_exhausted"), json!(CONVERSATION_URL))
    );
}

#[test]
fn a_usage_notice_after_sending_is_quota_exhausted() {
    let mut facts = chatgpt::start_facts(&START_STEPS[..6], None);
    facts["quota_notice"] = json!("你已達到深度研究的使用上限");

    let (_, output) = run(facts);

    assert_eq!(
        failure(&output),
        (
            Some(4),
            json!("quota_exhausted"),
            format!(
                "ChatGPT says `你已達到深度研究的使用上限`: the Deep Research quota is used up; wait until it resets; see {CONVERSATION_URL}"
            ),
            json!(CONVERSATION_URL)
        )
    );
}

#[test]
fn a_signed_out_browser_is_an_auth_failure() {
    let fake = FakeOpenCli::answering(
        "",
        "ok: false\nerror:\n  code: AUTH_REQUIRED\n  message: sign in to ChatGPT\n",
        77,
    );
    let environment = RunEnvironment::new(&chatgpt::config(&fake));

    let output = start(&environment, &[]);

    let (code, kind, _, url) = failure(&output);
    assert_eq!(
        (code, kind, url, fake.calls().len()),
        (Some(4), json!("auth"), Value::Null, 1)
    );
}

#[test]
fn a_sign_in_prompt_after_sending_is_auth_and_points_to_the_conversation() {
    let mut facts = chatgpt::start_facts(&START_STEPS[..8], None);
    facts["page"]["signed_out"] = json!(true);

    let (_, output) = run(facts);

    assert_eq!(
        failure(&output),
        (
            Some(4),
            json!("auth"),
            format!(
                "ChatGPT asks the browser to sign in; sign in to chatgpt.com in the Chrome that OpenCLI drives, then read the research with `forager chatgpt research result {CONVERSATION_URL}` before starting again"
            ),
            json!(CONVERSATION_URL)
        )
    );
}

/// Points `chatgpt_browser` at `fake` with a 7-second attempt and allows three attempts, so a
/// retry would fit in the 30-second command deadline the tests pass.
fn retrying_config(fake: &FakeOpenCli) -> String {
    format!(
        "{}timeout = 7\n\n[retry]\nmax_attempts = 3\nmax_wait = 0\n",
        chatgpt::config(fake)
    )
}

const UNKNOWN_OUTCOME: &str = "forager does not know whether ChatGPT received the question; look for it in the ChatGPT history at https://chatgpt.com/ before starting again";

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
    let mut facts = chatgpt::start_facts(&START_STEPS[..6], None);
    facts["timed_out"] = json!(true);

    let (fake, output) = run(facts);

    assert_eq!(
        (failure(&output), fake.calls().len()),
        (
            (
                Some(4),
                json!("timeout"),
                format!(
                    "the read deadline passed before ChatGPT answered the question; read it with `forager chatgpt research result {CONVERSATION_URL}` before starting again"
                ),
                json!(CONVERSATION_URL)
            ),
            1
        )
    );
}

#[test]
fn a_plan_without_a_reread_says_to_read_the_result_first() {
    let mut timed_out = chatgpt::start_facts(&START_STEPS[..8], None);
    timed_out["timed_out"] = json!(true);
    let mut stopped = chatgpt::start_facts(&START_STEPS[..8], None);
    stopped["problem"] = json!("Target closed");

    let failures = [timed_out, stopped].map(|facts| failure(&run(facts).1));

    assert_eq!(
        failures,
        [
            (
                Some(4),
                json!("timeout"),
                format!(
                    "ChatGPT proposed a research plan, but the read deadline passed before forager read the conversation again: {NOT_STARTED_HINT}"
                ),
                json!(CONVERSATION_URL)
            ),
            (
                Some(4),
                json!("runtime"),
                format!(
                    "ChatGPT proposed a research plan, but the forager-chatgpt adapter stopped before forager read the conversation again: Target closed: {NOT_STARTED_HINT}"
                ),
                json!(CONVERSATION_URL)
            ),
        ]
    );
}

#[test]
fn a_sent_question_without_a_conversation_leaves_the_outcome_unknown() {
    let mut facts = chatgpt::start_facts(&START_STEPS[..5], None);
    facts["timed_out"] = json!(true);

    let (_, output) = run(facts);

    assert_eq!(
        failure(&output),
        (
            Some(4),
            json!("timeout"),
            format!(
                "the read deadline passed before ChatGPT opened the new conversation; {UNKNOWN_OUTCOME}"
            ),
            Value::Null
        )
    );
}

#[test]
fn another_conversation_in_the_response_is_runtime() {
    let other = "0b1c2d3e-4f5a-4b6c-8d7e-9f0a1b2c3d4e";
    let facts = chatgpt::start_facts(
        &START_STEPS,
        Some(chatgpt::conversation_body_of(
            other,
            &chatgpt::research_turn(1, &chatgpt::running_state()),
        )),
    );

    let (_, output) = run(facts);

    assert_eq!(
        failure(&output),
        (
            Some(4),
            json!("runtime"),
            format!(
                "the ChatGPT response is for the conversation `{other}` instead of the started conversation {CONVERSATION_URL}"
            ),
            json!(CONVERSATION_URL)
        )
    );
}

#[test]
fn markdown_shows_the_conversation_and_the_plan() {
    let fake = chatgpt::starting(chatgpt::started_facts());
    let environment = RunEnvironment::new(&chatgpt::config(&fake));

    let output = start(&environment, &["--format", "markdown"]);

    assert_eq!(
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned()
        ),
        (
            Some(0),
            format!(
                "# ChatGPT Deep Research started\n\nConversation: <{CONVERSATION_URL}>\n\n## Plan: async-std 維護狀態與替代方案\n\n1. 查閱 async-std 倉庫與公告\n2. 整理官方推薦的替代方案\n3. 比較 smol 與 tokio\n4. 撰寫摘要\n\nRead where it stands with `forager chatgpt research result {CONVERSATION_URL}`.\n"
            )
        )
    );
}

#[test]
fn an_empty_question_exits_2_without_running_opencli() {
    let fake = chatgpt::starting(chatgpt::started_facts());
    let environment = RunEnvironment::new(&chatgpt::config(&fake));

    let output = environment.run(&["chatgpt", "research", "start", "  "]);

    assert_eq!((output.status.code(), fake.calls().len()), (Some(2), 0));
}

#[test]
fn an_adapter_that_stops_before_sending_says_why_nothing_was_sent_and_how_the_draft_ended() {
    let failures = [true, false].map(|cleared| {
        let mut facts = chatgpt::start_facts(&START_STEPS[..3], None);
        facts["problem"] = json!("the composer shows no Deep Research mention");
        facts["draft_cleared"] = json!(cleared);
        failure(&run(facts).1)
    });

    let stopped = "the forager-chatgpt adapter stopped before the question was sent: the composer shows no Deep Research mention (last step: deep_research); nothing was sent";
    assert_eq!(
        failures,
        [
            (
                Some(4),
                json!("runtime"),
                format!("{stopped}; the composer draft was cleared"),
                Value::Null
            ),
            (
                Some(4),
                json!("runtime"),
                format!(
                    "{stopped}; forager could not clear the composer, so remove the leftover draft on chatgpt.com by hand"
                ),
                Value::Null
            )
        ]
    );
}

#[test]
fn a_menu_without_deep_research_is_runtime_and_sends_nothing() {
    let mut facts = chatgpt::start_facts(&START_STEPS[..2], None);
    facts["deep_research_missing"] = json!(true);
    facts["draft_cleared"] = json!(true);

    let (_, output) = run(facts);

    assert_eq!(
        failure(&output),
        (
            Some(4),
            json!("runtime"),
            "ChatGPT's \"Add files and more\" menu in chat mode offers no Deep Research; check that this account can use Deep Research in the ChatGPT web app; nothing was sent; the composer draft was cleared".to_owned(),
            Value::Null
        )
    );
}

#[test]
fn a_start_that_opencli_never_ran_says_nothing_was_sent() {
    let missing_adapter = FakeOpenCli::answering(
        "",
        "ok: false\nerror:\n  code: ADAPTER_LOAD\n  message: cannot load forager-chatgpt/start\n",
        69,
    );
    let configs = [
        chatgpt::config(&missing_adapter),
        "[providers.chatgpt_browser]\ncommand = \"/nonexistent/forager-test/opencli\"\n".to_owned(),
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
