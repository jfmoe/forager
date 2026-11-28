use std::fs;
use std::path::Path;

use clap::CommandFactory;
use forager::app::{Cli, report_vocabularies};
use forager::types::{Capability, LiveCheckRule, Platform, PlatformRef};

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

/// The names in backticks in the sentence of `source` that starts at `anchor`.
fn named_in_sentence<'a>(source: &'a str, anchor: &str) -> Vec<&'a str> {
    let start = source
        .find(anchor)
        .unwrap_or_else(|| panic!("the CLI reference has `{anchor}`"));
    let sentence = &source[start + anchor.len()..];
    let end = [". ", ".\n"]
        .iter()
        .filter_map(|end| sentence.find(end))
        .min()
        .expect("the end of the sentence");
    sentence[..end].split('`').skip(1).step_by(2).collect()
}

/// The name in backticks that opens each item, at `indent`, of the list that follows `anchor` in
/// `source`, up to the blank line that ends it.
fn named_in_list<'a>(source: &'a str, anchor: &str, indent: &str) -> Vec<&'a str> {
    let start = source
        .find(anchor)
        .unwrap_or_else(|| panic!("the CLI reference has `{anchor}`"));
    let item = format!("{indent}- `");
    source[start..]
        .lines()
        .skip(1)
        .skip_while(|line| line.trim().is_empty())
        .take_while(|line| !line.trim().is_empty())
        .filter_map(|line| line.strip_prefix(item.as_str()))
        .filter_map(|line| line.split('`').next())
        .collect()
}

#[test]
fn the_live_check_rules_the_cli_reference_lists_are_the_rules_a_check_reports() {
    let source = fs::read_to_string(skill_path("references/cli.md")).expect("read CLI reference");

    assert_eq!(
        named_in_sentence(&source, "The contract rules are"),
        LiveCheckRule::VOCABULARY
            .map(LiveCheckRule::as_str)
            .to_vec()
    );
}

#[test]
fn the_report_vocabularies_the_cli_reference_lists_are_the_names_the_reports_use() {
    let source = fs::read_to_string(skill_path("references/cli.md")).expect("read CLI reference");
    let listed = |vocabulary: &str| match vocabulary {
        "health finding" => named_in_list(&source, "has a `finding`:", ""),
        "schedule status" => named_in_sentence(&source, "`age_hours` and `status`:"),
        "live check status" => named_in_list(&source, "`status` is one of:", ""),
        "not started cause" => named_in_list(&source, "and `cause` says why:", "  "),
        "notification action" => named_in_list(&source, "`title` and `body`:", ""),
        other => panic!("no list in the CLI reference for the {other} vocabulary"),
    };

    for (vocabulary, names) in report_vocabularies() {
        assert_eq!(listed(vocabulary), names, "{vocabulary}");
    }
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
