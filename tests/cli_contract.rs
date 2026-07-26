use std::process::{Command, Output};

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

fn run(arguments: &[&str]) -> Output {
    let isolated = tempfile::tempdir().expect("isolated environment");
    Command::new(env!("CARGO_BIN_EXE_forager"))
        .args(arguments)
        .env_clear()
        .env("HOME", isolated.path())
        .env("XDG_CONFIG_HOME", isolated.path().join("config"))
        .env("XDG_STATE_HOME", isolated.path().join("state"))
        .output()
        .expect("run forager")
}
