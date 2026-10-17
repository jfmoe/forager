//! X platform fixtures: trimmed real xAI Responses `response.completed` payloads of 2026-10-17,
//! configuration, and output readers shared by the `platform x` tests.

use std::process::Output;

use serde_json::{Value, json};

use super::{Response, request_json};

/// Reads a trimmed real completed response from `tests/fixtures/x`.
pub(crate) fn fixture(name: &str) -> Value {
    let path = format!("{}/tests/fixtures/x/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(path).expect("read fixture")).expect("fixture")
}

/// Streams `response` as xAI does: a creation event, then `response.completed`.
pub(crate) fn completed(response: &Value) -> Response {
    let created = json!({"type": "response.created", "response": {"status": "in_progress"}});
    let completed = json!({"type": "response.completed", "response": response});
    Response::sse(
        200,
        &format!(
            "event: response.created\ndata: {created}\n\nevent: response.completed\ndata: {completed}\n\n"
        ),
    )
}

/// Streams one terminal event of `kind` with `body` merged into it.
pub(crate) fn terminal(kind: &str, body: &Value) -> Response {
    let mut event = body.clone();
    event["type"] = json!(kind);
    Response::sse(200, &format!("event: {kind}\ndata: {event}\n\n"))
}

/// Replaces the structured output of `response` with `posts` and its sources with `cited`
/// status URLs.
pub(crate) fn with_posts(response: &Value, posts: &Value, cited: &[&str]) -> Value {
    let mut response = response.clone();
    let message = response["output"]
        .as_array_mut()
        .expect("output")
        .last_mut()
        .expect("message");
    message["content"][0]["text"] = json!(json!({"posts": posts}).to_string());
    message["content"][0]["annotations"] = cited
        .iter()
        .map(|url| json!({"type": "url_citation", "url": url, "start_index": 0, "end_index": 0, "title": url}))
        .collect();
    response
}

/// A configuration that sends xAI requests to `url` with `keys`; main search also enables web
/// search, which platform requests must never send.
pub(crate) fn config(url: &str, keys: &[&str]) -> String {
    let keys = keys
        .iter()
        .map(|key| format!("\"{key}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "[providers.xai]\nurl = \"{url}\"\nkeys = [{keys}]\nmodel = \"grok-test\"\ntools = [\"web_search\", \"x_search\"]\n[retry]\nmax_wait = 0\n"
    )
}

pub(crate) fn payload(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "parse JSON stdout: {error}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

pub(crate) fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The request line target and JSON body of a captured request.
pub(crate) fn request(request: &str) -> (String, Value) {
    let target = request
        .split_whitespace()
        .nth(1)
        .expect("request target")
        .to_owned();
    (target, request_json(request))
}

pub(crate) fn refs(page: &Value) -> Vec<String> {
    page["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|item| item["ref"].as_str().expect("ref").to_owned())
        .collect()
}
