//! Main-search Responses answers derived from the stored samples
//! `tests/samples/<provider>/search.json`, shared by the command contract tests and the in-crate
//! tests; the including module provides the sample store's reader as `samples`. A variant changes
//! only the answer's text and citations, so every other field stays as the sample has it.

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
    content["text"] = json!(text);
    content["annotations"] = citations
        .iter()
        .map(|(url, title)| json!({"type": "url_citation", "url": url, "title": title}))
        .collect();
    response
}
