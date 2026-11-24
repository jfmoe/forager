//! Crossref bodies derived from the stored samples in `tests/samples/ssrn_crossref/`, shared by
//! the command contract tests and, through the crate root, the in-crate tests. A variant changes
//! only what its test is about, so every other field stays as the sample has it.

use serde_json::{Value, json};

use super::samples::{sample_json, with};

/// The stored `work` body of `ssrn:2042750`, with `fields` set on its record.
pub(crate) fn work_with(fields: &Value) -> Value {
    let mut body = sample_json("ssrn_crossref/work.json");
    body["message"] = with(body["message"].clone(), fields);
    body
}

/// The stored work list holding `items` as a page of a search that matched `total` records.
pub(crate) fn work_list(total: u64, items: &[Value]) -> Value {
    let mut body = sample_json("ssrn_crossref/work-list.json");
    body["message"] = with(
        body["message"].clone(),
        &json!({"total-results": total, "items": items, "items-per-page": items.len()}),
    );
    body
}
