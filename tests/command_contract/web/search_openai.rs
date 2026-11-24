//! Main search through the `openai` backend against a local Responses SSE fixture.

use serde_json::Value;

use crate::support::main_search_samples::{answer_with, completed_events};
use crate::support::{Fixture, RunEnvironment, header, request_json};

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
        (output.status.code(), &payload["answer"]),
        (Some(0), &Value::String("Codex answer".into())),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let request = openai.finish();
    assert_eq!(
        (
            header(&request, "authorization"),
            &request_json(&request)["model"],
        ),
        (
            Some("Bearer openai-key".to_owned()),
            &Value::String("openai-model".into()),
        ),
        "{request}"
    );
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
