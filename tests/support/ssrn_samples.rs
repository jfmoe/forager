//! Crossref bodies derived from the stored samples in `tests/samples/ssrn_crossref/`, shared by
//! the command contract tests and, through the route's in-crate tests, the crate; the including
//! module provides the sample store's reader as `samples`. A variant changes only what its test
//! is about, so every other field stays as the sample has it.

use serde_json::{Value, json};

use super::samples::sample;

/// The stored body `name`, as the upstream answered it.
pub(crate) fn stored(name: &str) -> String {
    sample(&format!("ssrn_crossref/{name}"))
}

pub(crate) fn stored_json(name: &str) -> Value {
    serde_json::from_str(&stored(name))
        .unwrap_or_else(|error| panic!("stored ssrn_crossref/{name}: {error}"))
}

/// Sets fields of a record.
pub(crate) fn with(mut record: Value, fields: &Value) -> Value {
    for (key, value) in fields.as_object().expect("fields object") {
        record[key] = value.clone();
    }
    record
}

/// The stored `work` body of `ssrn:2042750`, with `fields` set on its record.
pub(crate) fn work_with(fields: &Value) -> Value {
    let mut body = stored_json("work.json");
    body["message"] = with(body["message"].clone(), fields);
    body
}

/// The stored work list holding `items` as a page of a search that matched `total` records.
pub(crate) fn work_list(total: u64, items: &[Value]) -> Value {
    let mut body = stored_json("work-list.json");
    body["message"] = with(
        body["message"].clone(),
        &json!({"total-results": total, "items": items, "items-per-page": items.len()}),
    );
    body
}

/// The stored work list's first record, an SSRN paper with an abstract, as `ssrn:<id>` titled
/// `Paper <id>` with the abstract `Abstract of <id>.`.
pub(crate) fn listed_work(id: &str) -> Value {
    with(
        stored_json("work-list.json")["message"]["items"][0].clone(),
        &json!({
            "DOI": format!("10.2139/ssrn.{id}"),
            "title": [format!("Paper {id}")],
            "abstract": format!("<jats:p>Abstract of {id}.</jats:p>"),
            "resource": {"primary": {"URL": format!("https://www.ssrn.com/abstract={id}")}}
        }),
    )
}
