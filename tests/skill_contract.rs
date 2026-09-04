use std::collections::HashSet;
use std::fs;
use std::path::Path;

use forager::types::ResearchPlan;

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
