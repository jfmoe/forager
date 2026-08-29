mod support;

use std::process::{Command, Output};

use support::run_command;

#[test]
fn command_tree_and_six_visible_aliases_remain_available() {
    let help = run(&["--help"]);
    let help = String::from_utf8(help.stdout).expect("UTF-8 help");

    for command in [
        "search",
        "research",
        "fetch",
        "map",
        "exa",
        "context7",
        "anysearch",
        "doctor",
        "smoke",
        "config",
        "setup",
    ] {
        assert!(
            help.contains(command),
            "missing top-level command {command}"
        );
    }
    for (alias, arguments) in [
        ("s", &["s", "--help"][..]),
        ("f", &["f", "--help"]),
        ("rs", &["rs", "--help"]),
        ("c7", &["c7", "--help"]),
        ("as", &["as", "--help"]),
        ("ls", &["config", "ls", "--help"]),
    ] {
        let output = run(arguments);
        assert_eq!(
            output.status.code(),
            Some(0),
            "visible alias {alias} failed: {}",
            String::from_utf8_lossy(&output.stderr)
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
