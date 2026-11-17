//! The OpenCLI process transport against a fake `opencli`: how forager runs an adapter command
//! and reads its OpenCLI 2.x output envelope and exit code, seen through the `xiaohongshu_browser`
//! route.
#![cfg(unix)]

mod support;

use std::fs;
use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use support::RunEnvironment;
use support::opencli::{self, FakeOpenCli};

const UPGRADE_HINT: &str = "forager needs OpenCLI 2.x and Browser Bridge 1.0.25 or later";
const SESSION_FLAGS: [&str; 8] = [
    "-f",
    "json",
    "--window",
    "background",
    "--site-session",
    "ephemeral",
    "--keep-tab",
    "false",
];

fn payload(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "parse JSON stdout: {error}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn search(fake: &FakeOpenCli) -> Value {
    let environment = RunEnvironment::new(&fake.config("[\"xiaohongshu_browser\"]"));
    let output = environment.run(&["platform", "xiaohongshu", "search", "咖啡", "--verbose"]);
    payload(&output)
}

/// Runs a search against a fake that prints `stdout` and `stderr` and exits with
/// `code`, and returns the error kind and message.
fn failed_search(stdout: &str, stderr: &str, code: i32) -> (Value, String) {
    let payload = search(&FakeOpenCli::answering(stdout, stderr, code));
    assert_eq!(
        payload["provider_attempts"][0]["transport"],
        json!("process"),
        "{payload}"
    );
    (
        payload["error_kind"].clone(),
        payload["message"].as_str().unwrap_or_default().to_owned(),
    )
}

fn failing(code: &str, message: &str, exit: i32) -> (Value, String) {
    failed_search(&opencli::failure(code, message, exit), "", exit)
}

#[test]
fn opencli_exit_codes_map_to_error_kinds() {
    let kinds = [
        ("EMPTY_RESULT", "forager-xhs/search returned no data", 66),
        ("BROWSER_CONNECT", "Browser Bridge is not connected", 69),
        ("TIMEOUT", "forager-xhs/search timed out after 40s", 75),
        ("AUTH_REQUIRED", "Xiaohongshu asks to log in", 77),
        (
            "ARGUMENT",
            "--window must be one of: foreground, background",
            2,
        ),
        ("COMMAND_EXEC", "boom", 1),
    ]
    .map(|(code, message, exit)| failing(code, message, exit).0);

    assert_eq!(
        kinds,
        [
            json!("runtime"),
            json!("network"),
            json!("timeout"),
            json!("auth"),
            json!("runtime"),
            json!("runtime")
        ]
    );
}

#[test]
fn the_failure_message_passes_through_with_urls_redacted() {
    let (_, message) = failing(
        "COMMAND_EXEC",
        "verification at https://www.xiaohongshu.com/explore?xsec_token=secret did not clear",
        1,
    );

    assert_eq!(
        (
            message.starts_with("OpenCLI exited with 1: verification at "),
            message.ends_with(" did not clear"),
            message.contains("secret"),
        ),
        (true, true, false),
        "{message}"
    );
}

#[test]
fn a_missing_or_outdated_adapter_is_a_runtime_failure_with_an_install_hint() {
    let failures = [
        ("ADAPTER_LOAD", "cannot load forager-xhs/search.js", 69),
        ("unknown_command", "Unknown command: forager-xhs", 2),
        (
            "commander.unknownCommand",
            "error: unknown command 'search'",
            2,
        ),
    ]
    .map(|(code, message, exit)| {
        let (kind, message) = failing(code, message, exit);
        (
            kind,
            message.contains(
                "copy the `opencli/forager-xhs` directory of the forager skill to `~/.opencli/clis/forager-xhs`",
            ),
        )
    });

    assert_eq!(failures.to_vec(), vec![(json!("runtime"), true); 3]);
}

#[test]
fn output_without_an_error_envelope_reports_the_first_stderr_line() {
    let (kind, message) = failed_search("", "\nSegmentation fault\nmore\n", 139);

    assert_eq!(
        (kind, message.as_str()),
        (
            json!("runtime"),
            "OpenCLI exited with 139: Segmentation fault"
        )
    );
}

#[test]
fn opencli_1_output_is_a_runtime_failure_that_asks_to_upgrade() {
    let bare_envelope =
        json!({"contract": "forager-xhs/2", "status": "ok", "data": {}}).to_string();
    let yaml_error = "ok: false\nerror:\n  code: AUTH_REQUIRED\n  message: verification did not clear\n  exitCode: 77\n";

    let failures = [
        failed_search(&bare_envelope, "", 0),
        failed_search("", yaml_error, 77),
    ]
    .map(|(kind, message)| (kind, message.contains(UPGRADE_HINT)));

    assert_eq!(failures.to_vec(), vec![(json!("runtime"), true); 2]);
}

#[test]
fn forager_passes_no_opencli_settings_through_the_environment() {
    let fake = FakeOpenCli::failing("EMPTY_RESULT", "forager-xhs/search returned no data", 66);

    search(&fake);
    let environments = fake.environments();

    assert_eq!(
        (
            environments.len(),
            environments[0]
                .iter()
                .filter(|variable| variable.starts_with("OPENCLI_"))
                .collect::<Vec<_>>()
        ),
        (1, Vec::<&String>::new())
    );
}

#[test]
fn every_command_ends_with_its_deadline_and_the_session_flags() {
    let fake = FakeOpenCli::failing("EMPTY_RESULT", "forager-xhs/search returned no data", 66);

    search(&fake);
    let call = fake.calls().remove(0);
    let (rest, flags) = call.split_at(call.len() - SESSION_FLAGS.len());
    let [command @ .., flag, timeout] = rest else {
        panic!("argv {call:?} lacks --timeout");
    };
    let timeout: u64 = timeout.parse().expect("integer --timeout");

    assert_eq!(
        (&command[..2], flag.as_str(), flags),
        (
            &["forager-xhs".to_owned(), "search".to_owned()][..],
            "--timeout",
            &SESSION_FLAGS.map(str::to_owned)[..]
        ),
        "argv {call:?}"
    );
    assert!(
        (1..=115).contains(&timeout),
        "the command timeout leaves the cleanup reserve of the 120-second attempt: {timeout}"
    );
}

#[test]
fn a_missing_executable_is_a_runtime_failure() {
    let environment = RunEnvironment::new(
        "[providers.xiaohongshu_browser]\ncommand = \"/nonexistent/opencli\"\n\n[platforms.xiaohongshu]\norder = [\"xiaohongshu_browser\"]\n",
    );

    let output = environment.run(&["platform", "xiaohongshu", "search", "咖啡"]);

    assert_eq!(
        (output.status.code(), payload(&output)["error_kind"].clone()),
        (Some(4), json!("runtime"))
    );
}

fn process_exists(pid: &str) -> bool {
    Command::new("/bin/kill")
        .args(["-0", pid.trim()])
        .output()
        .is_ok_and(|output| output.status.success())
}

#[test]
fn a_command_past_its_working_deadline_is_killed_with_its_process_group() {
    let fake = FakeOpenCli::hanging();
    let environment = RunEnvironment::new(&fake.config("[\"xiaohongshu_browser\"]").replace(
        "[providers.xiaohongshu_browser]\n",
        "[providers.xiaohongshu_browser]\ntimeout = 7\n",
    ));
    let started = Instant::now();

    let output = environment.run(&["platform", "xiaohongshu", "search", "咖啡"]);
    let elapsed = started.elapsed();
    let pids = ["leader.pid", "child.pid"]
        .map(|name| fs::read_to_string(fake.path(name)).expect("recorded pid"));
    let deadline = Instant::now() + Duration::from_secs(3);
    while pids.iter().any(|pid| process_exists(pid)) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }

    assert_eq!(
        (
            output.status.code(),
            payload(&output)["error_kind"].clone(),
            pids.iter().any(|pid| process_exists(pid)),
        ),
        (Some(4), json!("timeout"), false)
    );
    assert!(
        elapsed < Duration::from_secs(6),
        "the working deadline stops the command before the attempt deadline: {elapsed:?}"
    );
}
