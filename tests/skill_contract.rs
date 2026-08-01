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
}

#[test]
fn cli_reference_covers_the_public_cli_surface() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let cli = fs::read_to_string(root.join("skills/forager/references/cli.md"))
        .expect("read CLI reference");

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
