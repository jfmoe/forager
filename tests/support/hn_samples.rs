//! Algolia and Firebase bodies derived from the stored samples in `tests/samples/hn_api/`, shared
//! by the command contract tests and, through the route's in-crate tests, the crate; the
//! including module provides the sample store's reader as `samples`. A variant changes only what
//! its test is about, so every other field stays as the sample has it.

use serde_json::{Value, json};

use super::samples::sample;

/// The stored body `name`, as the upstream answered it.
pub(crate) fn stored(name: &str) -> String {
    sample(&format!("hn_api/{name}"))
}

pub(crate) fn stored_json(name: &str) -> Value {
    serde_json::from_str(&stored(name))
        .unwrap_or_else(|error| panic!("stored hn_api/{name}: {error}"))
}

/// Sets fields of a record.
pub(crate) fn with(mut record: Value, fields: &Value) -> Value {
    for (key, value) in fields.as_object().expect("fields object") {
        record[key] = value.clone();
    }
    record
}

fn without(mut record: Value, fields: &[&str]) -> Value {
    let object = record.as_object_mut().expect("record object");
    for field in fields {
        object.remove(*field);
    }
    record
}

/// The stored search page holding `hits` as page `page` of `pages`.
pub(crate) fn search_page(hits: &[Value], page: u64, pages: u64) -> Value {
    with(
        stored_json("search.json"),
        &json!({"hits": hits, "nbHits": hits.len(), "page": page, "nbPages": pages, "hitsPerPage": hits.len()}),
    )
}

/// The stored hit of the 2007 Dropbox launch story, `hn:8863`.
pub(crate) fn story_hit() -> Value {
    stored_json("search.json")["hits"][0].clone()
}

/// The stored hit of a comment on the Dropbox story, `hn:8952`.
pub(crate) fn comment_hit() -> Value {
    stored_json("search.json")["hits"][1].clone()
}

/// The stored Algolia tree of the Dropbox story: three top-level comments in ID order, the
/// first with replies listed out of order.
pub(crate) fn discussion() -> Value {
    stored_json("item-tree.json")
}

/// The stored tree's first comment, a top-level reply to the story, as comment `id` by
/// `author` saying `text`, posted `id` seconds after the story, with `children`.
pub(crate) fn node(id: u64, author: &str, text: &str, children: &[Value]) -> Value {
    let tree = discussion();
    let posted = tree["created_at_i"].as_u64().expect("story time") + id;
    with(
        tree["children"][0].clone(),
        &json!({"id": id, "author": author, "text": text, "created_at_i": posted, "children": children}),
    )
}

/// The stored Firebase item of the Dropbox story, with `kids` as its ranked replies.
pub(crate) fn story_item(kids: &[u64]) -> Value {
    with(stored_json("firebase-item.json"), &json!({"kids": kids}))
}

/// The stored Firebase item as comment `id` by `by` on `parent` saying `text`, with `kids`.
pub(crate) fn firebase_comment(id: u64, by: &str, parent: u64, text: &str, kids: &[u64]) -> Value {
    with(
        without(
            stored_json("firebase-item.json"),
            &["descendants", "score", "title", "url"],
        ),
        &json!({"id": id, "by": by, "parent": parent, "text": text, "kids": kids, "type": "comment"}),
    )
}

/// The stored story list with `ids` in list order.
pub(crate) fn story_list(ids: &[u64]) -> Value {
    let mut list = stored_json("firebase-stories.json");
    *list.as_array_mut().expect("story list") = ids.iter().map(|id| json!(id)).collect();
    list
}
