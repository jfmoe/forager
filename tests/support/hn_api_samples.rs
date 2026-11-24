//! Algolia and Firebase bodies derived from the stored samples in `tests/samples/hn_api/`, shared
//! by the command contract tests and, through the crate root, the in-crate tests. A variant
//! changes only what its test is about, so every other field stays as the sample has it.

use serde_json::{Value, json};

use super::samples::{sample_json, with};

/// The stored search page holding `hits` as page `page` of `pages`.
pub(crate) fn search_page(hits: &[Value], page: u64, pages: u64) -> Value {
    with(
        sample_json("hn_api/search.json"),
        &json!({"hits": hits, "nbHits": hits.len(), "page": page, "nbPages": pages, "hitsPerPage": hits.len()}),
    )
}

/// The stored hit of the 2007 Dropbox launch story, `hn:8863`.
pub(crate) fn story_hit() -> Value {
    sample_json("hn_api/search.json")["hits"][0].clone()
}

/// The stored Algolia tree of the Dropbox story: three top-level comments in ID order, the
/// first with replies listed out of order.
pub(crate) fn discussion() -> Value {
    sample_json("hn_api/item-tree.json")
}

/// The stored Firebase item of the Dropbox story, with `kids` as its ranked replies.
pub(crate) fn story_item(kids: &[u64]) -> Value {
    with(
        sample_json("hn_api/firebase-item.json"),
        &json!({"kids": kids}),
    )
}

/// The stored story list with `ids` in list order.
pub(crate) fn story_list(ids: &[u64]) -> Value {
    let mut list = sample_json("hn_api/firebase-stories.json");
    *list.as_array_mut().expect("story list") = ids.iter().map(|id| json!(id)).collect();
    list
}
