//! Live smoke of the `openai` main-search case: it runs once `providers.openai.keys` is set,
//! and searches through `openai` alone although `xai` heads the configured chain.

mod support;

use serde_json::{Value, json};
use support::smoke::{DISABLED_PLATFORMS, SmokeEnvironment, case};
use support::{Fixture, Response, request_json};

#[test]
fn live_smoke_runs_the_openai_case_through_openai_alone_while_xai_heads_the_chain() {
    let openai = Fixture::start(
        200,
        "text/event-stream",
        &format!(
            "data: {}\n\n",
            json!({
                "type": "response.completed",
                "response": {"output": [{"content": [{
                    "type": "output_text",
                    "text": "Rust stable release",
                    "annotations": [{"type": "url_citation", "url": "https://example.test/rust", "title": "Rust"}]
                }]}]}
            })
        ),
    );
    let xai = Fixture::start_repeating(Response::json(503, r#"{"error":"unavailable"}"#));
    let openai_endpoint = format!("{}/v1", openai.url);
    let xai_endpoint = format!("{}/v1", xai.url);
    let environment = SmokeEnvironment::new(|journal_dir| {
        format!(
            "[providers.xai]\nurl = {xai_endpoint:?}\nkeys = [\"xai-secret\"]\n[providers.openai]\nurl = {openai_endpoint:?}\nkeys = [\"openai-secret\"]\n{DISABLED_PLATFORMS}[journal]\ndir = {journal_dir:?}\n"
        )
    });

    let output = environment.run(&["smoke", "--live", "--timeout", "2"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse live smoke JSON");

    assert_eq!(
        (
            &case(&payload, "C01")["status"],
            &case(&payload, "C49")["status"],
            &case(&payload, "C49")["transport"],
        ),
        (
            &Value::String("failed".into()),
            &Value::String("passed".into()),
            &Value::String("sse".into()),
        ),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        request_json(&openai.finish())["tools"],
        json!([{"type": "web_search"}])
    );
    xai.finish_all();
}
