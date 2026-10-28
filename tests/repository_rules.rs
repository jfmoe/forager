//! Repository rules from `AGENTS.md` that a reviewer would otherwise check by hand.

use std::fs;
use std::path::{Path, PathBuf};

const LINE_LIMIT: usize = 1000;

/// Rust sources already over [`LINE_LIMIT`] and the most lines each may have. A change that
/// raises a ceiling or adds a file states in its commit why the file stays whole.
const OVERSIZED_SOURCES: &[(&str, usize)] = &[
    ("tests/search.rs", 3410),
    ("tests/research.rs", 3020),
    ("src/cli/dispatch.rs", 2105),
    ("src/ops/smoke.rs", 1755),
    ("src/evidence/research.rs", 1426),
    ("src/main.rs", 1336),
    ("src/capabilities/net.rs", 1321),
    ("tests/smoke.rs", 1265),
    ("src/capabilities/catalog.rs", 1213),
    ("tests/config_commands.rs", 1122),
    ("src/infra/config/schema.rs", 1044),
    ("src/infra/config/runtime.rs", 1002),
];

#[test]
fn every_module_and_adapter_file_is_named_in_the_architecture_spec() {
    let spec = read(&root().join("docs/spec/forager/04-architecture.md"));
    let normalized_spec = normalize(&spec);
    let paragraphs = spec.split("\n\n").collect::<Vec<_>>();
    let mut unnamed = Vec::new();
    for path in files(&root().join("src"), "rs") {
        let stem = file_name(&path).trim_end_matches(".rs");
        let is_test_module = stem == "tests" || stem.ends_with("_tests");
        let is_entry = matches!(stem, "mod" | "lib" | "main");
        if !is_test_module && !is_entry && !contains_word(&normalized_spec, &normalize(stem)) {
            unnamed.push(relative(&path));
        }
    }
    for path in files(&root().join("skills/forager/opencli"), "js") {
        let adapter = path.parent().map(file_name).unwrap_or_default();
        let directory = format!("opencli/{adapter}");
        let name = format!("`{}`", file_name(&path));
        let full_path = format!("{directory}/{}", file_name(&path));
        let in_adapter_paragraph = paragraphs.iter().any(|paragraph| {
            paragraph.contains(&full_path)
                || (paragraph.contains(&directory) && paragraph.contains(&name))
        });
        if !in_adapter_paragraph {
            unnamed.push(relative(&path));
        }
    }

    assert!(
        unnamed.is_empty(),
        "docs/spec/forager/04-architecture.md does not name these files; record what each owns: {unnamed:#?}"
    );
}

#[test]
fn rust_sources_stay_within_their_line_ceiling() {
    let mut over = Vec::new();
    for path in files(&root().join("src"), "rs")
        .into_iter()
        .chain(files(&root().join("tests"), "rs"))
    {
        let file = relative(&path);
        let ceiling = OVERSIZED_SOURCES
            .iter()
            .find_map(|(oversized, ceiling)| (*oversized == file).then_some(*ceiling))
            .unwrap_or(LINE_LIMIT);
        let lines = read(&path).lines().count();
        if lines > ceiling {
            over.push(format!("{file}: {lines} lines, ceiling {ceiling}"));
        }
    }

    assert!(
        over.is_empty(),
        "split these files by responsibility, or raise the ceiling in OVERSIZED_SOURCES and say why in the commit: {over:#?}"
    );
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Matches `attempt_log` with "attempt log" and `web_fetch` with "web-fetch".
fn normalize(text: &str) -> String {
    text.to_lowercase().replace(['-', ' '], "_")
}

/// Whether `word` occurs in `text` without letters or digits on either side, so `net` does not
/// match inside "network".
fn contains_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(start, _)| {
        let before = text[..start].chars().next_back();
        let after = text[start + word.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

fn file_name(path: &Path) -> &str {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
}

fn relative(path: &Path) -> String {
    path.strip_prefix(root())
        .unwrap_or(path)
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn files(directory: &Path, extension: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("read {}: {error}", directory.display()));
    for entry in entries {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            found.extend(files(&path, extension));
        } else if path.extension().is_some_and(|found| found == extension) {
            found.push(path);
        }
    }
    found
}
