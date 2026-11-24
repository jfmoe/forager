//! Exa bodies derived from the stored samples in `tests/samples/exa/`, shared by the command
//! contract tests and, through the crate root, the in-crate tests.

use serde_json::json;

use super::samples::{sample_json, with};

/// The stored search answer with no results.
pub(crate) fn no_results() -> String {
    with(sample_json("exa/search.json"), &json!({"results": []})).to_string()
}
