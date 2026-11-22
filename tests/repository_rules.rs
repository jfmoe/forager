//! Repository rules from `AGENTS.md` that a reviewer would otherwise check by hand.

#[path = "repository_rules/links.rs"]
mod links;

use std::collections::BTreeSet;
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
    ("src/capabilities/catalog.rs", 1223),
    ("tests/config_commands.rs", 1122),
];

/// Roots whose Markdown the document checks scan: the current documents, not working material
/// or generated files such as `CHANGELOG.md`.
const SCANNED_ROOTS: &[&str] = &[
    "src",
    "tests",
    "AGENTS.md",
    "GLOSSARY.md",
    "README.md",
    "docs/reference",
    "docs/adr",
    "docs/agents",
];

#[test]
fn every_module_is_named_in_the_architecture_doc() {
    let architecture = normalize(&read(&root().join("docs/reference/architecture.md")));
    let mut unnamed = Vec::new();
    for path in files(&root().join("src"), "rs") {
        let stem = file_name(&path).trim_end_matches(".rs");
        let is_test_module = stem == "tests" || stem.ends_with("_tests");
        let is_entry = matches!(stem, "mod" | "lib" | "main");
        if !is_test_module && !is_entry && !contains_word(&architecture, &normalize(stem)) {
            unnamed.push(relative(&path));
        }
    }

    assert!(
        unnamed.is_empty(),
        "docs/reference/architecture.md does not name these files; record what each owns: {unnamed:#?}"
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

#[test]
fn every_relative_link_in_the_scanned_documents_resolves() {
    let broken = scanned_markdown()
        .into_iter()
        .flat_map(|path| {
            let directory = path.parent().expect("document directory").to_owned();
            links::broken_links(&read(&path), &directory, Path::exists)
                .into_iter()
                .map(move |target| format!("{} -> {target}", relative(&path)))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert!(
        broken.is_empty(),
        "these relative links point nowhere; fix the path, or drop the link if its target was removed on purpose: {broken:#?}"
    );
}

#[test]
fn every_adr_reference_names_an_existing_adr() {
    let existing = files(&root().join("docs/adr"), "md")
        .iter()
        .filter_map(|path| file_name(path).get(..4)?.parse().ok())
        .collect::<BTreeSet<u16>>();
    let rule_test = root().join("tests/repository_rules.rs");
    let missing = scanned_files(&["md", "rs"])
        .into_iter()
        .filter(|path| *path != rule_test)
        .flat_map(|path| {
            missing_adr_references(&read(&path), &existing)
                .into_iter()
                .map(move |reference| format!("{} {reference}", relative(&path)))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert!(
        missing.is_empty(),
        "these ADRs do not exist; point to the ADR that replaced them, or drop the reference: {missing:#?}"
    );
}

#[test]
fn a_reference_to_a_deleted_adr_is_reported_outside_supersedes() {
    let adr = "See ADR-0012 and ADR 0007.\n\n## Supersedes\n\n- ADR-0012, merged here.\n";
    assert_eq!(
        missing_adr_references(adr, &BTreeSet::from([7])),
        ["line 1: ADR-0012"]
    );
}

/// ADR numbers `text` references that are not in `existing`. A reference is `ADR`, an optional
/// `-` or space, and four digits. Under a `Supersedes` heading an ADR may name deleted ADRs.
fn missing_adr_references(text: &str, existing: &BTreeSet<u16>) -> Vec<String> {
    let mut missing = Vec::new();
    let mut in_supersedes = false;
    for (index, line) in text.lines().enumerate() {
        if line.starts_with('#') {
            in_supersedes = line.trim_start_matches('#').trim() == "Supersedes";
        }
        for (start, _) in line.match_indices("ADR") {
            let rest = &line[start + 3..];
            let rest = rest.strip_prefix(['-', ' ']).unwrap_or(rest);
            let Some(number) = rest
                .get(..4)
                .filter(|digits| digits.bytes().all(|byte| byte.is_ascii_digit()))
            else {
                continue;
            };
            if !in_supersedes && !existing.contains(&number.parse().unwrap_or_default()) {
                missing.push(format!("line {}: ADR-{number}", index + 1));
            }
        }
    }
    missing
}

fn scanned_markdown() -> Vec<PathBuf> {
    scanned_files(&["md"])
}

fn scanned_files(extensions: &[&str]) -> Vec<PathBuf> {
    SCANNED_ROOTS
        .iter()
        .map(|scanned| root().join(scanned))
        .flat_map(|path| {
            if path.is_dir() {
                extensions
                    .iter()
                    .flat_map(|extension| files(&path, extension))
                    .collect()
            } else {
                vec![path]
            }
        })
        .collect()
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
