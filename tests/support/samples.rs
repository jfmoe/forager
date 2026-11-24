//! Reads upstream samples from the sample store, `tests/samples/<provider>/`, shared by the
//! command contract tests and, through the crate root, the in-crate tests. Paths are relative to
//! the crate root, because the test runner does not fix the working directory. Each provider's
//! variants of its samples are in its `<provider>_samples.rs` beside this file.

use serde_json::Value;

/// The sample at `path` under `tests/samples`, such as `arxiv_api/feed.xml`.
pub(crate) fn sample(path: &str) -> String {
    let file = format!("{}/tests/samples/{path}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&file).unwrap_or_else(|error| panic!("read sample {file}: {error}"))
}

/// The JSON sample at `path` under `tests/samples`, such as `exa/search.json`.
pub(crate) fn sample_json(path: &str) -> Value {
    serde_json::from_str(&sample(path))
        .unwrap_or_else(|error| panic!("parse sample {path}: {error}"))
}

/// `record` with each of `fields` set, so every other field stays as the sample has it.
pub(crate) fn with(mut record: Value, fields: &Value) -> Value {
    for (key, value) in fields.as_object().expect("fields object") {
        record[key] = value.clone();
    }
    record
}

/// `record` with each of `fields` removed.
pub(crate) fn without(mut record: Value, fields: &[&str]) -> Value {
    let object = record.as_object_mut().expect("record object");
    for field in fields {
        object.remove(*field);
    }
    record
}
