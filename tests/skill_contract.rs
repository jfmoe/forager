use std::collections::HashSet;
use std::fs;
use std::path::Path;

use forager::types::{PlanCapability, ResearchPlan};

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
fn research_plan_example_is_structurally_valid() {
    let plan: ResearchPlan = serde_json::from_str(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("skills/forager/references/research-plan.json"),
        )
        .expect("read research plan example"),
    )
    .expect("parse research plan example");
    let decomposition = plan.decomposition();
    let ids = decomposition
        .iter()
        .map(|subquestion| subquestion.id.as_str())
        .collect::<HashSet<_>>();

    assert_eq!(plan.plan_version(), 1);
    assert_eq!(decomposition.len(), 2);
    assert_eq!(ids.len(), decomposition.len());
    assert!(decomposition.iter().all(|subquestion| {
        !subquestion.id.trim().is_empty()
            && !subquestion.question.trim().is_empty()
            && !subquestion.reason.trim().is_empty()
    }));
    assert_eq!(
        decomposition[0].required_capabilities,
        [PlanCapability::DocsSearch]
    );
    assert_eq!(
        decomposition[1].required_capabilities,
        [PlanCapability::DocsSearch, PlanCapability::WebSearch]
    );
}
