//! xAI Responses bodies of the X route derived from the stored samples in `tests/samples/xai/`,
//! shared by the command contract tests and, through the route's in-crate tests, the crate; the
//! including module provides the sample store's reader as `samples`. A variant changes only what
//! its test is about, so every other field stays as the sample has it.

use serde_json::{Value, json};

use super::samples::sample;

/// The stored completed response `name`, as xAI answered it.
pub(crate) fn stored(name: &str) -> Value {
    serde_json::from_str(&sample(&format!("xai/{name}")))
        .unwrap_or_else(|error| panic!("stored xai/{name}: {error}"))
}

/// The event stream xAI sends for `response`: a creation event, then `response.completed`.
pub(crate) fn completed_events(response: &Value) -> String {
    let created = json!({"type": "response.created", "response": {"status": "in_progress"}});
    let completed = json!({"type": "response.completed", "response": response});
    format!(
        "event: response.created\ndata: {created}\n\nevent: response.completed\ndata: {completed}\n\n"
    )
}
