use std::collections::HashSet;
use std::fs;
use std::path::Path;

use clap::CommandFactory;
use forager::app::Cli;
use forager::types::{Capability, LiveCheckRule, Platform, PlatformRef};

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
fn capability_vocabulary_lists_the_cli_capabilities_in_canonical_order() {
    let source = fs::read_to_string(skill_path("references/capability-vocabulary.json"))
        .expect("read capability vocabulary");
    let vocabulary: serde_json::Value =
        serde_json::from_str(&source).expect("parse capability vocabulary");
    let ids = vocabulary["capabilities"]
        .as_array()
        .expect("capabilities array")
        .iter()
        .map(|entry| entry["id"].as_str().unwrap_or_default())
        .collect::<Vec<_>>();

    assert_eq!(ids, Capability::VOCABULARY.map(Capability::as_str).to_vec());
}

#[test]
fn the_live_check_rules_the_cli_reference_lists_are_the_rules_a_check_reports() {
    let source = fs::read_to_string(skill_path("references/cli.md")).expect("read CLI reference");
    let start = source
        .find("The contract rules are")
        .expect("the CLI reference lists the Live Check rules");
    let list = &source[start..];
    let end = [". ", ".\n"]
        .iter()
        .filter_map(|end| list.find(end))
        .min()
        .expect("the end of the list");
    let list = &list[..end];
    let named = list.split('`').skip(1).step_by(2).collect::<Vec<_>>();

    assert_eq!(
        named,
        LiveCheckRule::VOCABULARY
            .map(LiveCheckRule::as_str)
            .to_vec()
    );
}

#[test]
fn every_relative_link_in_the_skill_resolves_to_a_file() {
    let documents = [
        "SKILL.md",
        "references/platforms.md",
        "references/direct-retrieval.md",
        "references/ordinary-search.md",
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
fn cli_reference_documents_every_command_and_platform_operation() {
    let reference =
        fs::read_to_string(skill_path("references/cli.md")).expect("read CLI reference");

    assert_eq!(
        undocumented_commands(&reference, &Cli::command()),
        Vec::<String>::new()
    );
}

#[test]
fn undocumented_commands_reports_a_missing_command_and_platform_operation() {
    let cli = clap::Command::new("forager")
        .subcommand(clap::Command::new("search"))
        .subcommand(clap::Command::new("exa").subcommand(clap::Command::new("similar")))
        .subcommand(
            clap::Command::new("platform").subcommand(
                clap::Command::new("hn")
                    .subcommand(clap::Command::new("search"))
                    .subcommand(clap::Command::new("stories")),
            ),
        );
    let reference = "### `searching`\n### `exa similar`\n### `platform hn search`\n";

    assert_eq!(
        undocumented_commands(reference, &cli),
        ["search", "platform hn stories"]
    );
}

/// Each top-level command without a `### ` heading naming it or one of its subcommands, and each
/// `platform <id> <op>` without a heading of its own.
fn undocumented_commands(reference: &str, cli: &clap::Command) -> Vec<String> {
    let headings = reference
        .lines()
        .filter_map(|line| line.strip_prefix("### `"))
        .filter_map(|heading| heading.split('`').next())
        .collect::<Vec<_>>();
    let documented = |command: &str| {
        headings.iter().any(|heading| {
            heading
                .strip_prefix(command)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
        })
    };
    let commands = cli
        .get_subcommands()
        .filter(|command| !command.is_hide_set() && command.get_name() != "help")
        .map(|command| command.get_name().to_owned());
    let platform_operations = cli
        .find_subcommand("platform")
        .into_iter()
        .flat_map(clap::Command::get_subcommands)
        .flat_map(|platform| {
            platform.get_subcommands().map(move |operation| {
                format!("platform {} {}", platform.get_name(), operation.get_name())
            })
        });
    commands
        .chain(platform_operations)
        .filter(|command| !documented(command))
        .collect()
}
