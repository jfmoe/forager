mod support;

use std::process::Command;
use std::time::Duration;
use std::time::Instant;

use serde_json::Value;

use support::{Fixture, Response, RunEnvironment, run_command};

#[test]
fn map_sends_tavily_options_and_normalizes_site_results() {
    let fixture = Fixture::start(
        200,
        "application/json",
        r#"{
            "base_url": "https://docs.example.test",
            "results": [
                "https://docs.example.test/api?token=source-secret#fragment",
                "https://docs.example.test/guides"
            ],
            "response_time": 0.42
        }"#,
    );
    let environment = RunEnvironment::new(&map_config(&fixture.url, &["tavily-key"]));

    let output = environment.run(&[
        "map",
        "https://docs.example.test",
        "--instructions",
        "Only public API documentation",
        "--max-depth",
        "2",
        "--max-breadth",
        "10",
        "--limit",
        "25",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["provider"],
            &payload["url"],
            &payload["base_url"],
            &payload["results"],
            &payload["response_time"],
        ),
        (
            Some(0),
            &Value::String("tavily".into()),
            &Value::String("https://docs.example.test".into()),
            &Value::String("https://docs.example.test".into()),
            &serde_json::json!([
                "https://docs.example.test/api?token=********",
                "https://docs.example.test/guides"
            ]),
            &Value::from(0.42),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let request = fixture.finish();
    assert!(request.starts_with("POST /map "), "{request}");
    assert!(
        request.contains("authorization: Bearer tavily-key"),
        "{request}"
    );
    assert!(
        request.contains("\"url\":\"https://docs.example.test\""),
        "{request}"
    );
    assert!(
        request.contains("\"instructions\":\"Only public API documentation\""),
        "{request}"
    );
    assert!(request.contains("\"max_depth\":2"), "{request}");
    assert!(request.contains("\"max_breadth\":10"), "{request}");
    assert!(request.contains("\"limit\":25"), "{request}");
    assert!(request.contains("\"timeout\":150"), "{request}");
}

#[test]
fn map_sends_the_minimum_timeout_without_provider_clamping() {
    let fixture = Fixture::start(
        200,
        "application/json",
        r#"{"base_url":"https://docs.example.test","results":["https://docs.example.test"]}"#,
    );
    let environment = RunEnvironment::new(&map_config(&fixture.url, &["tavily-key"]));

    let output = environment.run(&["map", "https://docs.example.test", "--timeout", "10"]);
    let request = fixture.finish();

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(request.contains("\"timeout\":10"), "{request}");
}

#[test]
fn map_treats_an_empty_site_as_a_successful_result() {
    let fixture = Fixture::start(
        200,
        "application/json",
        r#"{"base_url":"https://empty.example.test","results":[],"response_time":0.1}"#,
    );
    let environment = RunEnvironment::new(&map_config(&fixture.url, &["tavily-key"]));

    let output = environment.run(&["map", "https://empty.example.test"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["base_url"],
            &payload["results"],
        ),
        (
            Some(0),
            &Value::String("https://empty.example.test".into()),
            &serde_json::json!([]),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.finish();
}

#[test]
fn map_rejects_malformed_success_envelopes() {
    for body in [
        r"{}",
        r#"{"base_url":"https://docs.example.test"}"#,
        r#"{"results":[]}"#,
        r#"{"base_url":"https://docs.example.test","results":["not-a-url"]}"#,
    ] {
        let fixture = Fixture::start(200, "application/json", body);
        let environment = RunEnvironment::new(&map_config(&fixture.url, &["tavily-key"]));

        let output = environment.run(&["map", "https://docs.example.test"]);
        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

        assert_eq!(
            (output.status.code(), &payload["error_kind"]),
            (Some(4), &Value::String("runtime".into())),
            "body: {body}; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        fixture.finish();
    }
}

#[test]
fn map_reports_tavily_parameter_errors() {
    let fixture = Fixture::start(
        400,
        "application/json",
        r#"{"message":"max_depth is invalid"}"#,
    );
    let environment = RunEnvironment::new(&map_config(&fixture.url, &["tavily-key"]));

    let output = environment.run(&["map", "https://docs.example.test"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (output.status.code(), &payload["error_kind"]),
        (Some(4), &Value::String("parameter".into())),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.finish();
}

#[test]
fn map_rejects_out_of_range_options_before_network_attempts() {
    let cases = [
        ("--timeout", "9"),
        ("--timeout", "151"),
        ("--timeout", "600"),
        ("--max-depth", "0"),
        ("--max-depth", "6"),
        ("--max-breadth", "0"),
        ("--max-breadth", "501"),
        ("--limit", "0"),
    ];

    for (option, value) in cases {
        let fixture = Fixture::start_canary();
        let environment = RunEnvironment::new(&map_config(&fixture.url, &["tavily-key"]));
        let output = environment.run(&[
            "map",
            "https://docs.example.test",
            option,
            value,
            "--verbose",
        ]);

        assert_eq!(
            (
                output.status.code(),
                output.stdout.is_empty(),
                String::from_utf8_lossy(&output.stderr).contains(option),
                fixture.finish_all(),
            ),
            (Some(2), true, true, Vec::new()),
            "option {option} accepted out-of-range value {value}"
        );
    }
}

#[test]
fn map_rejects_content_output_format() {
    let mut command = Command::new(env!("CARGO_BIN_EXE_forager"));
    command
        .args(["map", "https://docs.example.test", "--format", "content"])
        .env_clear();
    let output = run_command(&mut command, None);

    assert_eq!(
        (
            output.status.code(),
            output.stdout.is_empty(),
            String::from_utf8_lossy(&output.stderr).contains("--format"),
        ),
        (Some(2), true, true)
    );
}

#[test]
fn map_reports_tavily_authentication_failures() {
    let fixture = Fixture::start(
        401,
        "application/json",
        r#"{"message":"invalid credential"}"#,
    );
    let environment = RunEnvironment::new(&map_config(&fixture.url, &["tavily-key"]));

    let output = environment.run(&["map", "https://docs.example.test"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (output.status.code(), &payload["error_kind"]),
        (Some(4), &Value::String("auth".into())),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.finish();
}

#[test]
fn map_rotates_tavily_credentials_after_rate_limiting() {
    let fixture = Fixture::start_sequence(vec![
        Response::new(429, "application/json", r#"{"message":"rate limited"}"#),
        Response::new(
            200,
            "application/json",
            r#"{"base_url":"https://docs.example.test","results":[]}"#,
        ),
    ]);
    let environment = RunEnvironment::new(&map_config(&fixture.url, &["first-key", "second-key"]));

    let output = environment.run(&["map", "https://docs.example.test", "--verbose"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let requests = fixture.finish_all();

    assert_eq!(
        (
            output.status.code(),
            &payload["provider_attempts"][0]["error_kind"],
            &payload["provider_attempts"][1]["rotation_count"],
            requests[0].contains("authorization: Bearer first-key"),
            requests[1].contains("authorization: Bearer second-key"),
        ),
        (
            Some(0),
            &Value::String("rate_limited".into()),
            &Value::from(1),
            true,
            true,
        )
    );
}

#[test]
fn map_retries_retryable_failures_inside_the_command_deadline() {
    let fixture = Fixture::start_sequence(vec![
        Response::new(500, "application/json", r#"{"message":"upstream failed"}"#),
        Response::new(
            200,
            "application/json",
            r#"{"base_url":"https://docs.example.test","results":[]}"#,
        ),
    ]);
    let config =
        map_config(&fixture.url, &["tavily-key"]).replace("max_attempts = 1", "max_attempts = 2");
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["map", "https://docs.example.test", "--verbose"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["provider_attempts"][0]["error_kind"],
            &payload["provider_attempts"][1]["retry_count"],
        ),
        (Some(0), &Value::String("network".into()), &Value::from(1),),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.finish_all();
}

#[test]
fn map_caps_each_attempt_below_the_command_deadline() {
    let response = Response::new(
        200,
        "application/json",
        r#"{"base_url":"https://docs.example.test","results":[]}"#,
    )
    .with_delay(Duration::from_millis(1500));
    let fixture = Fixture::start_sequence(vec![response]);
    let config = map_config(&fixture.url, &["tavily-key"]).replace("timeout = 30", "timeout = 1");
    let environment = RunEnvironment::new(&config);

    let started = Instant::now();
    let output = environment.run(&[
        "map",
        "https://docs.example.test",
        "--timeout",
        "10",
        "--verbose",
    ]);
    let elapsed = started.elapsed();
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            &payload["provider_attempts"][0]["error_kind"],
        ),
        (
            Some(4),
            &Value::String("timeout".into()),
            &Value::String("timeout".into()),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(elapsed < Duration::from_secs(3), "elapsed: {elapsed:?}");
    fixture.finish_all();
}

fn map_config(tavily_url: &str, tavily_keys: &[&str]) -> String {
    format!(
        r"
[providers.tavily]
url = {tavily_url:?}
keys = {tavily_keys:?}
timeout = 30

[retry]
max_attempts = 1
multiplier = 1
max_wait = 0

[journal]
enabled = false
"
    )
}
