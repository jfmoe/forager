//! Web Fetch answers derived from the stored samples, shared by the command contract tests and
//! the in-crate tests, whose `web_samples` module adds the variants only they use; the including
//! module provides the fixture server as `fixture_server` and the sample reader as `samples`. A
//! variant changes only the field its test is about, so every other field stays as the sample has
//! it.

use serde_json::json;

use super::fixture_server::Response;
use super::samples::sample_json;

/// The stored body at `name` with the string at each pointer of `fields` set to its value.
pub(crate) fn stored_with(name: &str, fields: &[(&str, &str)]) -> String {
    let mut body = sample_json(name);
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
