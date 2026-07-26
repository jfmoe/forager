use std::fs;
use std::path::Path;

use forager::types::{
    ClaimRisk, EvidenceStrength, PlanCapability, RecencyRequirement, ResearchIntentSignals,
    ResearchPlan, ResearchSubquestion,
};
use serde_json::Value;

#[test]
fn repository_exposes_forager_as_an_installable_skill_with_a_minimum_cli_version() {
    let skills_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("skills");
    let skill_dirs = fs::read_dir(&skills_dir)
        .expect("read skills directory")
        .map(|entry| entry.expect("read skill entry").file_name())
        .collect::<Vec<_>>();
    let skill = fs::read_to_string(skills_dir.join("forager/SKILL.md"))
        .expect("read installable forager skill");

    assert_eq!(skill_dirs, ["forager"]);
    assert!(skill.starts_with("---\nname: forager\n"));
    assert!(skill.contains("forager: \">=0.1.0\""));
    assert!(!skill.contains("forager: \"=0.1.0\""));
}

#[test]
fn skill_guides_caller_declarations_and_schema_v1_research_plans() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let skill =
        fs::read_to_string(root.join("skills/forager/SKILL.md")).expect("read forager skill");
    let plan: ResearchPlan = serde_json::from_str(
        &fs::read_to_string(root.join("skills/forager/references/research-plan.json"))
            .expect("read research plan example"),
    )
    .expect("parse research plan example");

    assert!(skill.contains("forager search \"QUERY\" --capabilities CAPABILITIES"));
    assert!(skill.contains("--capabilities none --format json"));
    assert!(skill.contains("forager research \"QUERY\" --plan plan.json"));
    assert_eq!(
        plan,
        ResearchPlan {
            plan_version: 1,
            intent_signals: ResearchIntentSignals {
                recency_requirement: RecencyRequirement::Recent,
                docs_api_intent: true,
                source_authority_need: EvidenceStrength::High,
                claim_risk: ClaimRisk::Medium,
                cross_validation_need: EvidenceStrength::High,
            },
            decomposition: vec![ResearchSubquestion {
                id: "sq1".into(),
                question: "Which official documentation defines the current contract?".into(),
                reason: "Establish the authoritative baseline before comparing recent changes."
                    .into(),
                required_capabilities: vec![PlanCapability::DocsSearch, PlanCapability::WebSearch,],
            }],
        }
    );
}

#[test]
fn skill_and_classifier_share_the_capability_vocabulary_asset() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let vocabulary_path = root.join("skills/forager/references/capability-vocabulary.json");
    let vocabulary: Value = serde_json::from_str(
        &fs::read_to_string(&vocabulary_path).expect("read shared capability vocabulary"),
    )
    .expect("parse shared capability vocabulary");
    let ids = vocabulary["capabilities"]
        .as_array()
        .expect("capability entries")
        .iter()
        .map(|entry| entry["id"].as_str().expect("capability id"))
        .collect::<Vec<_>>();
    let classifier = fs::read_to_string(root.join("src/classifier.rs")).expect("read classifier");
    let skill =
        fs::read_to_string(root.join("skills/forager/SKILL.md")).expect("read forager skill");

    assert_eq!(
        ids,
        ["docs_search", "web_search", "web_fetch", "vertical_search"]
    );
    assert!(
        classifier
            .contains("include_str!(\"../skills/forager/references/capability-vocabulary.json\")")
    );
    assert!(skill.contains("references/capability-vocabulary.json"));
    assert!(!root.join("assets/capability-vocabulary.json").exists());
}

#[test]
fn migration_removes_the_legacy_skill_before_installing_forager() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let migration = fs::read_to_string(root.join("skills/forager/references/migration.md"))
        .expect("read skill migration guide");

    assert!(migration.contains("smart-search-cli` 是应删除的残留"));
    assert!(migration.contains("先删除旧 Skill，再安装新 Skill"));
    assert!(migration.contains("npx skills add jfmoe/forager"));
}

#[test]
fn installable_skill_excludes_retired_commands_plans_and_sync_workflows() {
    let skill_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("skills/forager");
    let mut corpus = String::new();
    append_files(&skill_root, &mut corpus);

    for retired in [
        "smart-search ",
        "forager deep",
        "forager route",
        "forager skills ",
        "steps.command",
        "\"steps\"",
        "capability_plan",
        "known_url",
        "locale_domain_scope",
        "Automatic Skill Sync",
    ] {
        assert!(
            !corpus.contains(retired),
            "installable Skill retained obsolete contract {retired}"
        );
    }
}

fn append_files(path: &Path, corpus: &mut String) {
    for entry in fs::read_dir(path).expect("read installable skill directory") {
        let path = entry.expect("read installable skill entry").path();
        if path.is_dir() {
            append_files(&path, corpus);
        } else {
            corpus.push_str(&fs::read_to_string(path).expect("read installable skill file"));
        }
    }
}
