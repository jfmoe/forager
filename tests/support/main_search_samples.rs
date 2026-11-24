//! Main-search Responses answers and failures derived from the stored samples
//! `tests/samples/<provider>/search.json` and `response-failed.json`, and the event stream that
//! carries any completed Responses answer, opened by the stored `xai/response-created.json`;
//! shared by the command contract tests and the in-crate tests, and the including module provides
//! the sample store's reader as `samples`. A variant changes only the answer's text and
//! citations, so every other field stays as the sample has it.

use serde_json::{Value, json};

use super::samples::sample;

/// The stored main-search answer of `provider`, `xai` or `openai`, as its Responses endpoint
/// completed it.
pub(crate) fn stored_answer(provider: &str) -> Value {
    serde_json::from_str(&sample(&format!("{provider}/search.json")))
        .unwrap_or_else(|error| panic!("stored {provider}/search.json: {error}"))
}

/// The stored main-search answer of `provider` with its output text replaced by `text` and its
/// citations by `citations`, each a URL and a title.
pub(crate) fn answer_with(provider: &str, text: &str, citations: &[(&str, &str)]) -> Value {
    let mut response = stored_answer(provider);
    let content = &mut response["output"][0]["content"][0];
    let stored_citation = content["annotations"][0].clone();
    content["text"] = json!(text);
    content["annotations"] = citations
        .iter()
        .map(|(url, title)| {
            let mut citation = stored_citation.clone();
            citation["url"] = json!(url);
            citation["title"] = json!(title);
            citation
        })
        .collect();
    response
}

/// The stored `response.failed` event that ends a failed Responses stream of `provider`, `xai`
/// or `openai`.
pub(crate) fn failed_event(provider: &str) -> Value {
    serde_json::from_str(&sample(&format!("{provider}/response-failed.json")))
        .unwrap_or_else(|error| panic!("stored {provider}/response-failed.json: {error}"))
}

/// The event stream a Responses endpoint sends for `response`: the stored creation event, which
/// the protocol's providers share, then `response.completed`.
pub(crate) fn completed_events(response: &Value) -> String {
    let created: Value = serde_json::from_str(&sample("xai/response-created.json"))
        .unwrap_or_else(|error| panic!("stored xai/response-created.json: {error}"));
    let completed = json!({"type": "response.completed", "response": response});
    format!(
        "event: response.created\ndata: {created}\n\nevent: response.completed\ndata: {completed}\n\n"
    )
}
