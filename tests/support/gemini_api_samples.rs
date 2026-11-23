//! Gemini API bodies derived from the stored samples in `tests/samples/gemini_api/`, shared by
//! the command contract tests and, through the route's in-crate tests, the crate; the including
//! module provides the sample store's reader as `samples`. A variant changes only what its test
//! is about, so every other field stays as the sample has it.

use serde_json::{Value, json};

use super::samples::sample;

/// The stored body `name`, as the Gemini API answered it.
pub(crate) fn stored(name: &str) -> Value {
    serde_json::from_str(&sample(&format!("gemini_api/{name}")))
        .unwrap_or_else(|error| panic!("stored gemini_api/{name}: {error}"))
}

/// The stored `generateContent` answer with `text` after its reasoning summary, stopped for
/// `finish_reason`.
pub(crate) fn generated(text: &str, finish_reason: &str) -> Value {
    let mut body = stored("generate-content.json");
    let candidate = &mut body["candidates"][0];
    candidate["content"]["parts"][1]["text"] = json!(text);
    candidate["finishReason"] = json!(finish_reason);
    body
}

/// The stored error body as a `status` answer with the status reason `reason` and `message`.
pub(crate) fn error(status: u16, reason: &str, message: &str) -> Value {
    let mut body = stored("error.json");
    body["error"] = json!({"code": status, "message": message, "status": reason});
    body
}
