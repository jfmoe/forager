//! Algolia and Firebase bodies derived from the stored samples in `tests/samples/hn_api/`, shared
//! by the command contract tests and, through the crate root, the in-crate tests. A variant
//! changes only what its test is about, so every other field stays as the sample has it.

use serde_json::{Value, json};

use super::samples::sample_json;

/// The stored story list with `ids` in list order.
pub(crate) fn story_list(ids: &[u64]) -> Value {
    let mut list = sample_json("hn_api/firebase-stories.json");
    *list.as_array_mut().expect("story list") = ids.iter().map(|id| json!(id)).collect();
    list
}
