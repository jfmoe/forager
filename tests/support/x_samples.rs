//! xAI Responses bodies of the X route derived from the stored samples in `tests/samples/xai/`,
//! shared by the command contract tests and, through the route's in-crate tests, the crate; the
//! including module provides the sample store's reader as `samples`. A variant changes only what
//! its test is about, so every other field stays as the sample has it.

use serde_json::Value;

use super::samples::sample;

/// The stored completed response `name`, as xAI answered it.
pub(crate) fn stored(name: &str) -> Value {
    serde_json::from_str(&sample(&format!("xai/{name}")))
        .unwrap_or_else(|error| panic!("stored xai/{name}: {error}"))
}
