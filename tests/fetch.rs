mod support;

use std::fs;
use std::time::{Duration, Instant};

use serde_json::Value;

use support::{Fixture, Response, RunEnvironment};

#[test]
fn fetch_follows_authoritative_order_and_falls_back_after_thin_html() {
    let jina = Fixture::start(200, "text/markdown", "thin");
    let rich_content = "Tavily fallback content. ".repeat(30);
    let tavily_body = serde_json::json!({
        "results": [{"raw_content": rich_content}]
    })
    .to_string();
    let tavily = Fixture::start(200, "application/json", &tavily_body);
    let config = fetch_config(
        &jina.url,
        &["jina-key"],
        &tavily.url,
        &["tavily-key"],
        "http://127.0.0.1:9",
        &[],
        &["jina", "firecrawl", "tavily"],
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["fetch", "https://example.test/article", "--verbose"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["provider"],
            &payload["provider_attempts"][0]["provider"],
            &payload["provider_attempts"][0]["error_kind"],
            &payload["provider_attempts"][1]["provider"],
            &payload["provider_attempts"][1]["error_kind"],
        ),
        (
            Some(0),
            &Value::String("tavily".into()),
            &Value::String("jina".into()),
            &Value::String("quality".into()),
            &Value::String("tavily".into()),
            &Value::Null,
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    jina.finish();
    tavily.finish();
}

#[test]
fn fetch_reports_quality_when_every_configured_provider_is_thin() {
    let jina = Fixture::start(200, "text/markdown", "");
    let tavily = Fixture::start(
        200,
        "application/json",
        r#"{"results":[{"raw_content":"also thin"}]}"#,
    );
    let config = fetch_config(
        &jina.url,
        &["jina-key"],
        &tavily.url,
        &["tavily-key"],
        "http://127.0.0.1:9",
        &[],
        &["jina", "tavily", "firecrawl"],
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["fetch", "https://example.test/article", "--verbose"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            payload["provider_attempts"].as_array().map(Vec::len),
            payload["provider_attempts"]
                .as_array()
                .is_some_and(|attempts| attempts
                    .iter()
                    .all(|attempt| { attempt["error_kind"] == Value::String("quality".into()) })),
        ),
        (Some(5), &Value::String("quality".into()), Some(2), true,)
    );
    assert_ne!(payload["error_kind"], Value::String("evidence".into()));
    jina.finish();
    tavily.finish();
}

#[test]
fn fetch_applies_only_the_length_line_to_pdf_content() {
    let content = "P".repeat(250);
    let jina = Fixture::start(200, "text/markdown", &content);
    let config = fetch_config(
        &jina.url,
        &["jina-key"],
        "http://127.0.0.1:9",
        &[],
        "http://127.0.0.1:9",
        &[],
        &["jina", "tavily", "firecrawl"],
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["fetch", "https://example.test/report.PDF?download=1"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["provider"],
            payload["content"].as_str().map(str::len),
        ),
        (Some(0), &Value::String("jina".into()), Some(250))
    );
    jina.finish();
}

#[test]
fn fetch_rotates_tavily_credentials_after_rate_limiting() {
    let rich_content = "Rotated Tavily content. ".repeat(30);
    let success = serde_json::json!({
        "results": [{"raw_content": rich_content}]
    })
    .to_string();
    let tavily = Fixture::start_sequence(vec![
        Response::new(429, "application/json", r#"{"message":"rate limited"}"#),
        Response::new(200, "application/json", &success),
    ]);
    let config = fetch_config(
        "http://127.0.0.1:9",
        &[],
        &tavily.url,
        &["first-key", "second-key"],
        "http://127.0.0.1:9",
        &[],
        &["tavily", "jina", "firecrawl"],
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["fetch", "https://example.test/article", "--verbose"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let requests = tavily.finish_all();

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
fn fetch_attributes_only_tavilys_final_attempt_after_rotation() {
    let tavily = Fixture::start_sequence(vec![
        Response::new(429, "application/json", r#"{"message":"rate limited"}"#),
        Response::new(500, "application/json", r#"{"message":"upstream failed"}"#),
    ]);
    let config = fetch_config(
        "http://127.0.0.1:9",
        &[],
        &tavily.url,
        &["first-key", "second-key"],
        "http://127.0.0.1:9",
        &[],
        &["tavily", "jina", "firecrawl"],
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["fetch", "https://example.test/article", "--verbose"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            &payload["provider_attempts"][0]["error_kind"],
            &payload["provider_attempts"][1]["error_kind"],
        ),
        (
            Some(4),
            &Value::String("network".into()),
            &Value::String("rate_limited".into()),
            &Value::String("network".into()),
        )
    );
    tavily.finish_all();
}

#[test]
fn fetch_retries_a_timed_out_attempt_inside_the_shared_deadline() {
    let rich_content = "Retried Jina content. ".repeat(30);
    let jina = Fixture::start_sequence(vec![
        delayed(
            Response::new(200, "text/markdown", "too late"),
            Duration::from_millis(1100),
        ),
        Response::new(202, "text/markdown", &rich_content),
    ]);
    let config = fetch_config(
        &jina.url,
        &["jina-key"],
        "http://127.0.0.1:9",
        &[],
        "http://127.0.0.1:9",
        &[],
        &["jina", "tavily", "firecrawl"],
    )
    .replace("timeout = 30", "timeout = 1")
    .replace("max_attempts = 1", "max_attempts = 2");
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "fetch",
        "https://example.test/article",
        "--timeout",
        "4",
        "--verbose",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["provider"],
            &payload["provider_attempts"][0]["error_kind"],
            &payload["provider_attempts"][1]["http_status"],
            &payload["provider_attempts"][1]["retry_count"],
        ),
        (
            Some(0),
            &Value::String("jina".into()),
            &Value::String("timeout".into()),
            &Value::from(202),
            &Value::from(1),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    jina.finish_all();
}

#[test]
fn fetch_preserves_fallback_budget_under_one_hard_deadline() {
    let jina = Fixture::start_sequence(vec![delayed(
        Response::new(200, "text/markdown", "too late"),
        Duration::from_millis(6200),
    )]);
    let rich_content = "Deadline fallback content. ".repeat(30);
    let tavily_body = serde_json::json!({
        "results": [{"raw_content": rich_content}]
    })
    .to_string();
    let tavily = Fixture::start(200, "application/json", &tavily_body);
    let config = fetch_config(
        &jina.url,
        &["jina-key"],
        &tavily.url,
        &["tavily-key"],
        "http://127.0.0.1:9",
        &[],
        &["jina", "tavily", "firecrawl"],
    );
    let environment = RunEnvironment::new(&config);

    let started = Instant::now();
    let output = environment.run(&[
        "fetch",
        "https://example.test/article",
        "--timeout",
        "12",
        "--verbose",
    ]);
    let elapsed = started.elapsed();
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["provider"],
            &payload["provider_attempts"][0]["error_kind"],
            &payload["provider_attempts"][1]["provider"],
        ),
        (
            Some(0),
            &Value::String("tavily".into()),
            &Value::String("timeout".into()),
            &Value::String("tavily".into()),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(elapsed < Duration::from_secs(12), "elapsed: {elapsed:?}");
    jina.finish_all();
    tavily.finish();
}

#[test]
fn fetch_redacts_canaries_from_stdout_stderr_and_tee() {
    let canary = "fetch-canary-do-not-print";
    let jina = Fixture::start(
        401,
        "application/json",
        &format!(r#"{{"message":"bad credential {canary}"}}"#),
    );
    let config = fetch_config(
        &jina.url,
        &[canary],
        "http://127.0.0.1:9",
        &[],
        "http://127.0.0.1:9",
        &[],
        &["jina", "tavily", "firecrawl"],
    );
    let environment = RunEnvironment::new(&config);
    let output_file = tempfile::NamedTempFile::new().expect("create tee file");
    let output_path = output_file.path().to_string_lossy().into_owned();

    let output = environment.run(&[
        "fetch",
        "https://user:password@example.test/article?api_key=source-canary#fragment",
        "--output",
        &output_path,
        "--verbose",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let combined = format!(
        "{}{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&fs::read(output_file.path()).expect("read tee output"))
    );

    assert_eq!(
        (output.status.code(), &payload["error_kind"]),
        (Some(4), &Value::String("auth".into()))
    );
    assert!(!combined.contains(canary));
    assert!(!combined.contains("source-canary"));
    assert!(!combined.contains("user:password"));
    jina.finish();
}

#[test]
fn fetch_reports_timeout_when_the_command_deadline_expires() {
    let jina = Fixture::start_sequence(vec![delayed(
        Response::new(200, "text/markdown", "too late"),
        Duration::from_millis(1500),
    )]);
    let config = fetch_config(
        &jina.url,
        &["jina-key"],
        "http://127.0.0.1:9",
        &[],
        "http://127.0.0.1:9",
        &[],
        &["jina", "tavily", "firecrawl"],
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "fetch",
        "https://example.test/article",
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
        )
    );
    jina.finish_all();
}

#[test]
fn fetch_rejects_a_chain_without_configured_credentials_before_network() {
    let config = fetch_config(
        "http://127.0.0.1:9",
        &[],
        "http://127.0.0.1:9",
        &[],
        "http://127.0.0.1:9",
        &[],
        &["jina", "tavily", "firecrawl"],
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["fetch", "https://example.test/article"]);

    assert_eq!(
        (
            output.status.code(),
            output.stdout.is_empty(),
            String::from_utf8_lossy(&output.stderr)
                .contains("web_fetch.order has no configured provider"),
        ),
        (Some(3), true, true)
    );
}

fn fetch_config(
    jina_url: &str,
    jina_keys: &[&str],
    tavily_url: &str,
    tavily_keys: &[&str],
    firecrawl_url: &str,
    firecrawl_keys: &[&str],
    order: &[&str],
) -> String {
    format!(
        r#"
[providers.jina]
url = {jina_url:?}
keys = {jina_keys:?}
timeout = 30

[providers.tavily]
url = {tavily_url:?}
keys = {tavily_keys:?}
timeout = 30

[providers.firecrawl]
url = {firecrawl_url:?}
keys = {firecrawl_keys:?}
timeout = 30

[capabilities.web_fetch]
order = {order:?}

[retry]
max_attempts = 1
multiplier = 1
max_wait = 0

[journal]
enabled = false
"#
    )
}

fn delayed(mut response: Response, delay: Duration) -> Response {
    response.delay = delay;
    response
}
