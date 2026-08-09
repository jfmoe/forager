use std::collections::HashSet;
use std::fs;
use std::path::Path;

use clap::CommandFactory;
use forager::app::Cli;
use forager::types::{PlanCapability, ResearchPlan};
use serde_json::Value;

#[test]
fn repository_exposes_forager_as_an_installable_skill() {
    let skills_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("skills");
    let skill_dirs = fs::read_dir(&skills_dir)
        .expect("read skills directory")
        .map(|entry| entry.expect("read skill entry").file_name())
        .collect::<Vec<_>>();
    let skill = fs::read_to_string(skills_dir.join("forager/SKILL.md"))
        .expect("read installable forager skill");

    assert_eq!(skill_dirs, ["forager"]);
    assert!(skill.starts_with("---\nname: forager\n"));
    assert!(skill.contains("forager >=0.2.0"));
}

#[test]
fn skill_documents_the_context7_library_id_workflow() {
    let skill = normalized_file("skills/forager/references/direct-retrieval.md");

    for required_guidance in [
        "forager context7 library NAME QUERY",
        "`/owner/project[/version]`",
        "reuse the same `library_id`",
        "keep the versioned ID",
        "`library_id` is not a URL",
        "do not pass it to `fetch`",
        "absolute URL",
        "do not invent a source",
    ] {
        assert!(
            skill.contains(required_guidance),
            "Context7 workflow is missing `{required_guidance}`"
        );
    }
}

#[test]
fn skill_routes_each_request_through_the_cost_ladder() {
    let skill = normalized_file("skills/forager/SKILL.md");

    for required_guidance in [
        "direct retrieval → ordinary search → research",
        "cheapest branch that can complete the request",
        "Research is the most expensive branch",
        "references/direct-retrieval.md",
        "references/ordinary-search.md",
        "references/research.md",
    ] {
        assert!(
            skill.contains(required_guidance),
            "cost-ladder contract is missing `{required_guidance}`"
        );
    }
}

#[test]
fn ordinary_search_has_one_bounded_recovery_chain() {
    let ordinary = normalized_file("skills/forager/references/ordinary-search.md");

    for required_guidance in [
        "terminal `timeout` or `network` error",
        "Retry at most once",
        "run `forager exa search` at most once",
        "Fetch at most two URLs",
        "authentication, configuration, or quota error",
        "Diagnose or configure",
        "`source_mode: fallback`",
        "actual rounds",
        "steps completed and the observed failure",
    ] {
        assert!(
            ordinary.contains(required_guidance),
            "ordinary-search recovery contract is missing `{required_guidance}`"
        );
    }
}

#[test]
fn ordinary_search_documents_branch_local_breadth_and_unified_candidates() {
    let ordinary = normalized_file("skills/forager/references/ordinary-search.md");

    for required_guidance in [
        "`0..=20`",
        "Web Search uses 3",
        "Documentation Search and Vertical Search use 1",
        "exact target",
        "`provider`",
        "`capability`",
        "`title`",
        "`url`",
        "`summary`",
        "`provider_data`",
        "typed library locator",
        "Documentation Search or the Research Evidence Pipeline",
        "not verified evidence",
    ] {
        assert!(
            ordinary.contains(required_guidance),
            "ordinary-search candidate contract is missing `{required_guidance}`"
        );
    }

    for stale_guidance in ["`vertical_results`", "`0` and `1` currently match"] {
        assert!(
            !ordinary.contains(stale_guidance),
            "ordinary-search reference retains stale guidance `{stale_guidance}`"
        );
    }
}

#[test]
fn research_orients_before_planning_and_consumes_evidence_by_path() {
    let research = normalized_file("skills/forager/references/research.md");

    for required_guidance in [
        "ordinary search before writing the plan",
        "reuse that existing result",
        "orientation material only",
        "`evidence_items[].path`",
        "`[eN](URL)`",
        "attribution, not semantic verification",
        "`unconsumed_candidates.path`",
        "unverified candidate",
        "`forager exa similar`",
        "re-run neither `forager research` nor main search",
    ] {
        assert!(
            research.contains(required_guidance),
            "research-consumption contract is missing `{required_guidance}`"
        );
    }
}

#[test]
fn research_documents_both_citation_forms_and_failure_recovery() {
    let research = normalized_file("skills/forager/references/research.md");

    for required_guidance in [
        "URL evidence as `[eN](URL)`",
        "documentation evidence as `[eN]`",
        "`evidence_items[].path`",
        "Research Recovery Manifest",
        "`summary_path`",
        "when `summary_path` is not null",
        "completed evidence",
        "gaps",
        "Diagnose or configure",
    ] {
        assert!(
            research.contains(required_guidance),
            "research recovery contract is missing `{required_guidance}`"
        );
    }

    for stale_guidance in [
        "synthesize each supported claim with `[eN](URL)`",
        "every supported claim with `[eN](URL)`",
    ] {
        assert!(
            !research.contains(stale_guidance),
            "research reference retains URL-only evidence guidance `{stale_guidance}`"
        );
    }
}

#[test]
fn cli_reference_documents_the_research_evidence_index() {
    let cli = normalized_file("skills/forager/references/cli.md");

    for required_guidance in [
        "Research Evidence Index",
        "`evidence_items[].path`",
        "`evidence_dir`",
        "`plan_path`",
        "`unconsumed_candidates`",
        "`gap_check`",
        "`capability_gaps`",
        "`synthesis_policy: \"fetch_before_claim\"`",
        "`journal_ref`",
    ] {
        assert!(
            cli.contains(required_guidance),
            "CLI research reference is missing `{required_guidance}`"
        );
    }
}

#[test]
fn cli_reference_matches_the_final_public_contract() {
    let cli = normalized_file("skills/forager/references/cli.md");

    for required_guidance in [
        "`0..=20`",
        "Web Search uses 3",
        "Documentation Search and Vertical Search use 1",
        "`provider`",
        "`capability`",
        "`provider_data`",
        "Research Recovery Manifest",
        "`summary_path`",
        "configuration, stdin, or plan preflight failures",
        "parseable error object on stdout",
        "`log.level=debug`",
        "attempts summary",
        "`trace` adds safe fields for each attempt",
        "configured provider",
        "unreachable",
        "exit code 4",
        "`10..=150`",
        "`1..=5`",
        "`1..=500`",
        "Required by the parser",
    ] {
        assert!(
            cli.contains(required_guidance),
            "CLI final contract is missing `{required_guidance}`"
        );
    }

    for stale_guidance in [
        "--validation",
        "search.validation",
        "`vertical_results`",
        "`0` and `1` currently produce the same target",
        "without a JSON payload",
        "Required at runtime",
        "parser currently displays `[DOMAIN]`",
    ] {
        assert!(
            !cli.contains(stale_guidance),
            "CLI reference retains stale guidance `{stale_guidance}`"
        );
    }
}

#[test]
fn external_integration_guide_only_exposes_context_protection_patterns() {
    let guide = normalized_file("docs/agents/forager-integration.md");

    for required_guidance in [
        "direct retrieval → ordinary search → research",
        "subagent",
        "conclusions and citations",
        "persist bulk evidence",
        "read it on demand",
        "single-page spot check",
    ] {
        assert!(
            guide.contains(required_guidance),
            "external integration guide is missing `{required_guidance}`"
        );
    }

    for internal_detail in [
        "provider configuration",
        "output fields",
        "routing criteria",
    ] {
        assert!(
            !guide.contains(internal_detail),
            "external integration guide exposes `{internal_detail}`"
        );
    }
}

#[test]
fn cli_reference_covers_the_public_cli_surface() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let cli = fs::read_to_string(root.join("skills/forager/references/cli.md"))
        .expect("read CLI reference");
    assert!(cli.contains("forager >=0.2.0"));
    assert!(cli.contains("exact command syntax, non-routine commands, or diagnosis and recovery"));

    let mut clap = Cli::command();
    clap.build();
    assert_visible_aliases_documented(&clap, &mut Vec::new(), &cli);
    let documented_commands: &[(&[&str], &str)] = &[
        (&["search"], "search"),
        (&["research"], "research"),
        (&["fetch"], "fetch"),
        (&["map"], "map"),
        (&["exa", "search"], "exa search"),
        (&["exa", "similar"], "exa similar"),
        (&["context7", "library"], "context7 library"),
        (&["context7", "docs"], "context7 docs"),
        (&["anysearch", "search"], "anysearch search"),
        (&["anysearch", "domains"], "anysearch domains"),
        (&["config", "path"], "config"),
        (&["config", "list"], "config"),
        (&["config", "set"], "config"),
        (&["config", "unset"], "config"),
        (&["setup"], "setup"),
        (&["doctor"], "doctor"),
        (&["smoke"], "smoke"),
    ];
    let expected_paths = documented_commands
        .iter()
        .map(|(path, _)| path.join(" "))
        .collect::<HashSet<_>>();
    let mut actual_paths = HashSet::new();
    collect_public_leaf_paths(&clap, &mut Vec::new(), &mut actual_paths);
    assert_eq!(actual_paths, expected_paths);

    for (path, heading) in documented_commands {
        let command = find_clap_command(&clap, path);
        let section = markdown_section(&cli, heading);
        let invocation = format!("forager {}", path.join(" "));
        assert!(
            section.contains(&invocation),
            "CLI reference section `{heading}` is missing `{invocation}`"
        );

        for argument in command
            .get_arguments()
            .filter(|argument| !argument.is_hide_set() && argument.get_id() != "help")
        {
            if let Some(long) = argument.get_long() {
                assert!(
                    section.contains(&format!("--{long}")),
                    "CLI reference section `{heading}` is missing `--{long}`"
                );
            } else if argument.get_index().is_some() {
                let value_names = argument.get_value_names().map_or_else(
                    || vec![argument.get_id().to_string().to_uppercase()],
                    |names| names.iter().map(ToString::to_string).collect::<Vec<_>>(),
                );
                for value_name in value_names {
                    assert!(
                        section.contains(&value_name),
                        "CLI reference section `{heading}` is missing positional `{value_name}`"
                    );
                }
            }

            let is_switch = matches!(
                argument.get_action(),
                clap::ArgAction::SetTrue | clap::ArgAction::SetFalse
            );
            if !is_switch {
                for default in argument.get_default_values() {
                    let default = default.to_string_lossy();
                    if !default.is_empty() {
                        assert!(
                            section.contains(default.as_ref()),
                            "CLI reference section `{heading}` is missing default `{default}`"
                        );
                    }
                }
            }

            if !is_switch
                && let Some(possible_values) = argument.get_value_parser().possible_values()
            {
                for possible_value in possible_values.filter(|value| !value.is_hide_set()) {
                    assert!(
                        section.contains(possible_value.get_name()),
                        "CLI reference section `{heading}` is missing value `{}`",
                        possible_value.get_name()
                    );
                }
            }
        }
    }

    let exit_codes = cli
        .split_once("## Exit codes")
        .expect("CLI exit-code section")
        .1;
    for code in ["`0`", "`2`", "`3`", "`4`", "`5`", "`101`"] {
        assert!(exit_codes.contains(code), "CLI reference is missing {code}");
    }
}

#[test]
fn research_plan_example_is_structurally_valid_without_freezing_its_prose() {
    let plan: ResearchPlan = serde_json::from_str(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("skills/forager/references/research-plan.json"),
        )
        .expect("read research plan example"),
    )
    .expect("parse research plan example");
    let ids = plan
        .decomposition
        .iter()
        .map(|subquestion| subquestion.id.as_str())
        .collect::<HashSet<_>>();

    assert_eq!(plan.plan_version, 1);
    assert_eq!(plan.decomposition.len(), 2);
    assert_eq!(ids.len(), plan.decomposition.len());
    assert!(plan.decomposition.iter().all(|subquestion| {
        !subquestion.id.trim().is_empty()
            && !subquestion.question.trim().is_empty()
            && !subquestion.reason.trim().is_empty()
    }));
    assert_eq!(
        plan.decomposition[0].required_capabilities,
        [PlanCapability::DocsSearch]
    );
    assert_eq!(
        plan.decomposition[1].required_capabilities,
        [PlanCapability::DocsSearch, PlanCapability::WebSearch]
    );
}

#[test]
fn classifier_uses_the_installable_capability_vocabulary_asset() {
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

    assert_eq!(
        ids,
        ["docs_search", "web_search", "web_fetch", "vertical_search"]
    );
    assert!(
        classifier
            .contains("include_str!(\"../skills/forager/references/capability-vocabulary.json\")")
    );
    assert!(!root.join("assets/capability-vocabulary.json").exists());
}

fn collect_public_leaf_paths(
    command: &clap::Command,
    prefix: &mut Vec<String>,
    paths: &mut HashSet<String>,
) {
    for subcommand in command
        .get_subcommands()
        .filter(|subcommand| !subcommand.is_hide_set() && subcommand.get_name() != "help")
    {
        prefix.push(subcommand.get_name().to_owned());
        if subcommand
            .get_subcommands()
            .all(|child| child.is_hide_set() || child.get_name() == "help")
        {
            paths.insert(prefix.join(" "));
        } else {
            collect_public_leaf_paths(subcommand, prefix, paths);
        }
        prefix.pop();
    }
}

fn find_clap_command<'a>(command: &'a clap::Command, path: &[&str]) -> &'a clap::Command {
    path.iter().fold(command, |parent, name| {
        parent
            .get_subcommands()
            .find(|subcommand| subcommand.get_name() == *name)
            .unwrap_or_else(|| panic!("missing Clap command `{}`", path.join(" ")))
    })
}

fn assert_visible_aliases_documented(
    command: &clap::Command,
    prefix: &mut Vec<String>,
    reference: &str,
) {
    for subcommand in command
        .get_subcommands()
        .filter(|subcommand| !subcommand.is_hide_set() && subcommand.get_name() != "help")
    {
        prefix.push(subcommand.get_name().to_owned());
        for alias in subcommand.get_visible_aliases() {
            let alias_path = if prefix.len() == 1 {
                alias.to_owned()
            } else {
                format!("{} {alias}", prefix[..prefix.len() - 1].join(" "))
            };
            assert!(
                reference.contains(&format!("`{alias_path}`")),
                "CLI reference is missing visible alias `{alias_path}`"
            );
        }
        assert_visible_aliases_documented(subcommand, prefix, reference);
        prefix.pop();
    }
}

fn markdown_section<'a>(markdown: &'a str, heading: &str) -> &'a str {
    let marker = format!("### `{heading}`");
    let start = markdown
        .find(&marker)
        .unwrap_or_else(|| panic!("missing CLI reference heading `{heading}`"));
    let body = &markdown[start + marker.len()..];
    let end = [body.find("\n### "), body.find("\n## ")]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(body.len());
    &body[..end]
}

fn normalized_file(path: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
        .unwrap_or_else(|error| panic!("read `{path}`: {error}"))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
