use std::fs;

use serde_json::Value;

use crate::support::samples::sample;
use crate::support::web_samples::jina;
use crate::support::{Fixture, Response, RunEnvironment};

/// The stored Tavily extract answer whose page content is `raw_content`.
fn tavily_extract(raw_content: &str) -> String {
    let mut answer: Value =
        serde_json::from_str(&sample("tavily/extract.json")).expect("stored extract answer");
    answer["results"][0]["raw_content"] = serde_json::json!(raw_content);
    answer.to_string()
}

/// The stored Jina Reader error with `message` as its reason.
fn jina_refusal(message: &str) -> String {
    let mut answer: Value =
        serde_json::from_str(&sample("jina/error.json")).expect("stored Reader error");
    answer["message"] = serde_json::json!(message);
    answer["readableMessage"] = serde_json::json!(message);
    answer.to_string()
}

#[test]
fn fetch_reports_quality_when_every_configured_provider_is_thin() {
    let jina = Fixture::start_sequence(vec![jina("")]);
    let tavily = Fixture::start(200, "application/json", &tavily_extract("also thin"));
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
fn fetch_redacts_canaries_from_stdout_stderr_and_tee() {
    let canary = "fetch-canary-do-not-print";
    let refusal = || {
        Response::new(
            401,
            "application/json",
            &jina_refusal(&format!("bad credential {canary}")),
        )
    };
    // Jina's anonymous fallback follows its rejected key.
    let jina = Fixture::start_sequence(vec![refusal(), refusal()]);
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
    jina.finish_all();
}

#[test]
fn fetch_rejects_a_chain_without_configured_credentials_before_network() {
    // Jina runs anonymously without keys, so only an order without it has no configured
    // provider.
    let config = fetch_config(
        "http://127.0.0.1:9",
        &[],
        "http://127.0.0.1:9",
        &[],
        "http://127.0.0.1:9",
        &[],
        &["tavily", "firecrawl"],
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
    let providers = fetch_config_with_default_order(
        jina_url,
        jina_keys,
        tavily_url,
        tavily_keys,
        firecrawl_url,
        firecrawl_keys,
    );
    format!("{providers}\n[capabilities.web_fetch]\norder = {order:?}\n")
}

fn fetch_config_with_default_order(
    jina_url: &str,
    jina_keys: &[&str],
    tavily_url: &str,
    tavily_keys: &[&str],
    firecrawl_url: &str,
    firecrawl_keys: &[&str],
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

[retry]
max_attempts = 1
multiplier = 1
max_wait = 0

[journal]
enabled = false
"
    )
}

#[test]
fn fetch_receipt_writes_the_result_file_and_prints_only_a_receipt() {
    let content = (1..=5)
        .map(|line| format!("Receipt fixture paragraph {line} with enough words to pass the gate."))
        .collect::<Vec<_>>()
        .join("\n");
    let jina = Fixture::start_sequence(vec![jina(&content)]);
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
    let output_file = tempfile::NamedTempFile::new().expect("create output file");
    let output_path = output_file.path().to_string_lossy().into_owned();

    let output = environment.run(&[
        "fetch",
        "https://example.test/article",
        "--output",
        &output_path,
        "--receipt",
    ]);
    let written = fs::read_to_string(output_file.path()).expect("read output file");
    let written_payload: Value = serde_json::from_str(&written).expect("parse written JSON");
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("parse receipt JSON");

    assert_eq!(
        (
            output.status.code(),
            receipt,
            written_payload["content"].as_str()
        ),
        (
            Some(0),
            serde_json::json!({
                "output_path": output_path,
                "bytes": written.len(),
                "lines": 1
            }),
            Some(content.as_str()),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    jina.finish();
}

#[test]
fn fetch_receipt_keeps_the_failure_payload_on_stdout() {
    let refusal = || Response::new(401, "application/json", &jina_refusal("bad credential"));
    let jina = Fixture::start_sequence(vec![refusal(), refusal()]);
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
    let output_file = tempfile::NamedTempFile::new().expect("create output file");
    let output_path = output_file.path().to_string_lossy().into_owned();

    let output = environment.run(&[
        "fetch",
        "https://example.test/article",
        "--output",
        &output_path,
        "--receipt",
    ]);
    let written = fs::read_to_string(output_file.path()).expect("read output file");

    assert_eq!(
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).trim_end(),
        ),
        (Some(4), written.trim_end()),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    jina.finish_all();
}

#[test]
fn fetch_receipt_requires_an_output_file() {
    let environment = RunEnvironment::new("");

    let output = environment.run(&["fetch", "https://example.test/article", "--receipt"]);

    assert_eq!(output.status.code(), Some(2));
}
