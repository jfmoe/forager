//! Reads upstream samples from the sample store, `tests/samples/<provider>/`, shared by the
//! command contract tests and, through the crate root, the in-crate tests. Paths are relative to
//! the crate root, because the test runner does not fix the working directory.

/// The sample at `path` under `tests/samples`, such as `arxiv_api/feed.xml`.
pub(crate) fn sample(path: &str) -> String {
    let file = format!("{}/tests/samples/{path}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&file).unwrap_or_else(|error| panic!("read sample {file}: {error}"))
}
