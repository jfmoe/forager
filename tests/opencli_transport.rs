//! The OpenCLI process transport against a fake `opencli`: how forager reads the OpenCLI 2.x
//! output envelope and exit code of an adapter command, seen through the `ssrn_browser` route.
#![cfg(unix)]

mod support;

use serde_json::{Value, json};

use support::RunEnvironment;
use support::opencli::{self, FakeOpenCli};

const UPGRADE_HINT: &str = "forager needs OpenCLI 2.x and Browser Bridge 1.0.25 or later";

fn search(fake: &FakeOpenCli) -> Value {
    let environment = RunEnvironment::new(&fake.config("[\"ssrn_browser\"]"));
    let output = environment.run(&["platform", "ssrn", "search", "dual momentum", "--verbose"]);
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "parse JSON stdout: {error}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// Runs a browser-only search against a fake that prints `stdout` and `stderr` and exits with
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
        ("EMPTY_RESULT", "ssrn/search returned no data", 66),
        ("BROWSER_CONNECT", "Browser Bridge is not connected", 69),
        ("TIMEOUT", "ssrn/search timed out after 40s", 75),
        (
            "AUTH_REQUIRED",
            "SSRN security verification did not clear",
            77,
        ),
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
        "verification at https://papers.ssrn.com/sol3/x?token=secret did not clear",
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
        ("ADAPTER_LOAD", "cannot load ssrn/search.js", 69),
        ("unknown_command", "Unknown command: ssrn", 2),
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
                "copy the `opencli/ssrn` directory of the forager skill to `~/.opencli/clis/ssrn`",
            ),
        )
    });

    assert_eq!(failures.to_vec(), vec![(json!("runtime"), true); 3]);
}

#[test]
fn another_contract_version_is_a_runtime_failure_with_an_install_hint() {
    let stdout =
        opencli::success(&json!({"contract": "forager-ssrn/0", "status": "ok", "data": {}}));

    let (kind, message) = failed_search(&stdout, "", 0);

    assert_eq!(
        (
            kind,
            message.contains("forager-ssrn/0"),
            message.contains("install or update")
        ),
        (json!("runtime"), true, true),
        "{message}"
    );
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
        json!({"contract": "forager-ssrn/4", "status": "ok", "data": {}}).to_string();
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
    let fake = FakeOpenCli::envelope(
        "no_results",
        &json!({"request_url": "", "total": 0, "papers": []}),
    );

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
