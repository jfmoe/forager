mod support;

use std::process::{Command, Output};

use clap::{CommandFactory, Parser};
use forager::app::Cli;
use support::run_command;

#[test]
fn command_tree_aliases_and_exact_subcommands_match_the_contract() {
    let command = Cli::command();
    let commands = command
        .get_subcommands()
        .map(|subcommand| {
            (
                subcommand.get_name(),
                subcommand.get_visible_aliases().collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        commands,
        [
            ("search", vec!["s"]),
            ("research", vec!["rs"]),
            ("fetch", vec!["f"]),
            ("map", vec![]),
            ("anysearch", vec!["as"]),
            ("context7", vec!["c7"]),
            ("exa", vec![]),
            ("platform", vec![]),
            ("config", vec![]),
            ("setup", vec![]),
            ("doctor", vec![]),
            ("smoke", vec![]),
        ]
    );
    let config = command
        .get_subcommands()
        .find(|subcommand| subcommand.get_name() == "config")
        .expect("config command");
    assert_eq!(
        config
            .get_subcommands()
            .map(|subcommand| {
                (
                    subcommand.get_name(),
                    subcommand.get_visible_aliases().collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>(),
        [
            ("path", vec![]),
            ("list", vec!["ls"]),
            ("set", vec![]),
            ("unset", vec![]),
        ]
    );
    assert!(Cli::try_parse_from(["forager", "setu", "--non-interactive"]).is_err());
}

#[test]
fn platform_commands_nest_operations_under_each_platform() {
    let command = Cli::command();
    let platforms = command
        .find_subcommand("platform")
        .expect("platform command")
        .get_subcommands()
        .map(|platform| {
            (
                platform.get_name(),
                platform
                    .get_subcommands()
                    .map(clap::Command::get_name)
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        platforms,
        [
            ("arxiv", vec!["search", "fetch"]),
            ("ssrn", vec!["search", "fetch"])
        ]
    );
}

#[test]
fn arxiv_search_help_lists_every_option_and_value() {
    let output = run(&["platform", "arxiv", "search", "--help"]);
    let help = String::from_utf8(output.stdout).expect("UTF-8 help");

    for expected in [
        "[QUERY]",
        "--category <CATEGORY>",
        "--author <AUTHOR>",
        "--title <TITLE>",
        "--submitted-from <YYYY-MM-DD>",
        "--submitted-to <YYYY-MM-DD>",
        "[possible values: relevance, submitted, updated]",
        "--limit <LIMIT>",
        "--cursor <CURSOR>",
        "--timeout <TIMEOUT>",
        "--verbose",
    ] {
        assert!(
            help.contains(expected),
            "missing {expected} in help:\n{help}"
        );
    }
}

#[test]
fn arxiv_fetch_help_lists_every_option_and_value() {
    let output = run(&["platform", "arxiv", "fetch", "--help"]);
    let help = String::from_utf8(output.stdout).expect("UTF-8 help");

    for expected in [
        "<REFERENCE>",
        "--depth <DEPTH>",
        "[default: full_text]",
        "[possible values: full_text, abstract]",
        "--content-dir <DIR>",
        "[possible values: json, markdown, content]",
        "--timeout <TIMEOUT>",
        "--verbose",
    ] {
        assert!(
            help.contains(expected),
            "missing {expected} in help:\n{help}"
        );
    }
}

#[test]
fn ssrn_search_help_lists_every_option() {
    let output = run(&["platform", "ssrn", "search", "--help"]);
    let help = String::from_utf8(output.stdout).expect("UTF-8 help");

    for expected in [
        "[QUERY]",
        "--limit <LIMIT>",
        "--cursor <CURSOR>",
        "--timeout <TIMEOUT>",
        "--verbose",
    ] {
        assert!(
            help.contains(expected),
            "missing {expected} in help:\n{help}"
        );
    }
}

#[test]
fn ssrn_fetch_help_defaults_to_metadata_depth() {
    let output = run(&["platform", "ssrn", "fetch", "--help"]);
    let help = String::from_utf8(output.stdout).expect("UTF-8 help");

    for expected in [
        "<REFERENCE>",
        "--depth <DEPTH>",
        "[default: metadata]",
        "[possible values: metadata, abstract, full_text]",
        "[possible values: json, markdown, content]",
        "--timeout <TIMEOUT>",
        "--verbose",
    ] {
        assert!(
            help.contains(expected),
            "missing {expected} in help:\n{help}"
        );
    }
}

#[test]
fn research_help_declares_standard_as_the_default_budget() {
    let output = run(&["research", "--help"]);
    let help = String::from_utf8(output.stdout).expect("UTF-8 help");

    assert!(
        help.contains("--budget <BUDGET>")
            && help.contains("[default: standard]")
            && help.contains("[possible values: quick, standard, deep]"),
        "research help:\n{help}"
    );
}

#[test]
fn search_rejects_the_removed_validation_option_as_unknown() {
    let output = run(&["search", "query", "--validation", "strict"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        (output.status.code(), stderr.contains("--validation")),
        (Some(2), true),
        "stderr: {stderr}"
    );
}

fn run(arguments: &[&str]) -> Output {
    let isolated = tempfile::tempdir().expect("isolated environment");
    let mut command = Command::new(env!("CARGO_BIN_EXE_forager"));
    command
        .args(arguments)
        .env_clear()
        .env("HOME", isolated.path())
        .env("XDG_CONFIG_HOME", isolated.path().join("config"))
        .env("XDG_STATE_HOME", isolated.path().join("state"));
    run_command(&mut command, None)
}
