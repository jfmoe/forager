//! Doctor for `chatgpt_browser`, a process provider that only explicit `forager chatgpt`
//! commands run: the default doctor leaves it alone, and `--provider chatgpt_browser` reads the
//! adapter's `status` against a fake `opencli`.
#![cfg(unix)]

mod support;

use serde_json::{Value, json};

use support::chatgpt;
use support::doctor::{assert_deep_success, reachable_responses, shallow_config};
use support::opencli::FakeOpenCli;
use support::{Fixture, RunEnvironment};

#[test]
fn the_default_doctor_leaves_the_chatgpt_route_alone() {
    let fixture = Fixture::start_sequence(reachable_responses(9));
    let fake = FakeOpenCli::answering("", "", 1);
    let environment = RunEnvironment::new(&format!(
        "{}\n{}",
        shallow_config(&fixture.url),
        chatgpt::config(&fake)
    ));

    let output = environment.run(&["doctor"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");
    let status = payload["providers"]
        .as_array()
        .expect("providers")
        .iter()
        .find(|provider| provider["provider"] == "chatgpt_browser")
        .expect("chatgpt_browser status")
        .clone();

    assert_eq!(
        (
            output.status.code(),
            &payload["ok"],
            &status["configured"],
            &status["message"],
            fake.calls().len(),
        ),
        (
            Some(0),
            &Value::Bool(true),
            &Value::Bool(false),
            &json!("checked only by `forager doctor --provider chatgpt_browser`"),
            0
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fixture.finish_all().len(), 9);
}

#[test]
fn the_chatgpt_deep_doctor_reads_the_adapter_status_in_the_background() {
    let fake = chatgpt::status_answering(chatgpt::status(false));
    let environment = RunEnvironment::new(&chatgpt::config(&fake));

    let output = environment.run(&["doctor", "--provider", "chatgpt_browser"]);

    assert_deep_success(&output, "chatgpt_browser", &[("status", "process")]);
    let calls = fake.calls();
    assert_eq!(
        (calls.len(), calls[0][..3].to_vec(), calls[0][4..].to_vec()),
        (
            1,
            ["forager-chatgpt", "status", "--timeout"]
                .map(String::from)
                .to_vec(),
            [
                "-f",
                "json",
                "--window",
                "background",
                "--site-session",
                "ephemeral",
                "--keep-tab",
                "false"
            ]
            .map(String::from)
            .to_vec()
        )
    );
}

#[test]
fn the_chatgpt_deep_doctor_reports_a_signed_out_browser_as_auth() {
    let fake = chatgpt::status_answering(chatgpt::status(true));
    let environment = RunEnvironment::new(&chatgpt::config(&fake));

    let output = environment.run(&["doctor", "--provider", "chatgpt_browser"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");

    assert_eq!(
        (output.status.code(), &payload["ok"], &payload["error_kind"]),
        (Some(4), &Value::Bool(false), &json!("auth")),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn the_chatgpt_deep_doctor_reports_a_missing_adapter_with_install_steps() {
    let fake = FakeOpenCli::failing("unknown_command", "Unknown command: forager-chatgpt", 2);
    let environment = RunEnvironment::new(&chatgpt::config(&fake));

    let output = environment.run(&["doctor", "--provider", "chatgpt_browser"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");
    let message = payload["message"].as_str().unwrap_or_default();

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            message.contains("copy the `opencli/forager-chatgpt` directory")
        ),
        (Some(4), &json!("runtime"), true),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}
