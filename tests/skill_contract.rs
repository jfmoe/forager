use std::collections::HashSet;
use std::fs;
use std::path::Path;

use forager::types::{Platform, PlatformRef, ResearchPlan};

#[test]
fn repository_exposes_only_the_named_installable_skill() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let skill_dirs = fs::read_dir(root.join("skills"))
        .expect("read skills directory")
        .map(|entry| {
            entry
                .expect("read skill entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<HashSet<_>>();
    assert_eq!(skill_dirs, HashSet::from(["forager".to_owned()]));

    let source =
        fs::read_to_string(root.join("skills/forager/SKILL.md")).expect("read installable skill");
    let frontmatter = source
        .strip_prefix("---\n")
        .and_then(|source| source.split_once("\n---\n").map(|(yaml, _)| yaml))
        .expect("skill YAML frontmatter");
    let metadata: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(frontmatter).expect("parse skill YAML frontmatter");

    assert_eq!(metadata["name"].as_str(), Some("forager"));
}

#[test]
fn research_plan_example_matches_the_current_schema() {
    let source = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("skills/forager/references/research-plan.json"),
    )
    .expect("read research plan example");

    serde_json::from_str::<ResearchPlan>(&source).expect("parse research plan example");
}

fn skill_path(relative: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("skills/forager")
        .join(relative)
}

fn platform_vocabulary() -> Vec<serde_json::Value> {
    let source = fs::read_to_string(skill_path("references/platform-vocabulary.json"))
        .expect("read platform vocabulary");
    let vocabulary: serde_json::Value =
        serde_json::from_str(&source).expect("parse platform vocabulary");
    vocabulary["platforms"]
        .as_array()
        .expect("platforms array")
        .clone()
}

#[test]
fn platform_vocabulary_lists_every_built_in_platform_with_its_selection_fields() {
    let entries = platform_vocabulary();
    let ids = entries
        .iter()
        .map(|entry| entry["id"].as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    let incomplete = entries
        .iter()
        .filter(|entry| {
            ["purpose", "select_when"].iter().any(|field| {
                entry[field]
                    .as_str()
                    .is_none_or(|text| text.trim().is_empty())
            }) || entry["examples"].as_array().is_none_or(Vec::is_empty)
                || entry["ref_syntax"]["ref"].as_str().is_none()
        })
        .map(|entry| entry["id"].to_string())
        .collect::<Vec<_>>();

    assert_eq!(
        (ids, incomplete),
        (
            Platform::ALL.map(Platform::as_str).to_vec(),
            Vec::<String>::new()
        )
    );
}

#[test]
fn platform_vocabulary_ref_examples_parse_as_refs_of_their_platform() {
    let unparsed = platform_vocabulary()
        .iter()
        .flat_map(|entry| {
            let platform = Platform::ALL
                .into_iter()
                .find(|platform| entry["id"] == platform.as_str())
                .expect("vocabulary id is a built-in platform");
            entry["ref_syntax"]["examples"]
                .as_array()
                .expect("ref examples")
                .iter()
                .map(|example| example.as_str().expect("ref example").to_owned())
                .filter(move |example| PlatformRef::parse(platform, example).is_err())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert_eq!(unparsed, Vec::<String>::new());
}

#[test]
fn every_relative_link_in_the_skill_resolves_to_a_file() {
    let documents = [
        "SKILL.md",
        "references/platforms.md",
        "references/direct-retrieval.md",
        "references/ordinary-search.md",
        "references/research.md",
        "references/cli.md",
    ];
    let broken = documents
        .iter()
        .flat_map(|document| {
            let path = skill_path(document);
            let source = fs::read_to_string(&path).expect("read skill document");
            let directory = path.parent().expect("document directory").to_owned();
            let prose = source.split('`').step_by(2).collect::<Vec<_>>().join(" ");
            prose
                .split("](")
                .skip(1)
                .filter_map(|rest| rest.split(')').next())
                .filter(|target| !target.contains("://") && !target.starts_with('#'))
                .map(|target| target.split('#').next().unwrap_or_default().to_owned())
                .filter(|target| !directory.join(target).is_file())
                .map(|target| format!("{document} -> {target}"))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let skill = fs::read_to_string(skill_path("SKILL.md")).expect("read skill");

    assert_eq!(
        (broken, skill.contains("](references/platforms.md)")),
        (Vec::<String>::new(), true)
    );
}

#[test]
fn cli_reference_documents_every_platform_operation() {
    let reference =
        fs::read_to_string(skill_path("references/cli.md")).expect("read CLI reference");
    let missing = Platform::ALL
        .into_iter()
        .flat_map(|platform| {
            ["search", "fetch"].map(|operation| format!("### `platform {platform} {operation}`"))
        })
        .filter(|heading| !reference.contains(heading.as_str()))
        .collect::<Vec<_>>();

    assert_eq!(missing, Vec::<String>::new());
}
