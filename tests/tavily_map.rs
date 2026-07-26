mod support;

use std::process::Command;
use std::time::Duration;

use serde_json::Value;

use support::{Fixture, Response, RunEnvironment};

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
fn map_rejects_zero_depth_before_loading_configuration() {
    let output = Command::new(env!("CARGO_BIN_EXE_forager"))
        .args(["map", "https://docs.example.test", "--max-depth", "0"])
        .env_clear()
        .output()
        .expect("run forager");

    assert_eq!(
        (
            output.status.code(),
            output.stdout.is_empty(),
            String::from_utf8_lossy(&output.stderr).contains("--max-depth"),
        ),
        (Some(2), true, true)
    );
}

#[test]
fn map_rejects_content_output_format() {
    let output = Command::new(env!("CARGO_BIN_EXE_forager"))
        .args(["map", "https://docs.example.test", "--format", "content"])
        .env_clear()
        .output()
        .expect("run forager");

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

    let output = environment.run(&[
        "map",
        "https://docs.example.test",
        "--timeout",
        "3",
        "--verbose",
    ]);
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
fn map_preserves_completed_attempts_when_the_hard_deadline_expires() {
    let mut response = Response::new(
        200,
        "application/json",
        r#"{"base_url":"https://docs.example.test","results":[]}"#,
    );
    response.delay = Duration::from_millis(1500);
    let fixture = Fixture::start_sequence(vec![response]);
    let config =
        map_config(&fixture.url, &["tavily-key"]).replace("max_attempts = 1", "max_attempts = 2");
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "map",
        "https://docs.example.test",
        "--timeout",
        "1",
        "--verbose",
    ]);
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
    fixture.finish_all();
}

fn map_config(tavily_url: &str, tavily_keys: &[&str]) -> String {
    format!(
        r#"
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
"#
    )
}
