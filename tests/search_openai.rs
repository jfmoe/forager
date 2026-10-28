//! Main search through the `openai` backend against local Responses SSE fixtures: alone, as the
//! fallback of `xai`, and as the preferred backend.

mod support;

use serde_json::Value;

use support::{Fixture, Response, RunEnvironment, read_only_journal, request_json};

#[test]
fn openai_backend_answers_main_search_through_responses_web_search() {
    let openai = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("Codex answer", "https://example.test/codex"),
    );
    let environment = RunEnvironment::new(&format!(
        "[search]\nbackends = [\"openai\"]\n{}{RETRY}",
        openai_section(&openai.url)
    ));

    let output = environment.run(&["search", "What changed?", "--capabilities", "none"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["answer"],
            &payload["sources"]
        ),
        (
            Some(0),
            &Value::String("Codex answer".into()),
            &serde_json::json!([{"title": "Source", "url": "https://example.test/codex"}]),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let request = openai.finish();
    assert!(request.starts_with("POST /v1/responses "), "{request}");
    assert!(
        request.contains("authorization: Bearer openai-key"),
        "{request}"
    );
    let body = request_json(&request);
    assert_eq!(
        (
            &body["model"],
            &body["stream"],
            &body["tools"],
            body["instructions"].is_string(),
        ),
        (
            &Value::String("openai-model".into()),
            &Value::Bool(true),
            &serde_json::json!([{"type": "web_search"}]),
            true,
        )
    );
}

#[test]
fn openai_answers_after_xai_fails_and_the_journal_records_the_answering_backend() {
    let xai = Fixture::start_json(503, r#"{"error":{"message":"overloaded"}}"#);
    let openai = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("Fallback answer", "https://example.test/fallback"),
    );
    let environment = RunEnvironment::new(&format!(
        "{}{}{RETRY}\n[journal]\nenabled = true\n",
        xai_section(&xai.url),
        openai_section(&openai.url)
    ));

    let output = environment.run(&[
        "search",
        "What changed?",
        "--capabilities",
        "none",
        "--verbose",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let journal = read_only_journal(&environment);

    assert_eq!(
        (
            output.status.code(),
            &payload["answer"],
            attempt_summary(&payload["provider_attempts"]),
            attempt_summary(&journal["execution"]["provider_attempts"]),
            &journal["result"]["provider"],
        ),
        (
            Some(0),
            &Value::String("Fallback answer".into()),
            vec![
                ("xai".into(), "failed".into(), Some(503)),
                ("openai".into(), "succeeded".into(), Some(200)),
            ],
            vec![
                ("xai".into(), "failed".into(), Some(503)),
                ("openai".into(), "succeeded".into(), Some(200)),
            ],
            &Value::String("openai".into()),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    xai.finish();
    openai.finish();
}

#[test]
fn model_override_reaches_only_the_backend_that_runs_first() {
    let xai = Fixture::start_json(503, r#"{"error":{"message":"overloaded"}}"#);
    let openai = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("Fallback answer", "https://example.test/fallback"),
    );
    let environment = RunEnvironment::new(&format!(
        "{}{}{RETRY}",
        xai_section(&xai.url),
        openai_section(&openai.url)
    ));

    let output = environment.run(&[
        "search",
        "What changed?",
        "--capabilities",
        "none",
        "--model",
        "head-model",
    ]);

    assert_eq!(
        (
            output.status.code(),
            request_json(&xai.finish())["model"].clone(),
            request_json(&openai.finish())["model"].clone(),
        ),
        (
            Some(0),
            Value::String("head-model".into()),
            Value::String("openai-model".into()),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn model_override_reaches_the_first_configured_backend_when_the_head_has_no_keys() {
    let openai = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("Codex answer", "https://example.test/codex"),
    );
    let environment = RunEnvironment::new(&format!("{}{RETRY}", openai_section(&openai.url)));

    let output = environment.run(&[
        "search",
        "What changed?",
        "--capabilities",
        "none",
        "--model",
        "head-model",
    ]);

    assert_eq!(
        (
            output.status.code(),
            request_json(&openai.finish())["model"].clone(),
        ),
        (Some(0), Value::String("head-model".into())),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn openai_sources_drop_only_the_tracking_parameter_openai_appends() {
    let body = format!(
        "data: {}\n\n",
        serde_json::json!({
            "type": "response.completed",
            "response": {"output": [{"content": [{
                "type": "output_text",
                "text": "Answer ([example.test](https://example.test/page?id=7&utm_source=openai))",
                "annotations": [
                    {"type": "url_citation", "url": "https://example.test/page?id=7&utm_source=openai", "title": "Page"},
                    {"type": "url_citation", "url": "https://example.test/other?utm_source=openai", "title": "Other"},
                    {"type": "url_citation", "url": "https://example.test/page?id=7", "title": "Page again"}
                ]
            }]}]}
        })
    );
    let openai = Fixture::start(200, "text/event-stream", &body);
    let environment = RunEnvironment::new(&format!(
        "[search]\nbackends = [\"openai\"]\n{}{RETRY}",
        openai_section(&openai.url)
    ));

    let output = environment.run(&["search", "What changed?", "--capabilities", "none"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (output.status.code(), &payload["sources"]),
        (
            Some(0),
            &serde_json::json!([
                {"title": "Page", "url": "https://example.test/page?id=7"},
                {"title": "Other", "url": "https://example.test/other"}
            ]),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    openai.finish();
}

#[test]
fn openai_failed_terminal_event_maps_its_structured_code_and_hides_the_key() {
    let output = run_openai_only(
        "data: {\"type\":\"response.failed\",\"error\":{\"code\":\"insufficient_quota\"}}\n\n",
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            String::from_utf8_lossy(&output.stdout).contains("openai-key"),
        ),
        (Some(4), &Value::String("quota_exhausted".into()), false),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn openai_completed_response_without_output_text_is_a_runtime_failure() {
    let output = run_openai_only(
        "data: {\"type\":\"response.completed\",\"response\":{\"output\":[{\"content\":[{\"type\":\"reasoning\",\"text\":\"hidden\"}]}]}}\n\n",
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (output.status.code(), &payload["error_kind"]),
        (Some(4), &Value::String("runtime".into())),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn openai_answers_when_xai_normalization_leaves_no_answer() {
    let xai = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("<think>hidden only</think>", "https://example.test/hidden"),
    );
    let openai = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("Fallback answer", "https://example.test/fallback"),
    );
    let environment = RunEnvironment::new(&format!(
        "{}{}{RETRY}",
        xai_section(&xai.url),
        openai_section(&openai.url)
    ));

    let output = environment.run(&["search", "What changed?", "--capabilities", "none"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (output.status.code(), &payload["answer"]),
        (Some(0), &Value::String("Fallback answer".into())),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    xai.finish();
    openai.finish();
}

#[test]
fn fallback_off_ends_main_search_at_a_failed_xai_head() {
    let xai = Fixture::start_json(503, r#"{"error":{"message":"overloaded"}}"#);
    let openai = Fixture::start_repeating(Response::sse(
        200,
        &completed_body("Unused answer", "https://example.test/unused"),
    ));
    let environment = RunEnvironment::new(&format!(
        "{}{}{RETRY}",
        xai_section(&xai.url),
        openai_section(&openai.url)
    ));

    let output = environment.run(&[
        "search",
        "What changed?",
        "--capabilities",
        "none",
        "--fallback",
        "off",
    ]);

    assert_eq!(
        (output.status.code(), openai.finish_all().len()),
        (Some(4), 0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    xai.finish();
}

#[test]
fn openai_first_order_answers_without_sending_to_xai() {
    let xai = Fixture::start_repeating(Response::sse(
        200,
        &completed_body("Unused answer", "https://example.test/unused"),
    ));
    let openai = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("Codex answer", "https://example.test/codex"),
    );
    let environment = RunEnvironment::new(&format!(
        "[search]\nbackends = [\"openai\", \"xai\"]\n{}{}{RETRY}",
        xai_section(&xai.url),
        openai_section(&openai.url)
    ));

    let output = environment.run(&["search", "What changed?", "--capabilities", "none"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["answer"],
            xai.finish_all().len()
        ),
        (Some(0), &Value::String("Codex answer".into()), 0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    openai.finish();
}

#[test]
fn main_search_without_keys_for_any_backend_fails_before_flight() {
    let environment = RunEnvironment::new("[search]\nbackends = [\"openai\", \"xai\"]\n");

    let output = environment.run(&["search", "What changed?", "--capabilities", "none"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (output.status.code(), &payload["error_kind"]),
        (Some(3), &Value::String("config".into())),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn search_backends_outside_the_main_search_catalog_are_a_config_error() {
    assert_config_error("[search]\nbackends = [\"openai\", \"gemini_api\"]\n");
}

#[test]
fn repeated_search_backends_are_a_config_error() {
    assert_config_error("[search]\nbackends = [\"openai\", \"openai\"]\n");
}

#[test]
fn an_empty_openai_model_is_a_config_error() {
    assert_config_error("[providers.openai]\nkeys = [\"openai-key\"]\nmodel = \"\"\n");
}

const RETRY: &str = "\n[retry]\nmax_attempts = 1\nmultiplier = 1\nmax_wait = 0\n";

fn openai_section(url: &str) -> String {
    format!(
        "\n[providers.openai]\nurl = \"{url}/v1\"\nkeys = [\"openai-key\"]\nmodel = \"openai-model\"\n"
    )
}

fn completed_body(answer: &str, source_url: &str) -> String {
    format!(
        "data: {}\n\n",
        serde_json::json!({
            "type": "response.completed",
            "response": {"output": [{"content": [{
                "type": "output_text",
                "text": answer,
                "annotations": [{"type": "url_citation", "url": source_url, "title": "Source"}]
            }]}]}
        })
    )
}

fn xai_section(url: &str) -> String {
    format!(
        "\n[providers.xai]\nurl = \"{url}/v1\"\nkeys = [\"xai-key\"]\nmodel = \"xai-model\"\ntools = [\"web_search\"]\n"
    )
}

/// The provider, outcome, and HTTP status of each attempt.
fn attempt_summary(attempts: &Value) -> Vec<(String, String, Option<u64>)> {
    attempts
        .as_array()
        .expect("attempt list")
        .iter()
        .map(|attempt| {
            (
                attempt["provider"].as_str().unwrap_or_default().to_owned(),
                attempt["disposition"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                attempt["http_status"].as_u64(),
            )
        })
        .collect()
}

fn run_openai_only(body: &str) -> std::process::Output {
    let openai = Fixture::start_sequence(vec![Response::sse(200, body)]);
    let environment = RunEnvironment::new(&format!(
        "[search]\nbackends = [\"openai\"]\n{}{RETRY}",
        openai_section(&openai.url)
    ));
    let output = environment.run(&["search", "What changed?", "--capabilities", "none"]);
    openai.finish();
    output
}

fn assert_config_error(config: &str) {
    let environment = RunEnvironment::new(config);

    let output = environment.run(&["search", "What changed?", "--capabilities", "none"]);

    assert_eq!(
        output.status.code(),
        Some(3),
        "{config}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
