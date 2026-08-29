use std::collections::HashSet;
use std::fs;
use std::path::Path;

use forager::types::ResearchPlan;

#[test]
fn repository_exposes_the_tracked_installable_skills() {
    let skills_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("skills");
    let skill_dirs = fs::read_dir(&skills_dir)
        .expect("read skills directory")
        .map(|entry| {
            entry
                .expect("read skill entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<HashSet<_>>();

    assert_eq!(
        skill_dirs,
        HashSet::from(["forager".to_owned(), "kimi-datasource".to_owned()])
    );
    for path in [
        "skills/forager/SKILL.md",
        "skills/kimi-datasource/SKILL.md",
        "skills/kimi-datasource/scripts/kimi-datasource.mjs",
    ] {
        assert!(
            Path::new(path).is_file(),
            "missing installable skill asset {path}"
        );
    }
}

#[test]
fn research_plan_example_matches_the_current_schema() {
    let source = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("skills/forager/references/research-plan.json"),
    )
    .expect("read research plan example");

    serde_json::from_str::<ResearchPlan>(&source).expect("parse research plan example");
}
