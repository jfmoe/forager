//! Doc tests. The checks run the tests through nextest, which does not run doc tests, so an
//! example in a doc comment that rustdoc would compile and run is proven by nothing.

/// Info-string words that leave a fenced block a Rust doc test.
const RUST_ATTRIBUTES: &[&str] = &[
    "rust",
    "ignore",
    "should_panic",
    "no_run",
    "compile_fail",
    "test_harness",
    "standalone_crate",
];

/// The 1-based lines of `source` that open a doc-comment code block rustdoc collects as a doc
/// test, including an `ignore` one, which it lists and never runs: a fenced block whose info
/// string is Rust, or an indented block, which starts after a blank doc line. Only `///` and `//!`
/// comments are read, not `#[doc = "…"]` attributes.
pub(crate) fn doc_test_lines(source: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut open_fence: Option<&str> = None;
    let mut after_blank = true;
    let mut in_indented = false;
    for (index, line) in source.lines().enumerate() {
        let Some(doc) = doc_text(line) else {
            open_fence = None;
            after_blank = true;
            in_indented = false;
            continue;
        };
        let text = doc.strip_prefix(' ').unwrap_or(doc);
        if let Some(fence) = open_fence {
            if text.trim_start().starts_with(fence) {
                open_fence = None;
            }
            continue;
        }
        let indented = text.starts_with("    ") || text.starts_with('\t');
        if indented && !text.trim().is_empty() {
            if after_blank && !in_indented {
                found.push(index + 1);
            }
            in_indented = after_blank || in_indented;
            after_blank = false;
            continue;
        }
        in_indented = false;
        after_blank = text.trim().is_empty();
        let trimmed = text.trim_start();
        let Some(fence) = ["```", "~~~"]
            .into_iter()
            .find(|fence| trimmed.starts_with(fence))
        else {
            continue;
        };
        open_fence = Some(fence);
        if is_rust(trimmed[fence.len()..].trim()) {
            found.push(index + 1);
        }
    }
    found
}

/// The text of a `///` or `//!` doc comment line.
fn doc_text(line: &str) -> Option<&str> {
    let line = line.trim_start();
    if line.starts_with("////") {
        return None;
    }
    line.strip_prefix("///")
        .or_else(|| line.strip_prefix("//!"))
}

fn is_rust(info: &str) -> bool {
    info.split([',', ' ', '\t'])
        .filter(|word| !word.is_empty())
        .all(|word| RUST_ATTRIBUTES.contains(&word) || word.starts_with("edition"))
}
