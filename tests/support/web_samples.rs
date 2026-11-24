//! Web Fetch and web search answers derived from the stored samples, shared by the command
//! contract tests and the in-crate tests; the including module provides the sample store's reader
//! as `samples` and the fixture server as `fixture_server`. A variant changes only the field its
//! test is about, so every other field stays as the sample has it.

use serde_json::{Value, json};

use super::fixture_server::Response;
use super::samples::sample;

/// The stored body at `name` with the string at each pointer of `fields` set to its value.
pub(crate) fn stored_with(name: &str, fields: &[(&str, &str)]) -> String {
    let mut body: Value = serde_json::from_str(&sample(name)).expect("stored JSON");
    for (pointer, value) in fields {
        *body.pointer_mut(pointer).expect("stored field") = json!(value);
    }
    body.to_string()
}

/// The stored Jina Reader answer whose page content is `content`.
pub(crate) fn jina(content: &str) -> Response {
    Response::json(
        200,
        &stored_with("jina/read.json", &[("/data/content", content)]),
    )
}

/// The stored Tavily search answer whose results are `results`.
pub(crate) fn tavily_results(results: &Value) -> Response {
    let mut body: Value = serde_json::from_str(&sample("tavily/search.json")).expect("stored JSON");
    body["results"] = results.clone();
    Response::json(200, &body.to_string())
}
