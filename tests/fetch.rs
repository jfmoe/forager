mod support;

use std::fs;
use std::time::Duration;

use serde_json::Value;

use support::{Fixture, Response, RunEnvironment, jina_response};

const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

#[test]
fn fetch_uses_the_shared_provider_order_and_falls_back_after_thin_html() {
    let tavily = Fixture::start(
        200,
        "application/json",
        r#"{"results":[{"raw_content":"thin"}]}"#,
    );
    let firecrawl = Fixture::start(
        200,
        "application/json",
        r#"{"success":true,"data":{"markdown":"also thin"}}"#,
    );
    let rich_content = "Jina fallback content. ".repeat(30);
    let jina_body = serde_json::json!({
        "code": 200,
        "status": 20000,
        "data": {"content": rich_content}
    })
    .to_string();
    let jina = Fixture::start(200, "application/json", &jina_body);
    let config = fetch_config(
        &jina.url,
        &["jina-key"],
        &tavily.url,
        &["tavily-key"],
        &firecrawl.url,
        &["firecrawl-key"],
        &["tavily", "firecrawl", "jina"],
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
            &payload["provider_attempts"][2]["provider"],
            &payload["provider_attempts"][2]["error_kind"],
        ),
        (
            Some(0),
            &Value::String("jina".into()),
            &Value::String("tavily".into()),
            &Value::String("quality".into()),
            &Value::String("firecrawl".into()),
            &Value::String("quality".into()),
            &Value::String("jina".into()),
            &Value::Null,
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    jina.finish();
    tavily.finish();
    firecrawl.finish();
}

#[test]
fn fetch_reports_quality_when_every_configured_provider_is_thin() {
    let jina = Fixture::start(200, "application/json", &jina_response(""));
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
    jina.finish();
    tavily.finish();
}

#[test]
fn fetch_terminal_kind_and_message_describe_the_same_attempt() {
    let jina = Fixture::start(200, "application/json", &jina_response("thin"));
    let tavily = Fixture::start(
        503,
        "application/json",
        r#"{"message":"service unavailable"}"#,
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
            payload["message"]
                .as_str()
                .is_some_and(|message| message.contains("too thin")),
        ),
        (Some(5), &Value::String("quality".into()), true)
    );
    jina.finish();
    tavily.finish();
}

#[test]
fn fetch_applies_only_the_length_line_to_pdf_content() {
    let content = "P".repeat(250);
    let tavily = Fixture::start(
        200,
        "application/json",
        r#"{"results":[{"raw_content":"thin"}]}"#,
    );
    let firecrawl = Fixture::start(
        200,
        "application/json",
        r#"{"success":true,"data":{"markdown":"also thin"}}"#,
    );
    let jina = Fixture::start(200, "application/json", &jina_response(&content));
    let config = fetch_config(
        &jina.url,
        &["jina-key"],
        &tavily.url,
        &["tavily-key"],
        &firecrawl.url,
        &["firecrawl-key"],
        &["tavily", "firecrawl", "jina"],
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
    tavily.finish();
    firecrawl.finish();
}

#[test]
fn fetch_truncates_oversized_content_on_a_utf8_boundary_with_a_diagnostic() {
    let prefix = r#"{"data":{"content":""#;
    let retained = "a".repeat(MAX_RESPONSE_BYTES - prefix.len() - 1);
    let body = format!("{prefix}{retained}€unreachable suffix");
    let jina = Fixture::start(200, "application/json", &body);
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

    let output = environment.run(&["fetch", "https://example.test/large"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            payload["content"].as_str(),
            String::from_utf8_lossy(&output.stderr).as_ref(),
        ),
        (
            Some(0),
            Some(retained.as_str()),
            "content truncated at 4 MiB\n",
        )
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains('\u{fffd}'));
    jina.finish();
}

#[test]
fn fetch_falls_back_when_truncated_content_is_still_thin() {
    let tavily_body = format!(
        r#"{{"results":[{{"raw_content":"thin","padding":"{}"#,
        "x".repeat(MAX_RESPONSE_BYTES)
    );
    let tavily = Fixture::start(200, "application/json", &tavily_body);
    let rich_content = "Firecrawl fallback after truncated thin content. ".repeat(20);
    let firecrawl_body = serde_json::json!({
        "success": true,
        "data": {"markdown": rich_content}
    })
    .to_string();
    let firecrawl = Fixture::start(200, "application/json", &firecrawl_body);
    let config = fetch_config(
        "http://127.0.0.1:9",
        &[],
        &tavily.url,
        &["tavily-key"],
        &firecrawl.url,
        &["firecrawl-key"],
        &["tavily", "firecrawl", "jina"],
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["fetch", "https://example.test/large", "--verbose"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["provider"],
            &payload["provider_attempts"][0]["provider"],
            &payload["provider_attempts"][0]["error_kind"],
            &payload["provider_attempts"][1]["provider"],
            String::from_utf8_lossy(&output.stderr).as_ref(),
        ),
        (
            Some(0),
            &Value::String("firecrawl".into()),
            &Value::String("tavily".into()),
            &Value::String("quality".into()),
            &Value::String("firecrawl".into()),
            "content truncated at 4 MiB\n",
        )
    );
    tavily.finish();
    firecrawl.finish();
}

#[test]
fn fetch_falls_back_when_jina_returns_malformed_structured_json() {
    let jina = Fixture::start(200, "application/json", r#"{"data":null}"#);
    let rich_content = "Tavily fallback after malformed Jina JSON. ".repeat(20);
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

    let output = environment.run(&["fetch", "https://example.test/article", "--verbose"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["provider"],
            &payload["provider_attempts"][0]["provider"],
            &payload["provider_attempts"][0]["error_kind"],
            &payload["provider_attempts"][1]["provider"],
        ),
        (
            Some(0),
            &Value::String("tavily".into()),
            &Value::String("jina".into()),
            &Value::String("runtime".into()),
            &Value::String("tavily".into()),
        )
    );
    jina.finish();
    tavily.finish();
}

#[test]
fn fetch_preserves_the_available_tavily_content_when_json_is_truncated() {
    let body = format!(
        r#"{{"results":[{{"raw_content":"{}"#,
        "t".repeat(MAX_RESPONSE_BYTES)
    );
    let tavily = Fixture::start(200, "application/json", &body);
    let config = fetch_config(
        "http://127.0.0.1:9",
        &[],
        &tavily.url,
        &["tavily-key"],
        "http://127.0.0.1:9",
        &[],
        &["tavily", "jina", "firecrawl"],
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["fetch", "https://example.test/large-json"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert!(
        output.status.success()
            && payload["content"]
                .as_str()
                .is_some_and(|content| content.len() > MAX_RESPONSE_BYTES - 100)
            && String::from_utf8_lossy(&output.stderr) == "content truncated at 4 MiB\n",
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    tavily.finish();
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
fn fetch_rotates_firecrawl_credentials_after_payment_required() {
    let rich_content = "Rotated Firecrawl content. ".repeat(30);
    let success = serde_json::json!({
        "success": true,
        "data": {"markdown": rich_content}
    })
    .to_string();
    let firecrawl = Fixture::start_sequence(vec![
        Response::new(402, "application/json", r#"{"message":"Payment Required"}"#),
        Response::new(200, "application/json", &success),
    ]);
    let config = fetch_config(
        "http://127.0.0.1:9",
        &[],
        "http://127.0.0.1:9",
        &[],
        &firecrawl.url,
        &["first-key", "second-key"],
        &["firecrawl", "jina", "tavily"],
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["fetch", "https://example.test/article", "--verbose"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["provider_attempts"][0]["error_kind"],
            &payload["provider_attempts"][1]["rotation_count"],
        ),
        (
            Some(0),
            &Value::String("quota_exhausted".into()),
            &Value::from(1),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let requests = firecrawl.finish_all();
    assert!(
        requests[0].contains("authorization: Bearer first-key")
            && requests[1].contains("authorization: Bearer second-key")
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
    let tee = fs::read(output_file.path()).expect("read tee output");

    assert_eq!(
        (output.status.code(), &payload["error_kind"]),
        (Some(4), &Value::String("auth".into()))
    );
    for (sink, contents) in [
        ("stdout", output.stdout.as_slice()),
        ("stderr", output.stderr.as_slice()),
        ("tee", tee.as_slice()),
    ] {
        let contents = String::from_utf8_lossy(contents);
        for secret in [canary, "source-canary", "user:password"] {
            assert!(
                !contents.contains(secret),
                "{sink} leaked protected value {secret}"
            );
        }
    }
    jina.finish();
}

#[test]
fn fetch_reports_timeout_when_the_command_deadline_expires() {
    let jina = Fixture::start_sequence(vec![
        Response::new(200, "application/json", &jina_response("too late"))
            .with_delay(Duration::from_millis(1500)),
    ]);
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
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            payload["message"].as_str().is_some_and(
                |message| message.contains("web_fetch.order has no configured provider")
            ),
            &payload["journal_ref"],
            &payload["journal_status"],
            output.stderr.is_empty(),
        ),
        (
            Some(3),
            &Value::String("config".into()),
            true,
            &Value::Null,
            &Value::String("unavailable".into()),
            true,
        )
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
        r"
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
"
    )
}
