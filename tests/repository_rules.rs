//! Repository rules from `AGENTS.md` that a reviewer would otherwise check by hand.

#[path = "repository_rules/links.rs"]
mod links;
#[path = "repository_rules/retired.rs"]
mod retired;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const AGENTS_LINE_LIMIT: usize = 100;

/// Roots whose Markdown the document checks scan: the current documents, not working material
/// or generated files such as `CHANGELOG.md`.
const SCANNED_ROOTS: &[&str] = &[
    "src",
    "tests",
    "AGENTS.md",
    "GLOSSARY.md",
    "README.md",
    "docs/architecture.md",
    "docs/adr",
    "docs/agents",
];

#[test]
fn agents_md_stays_short() {
    let lines = read(&root().join("AGENTS.md")).lines().count();
    assert!(
        lines <= AGENTS_LINE_LIMIT,
        "AGENTS.md has {lines} lines, limit {AGENTS_LINE_LIMIT}: it is loaded in every session, so move detail to the document that owns it"
    );
}

#[test]
fn docs_holds_only_the_architecture_document_adrs_and_procedures() {
    let docs = root().join("docs");
    let misplaced = misplaced_docs(
        &all_files(&docs)
            .iter()
            .map(|path| relative(path).trim_start_matches("docs/").to_owned())
            .collect::<Vec<_>>(),
    );

    assert!(
        misplaced.is_empty(),
        "docs/ holds only architecture.md and Markdown under adr/ and agents/; put research, live-test records, \
         design drafts and captured material in .local/, and their conclusion in an ADR or an issue: {misplaced:#?}"
    );
}

#[test]
fn material_outside_the_docs_layout_is_reported() {
    let paths = [
        "architecture.md",
        "adr/0002-x.md",
        "agents/release.md",
        ".DS_Store",
        "research/notes.md",
        "adr/capture.json",
    ]
    .map(str::to_owned);
    assert_eq!(
        misplaced_docs(&paths),
        ["research/notes.md", "adr/capture.json"]
    );
}

/// Paths under `docs/`, relative to it, that the layout does not allow. Dotfiles are ignored.
fn misplaced_docs(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .filter(|path| !path.split('/').any(|part| part.starts_with('.')))
        .filter(|path| {
            let placed = path.as_str() == "architecture.md"
                || path.starts_with("adr/")
                || path.starts_with("agents/");
            !(placed
                && Path::new(path)
                    .extension()
                    .is_some_and(|extension| extension == "md"))
        })
        .cloned()
        .collect()
}

#[test]
fn retired_identifiers_appear_only_where_allowed() {
    let found = SCANNED_ROOTS
        .iter()
        .chain(&["skills", ".github"])
        .map(|scanned| root().join(scanned))
        .flat_map(|path| {
            if path.is_dir() {
                all_files(&path)
            } else {
                vec![path]
            }
        })
        .filter(|path| !retired::allowed(&relative(path)))
        .filter_map(|path| {
            let text = fs::read_to_string(&path).ok()?;
            let mentions = retired::mentions(&text);
            (!mentions.is_empty()).then(|| format!("{}: {}", relative(&path), mentions.join(", ")))
        })
        .collect::<Vec<_>>();

    assert!(
        found.is_empty(),
        "these files mention retired identifiers; remove the mention, or, if it is legitimate, add the path to \
         `retired::ALLOWED` with its reason: {found:#?}"
    );
}

#[test]
fn the_library_exposes_only_its_three_facades() {
    assert_eq!(
        public_modules(&read(&root().join("src/lib.rs"))),
        ["app", "config", "types"],
        "the library's public surface is the app, config and types facades; keep other modules private to the crate, \
         and widen the surface only through a spec that decides it"
    );
}

#[test]
fn a_public_module_beyond_the_facades_is_reported() {
    assert_eq!(
        public_modules("pub mod app;\nmod chain;\npub mod catalog;\n"),
        ["app", "catalog"]
    );
}

/// The modules `lib_rs` declares `pub`.
fn public_modules(lib_rs: &str) -> Vec<&str> {
    lib_rs
        .lines()
        .filter_map(|line| line.trim().strip_prefix("pub mod "))
        .map(|rest| rest.trim_end_matches(';').trim())
        .collect()
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

const SPAWN: &str = "Command::new(env!(\"CARGO_BIN_EXE_forager\"))";

#[test]
fn only_the_shared_test_environment_spawns_the_binary() {
    let shared = [
        root().join("tests/support/mod.rs"),
        root().join("tests/repository_rules.rs"),
    ];
    let spawning = files(&root().join("tests"), "rs")
        .into_iter()
        .filter(|path| !shared.contains(path) && read(path).contains(SPAWN))
        .map(|path| relative(&path))
        .collect::<Vec<_>>();

    assert!(
        spawning.is_empty(),
        "spawn the binary through `support::forager_command`, which routes HTTP to a closed port so a test cannot reach a real site: {spawning:#?}"
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

fn all_files(directory: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("read {}: {error}", directory.display()));
    for entry in entries {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            found.extend(all_files(&path));
        } else {
            found.push(path);
        }
    }
    found
}

fn files(directory: &Path, extension: &str) -> Vec<PathBuf> {
    all_files(directory)
        .into_iter()
        .filter(|path| path.extension().is_some_and(|found| found == extension))
        .collect()
}
