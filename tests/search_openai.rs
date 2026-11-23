//! Main search through the `openai` backend against a local Responses SSE fixture.

mod support;

use serde_json::Value;

use support::main_search_samples::answer_with;
use support::x_samples::completed_events;
use support::{Fixture, Response, RunEnvironment, request_json};

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
fn repeated_search_backends_are_a_config_error() {
    assert_config_error("[search]\nbackends = [\"openai\", \"openai\"]\n");
}

const RETRY: &str = "\n[retry]\nmax_attempts = 1\nmultiplier = 1\nmax_wait = 0\n";

fn openai_section(url: &str) -> String {
    format!(
        "\n[providers.openai]\nurl = \"{url}/v1\"\nkeys = [\"openai-key\"]\nmodel = \"openai-model\"\n"
    )
}

fn completed_body(answer: &str, source_url: &str) -> String {
    completed_events(&answer_with("openai", answer, &[(source_url, "Source")]))
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
