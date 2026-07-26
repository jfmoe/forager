use std::fs;
use std::io::Write;
use std::process::{Command, Output, Stdio};

use serde_json::Value;

#[test]
fn config_list_reports_the_complete_default_effective_view() {
    let config_dir = tempfile::tempdir().expect("create config directory");

    let output = run(config_dir.path(), &["config", "list"], &[], None);
    let view: Value = serde_json::from_slice(&output.stdout).expect("parse config view");

    assert_eq!(
        (
            output.status.code(),
            count_effective_leaves(&view),
            &view["search"]["validation"],
            &view["providers"]["exa"]["keys"],
            &view["http"]["ssl_verify"],
        ),
        (
            Some(0),
            48,
            &serde_json::json!({"value": "balanced", "source": "default"}),
            &serde_json::json!({
                "value": [],
                "source": "default",
                "configured": false,
                "key_count": 0
            }),
            &serde_json::json!({"value": true, "source": "default"}),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn count_effective_leaves(value: &Value) -> usize {
    let Some(object) = value.as_object() else {
        return 0;
    };
    if object.contains_key("value") && object.contains_key("source") {
        1
    } else {
        object.values().map(count_effective_leaves).sum()
    }
}

#[test]
fn config_list_applies_file_then_environment_precedence() {
    let config_dir = tempfile::tempdir().expect("create config directory");
    fs::write(
        config_dir.path().join("config.toml"),
        "[providers.exa]\ntimeout = 41\n",
    )
    .expect("write config");

    let output = run(
        config_dir.path(),
        &["config", "list"],
        &[("FORAGER_PROVIDERS__EXA__TIMEOUT", "52")],
        None,
    );
    let view: Value = serde_json::from_slice(&output.stdout).expect("parse config view");

    assert_eq!(
        (
            &view["providers"]["exa"]["timeout"],
            &view["providers"]["exa"]["url"],
        ),
        (
            &serde_json::json!({"value": 52, "source": "env"}),
            &serde_json::json!({"value": "https://api.exa.ai", "source": "default"}),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn config_set_and_unset_preserve_unrelated_toml() {
    let config_dir = tempfile::tempdir().expect("create config directory");
    let path = config_dir.path().join("config.toml");
    fs::write(
        &path,
        "# keep this\n[providers.exa]\nurl = \"https://example.test\" # keep inline\n",
    )
    .expect("write config");

    let set = run(
        config_dir.path(),
        &["config", "set", "providers.exa.timeout", "45"],
        &[],
        None,
    );
    assert_eq!(set.status.code(), Some(0), "{set:?}");
    let unset = run(
        config_dir.path(),
        &["config", "unset", "providers.exa.timeout"],
        &[],
        None,
    );
    assert_eq!(unset.status.code(), Some(0), "{unset:?}");

    assert_eq!(
        fs::read_to_string(path).expect("read config"),
        "# keep this\n[providers.exa]\nurl = \"https://example.test\" # keep inline\n"
    );
}

#[test]
fn config_set_reads_an_array_from_stdin_and_masks_each_key() {
    let config_dir = tempfile::tempdir().expect("create config directory");
    let canary = "credential-canary-do-not-print";

    let set = run(
        config_dir.path(),
        &["config", "set", "providers.exa.keys", "-"],
        &[],
        Some(format!("[\"{canary}\", \"second\"]\r\n").as_bytes()),
    );
    assert_eq!(set.status.code(), Some(0), "{set:?}");

    let list = run(config_dir.path(), &["config", "list"], &[], None);
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&list.stdout),
        String::from_utf8_lossy(&list.stderr)
    );
    let view: Value = serde_json::from_slice(&list.stdout).expect("parse config view");
    assert_eq!(
        view["providers"]["exa"]["keys"],
        serde_json::json!({
            "value": ["********", "********"],
            "source": "file",
            "configured": true,
            "key_count": 2
        })
    );
    assert!(!combined.contains(canary));
}

#[test]
fn config_set_can_repair_a_strictly_invalid_but_parseable_document() {
    let config_dir = tempfile::tempdir().expect("create config directory");
    let path = config_dir.path().join("config.toml");
    fs::write(
        &path,
        "[providers.exa]\ntimeout = 0\n[unknown]\nvalue = true\n",
    )
    .expect("write invalid config");

    let output = run(
        config_dir.path(),
        &["config", "set", "providers.exa.timeout", "30"],
        &[],
        None,
    );

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(
        fs::read_to_string(path)
            .expect("read repaired config")
            .contains("[unknown]")
    );
}

#[test]
fn config_list_rejects_unknown_file_and_environment_keys() {
    let config_dir = tempfile::tempdir().expect("create config directory");
    fs::write(
        config_dir.path().join("config.toml"),
        "[providers.exa]\nunknown = true\n",
    )
    .expect("write config");

    let file_error = run(config_dir.path(), &["config", "list"], &[], None);
    fs::write(config_dir.path().join("config.toml"), "").expect("clear config");
    let env_error = run(
        config_dir.path(),
        &["config", "list"],
        &[("FORAGER_UNKNOWN", "value")],
        None,
    );

    assert_eq!(
        (
            file_error.status.code(),
            String::from_utf8_lossy(&file_error.stderr).contains("unknown"),
            env_error.status.code(),
            String::from_utf8_lossy(&env_error.stderr).contains("FORAGER_UNKNOWN"),
        ),
        (Some(3), true, Some(3), true)
    );
}

#[test]
fn config_set_rejects_invalid_paths_and_values_as_arguments() {
    let config_dir = tempfile::tempdir().expect("create config directory");
    let bad_path = run(
        config_dir.path(),
        &["config", "set", "providers.exa.missing", "1"],
        &[],
        None,
    );
    let bad_value = run(
        config_dir.path(),
        &["config", "set", "providers.exa.timeout", "0"],
        &[],
        None,
    );
    let duplicate_backends = run(
        config_dir.path(),
        &["config", "set", "search.backends", "[\"xai\", \"xai\"]"],
        &[],
        None,
    );

    assert_eq!(
        (
            bad_path.status.code(),
            bad_value.status.code(),
            duplicate_backends.status.code(),
        ),
        (Some(2), Some(2), Some(2))
    );
}

#[test]
fn config_unset_warns_when_the_environment_still_overrides_the_key() {
    let config_dir = tempfile::tempdir().expect("create config directory");
    fs::write(
        config_dir.path().join("config.toml"),
        "[providers.exa]\ntimeout = 41\n",
    )
    .expect("write config");

    let output = run(
        config_dir.path(),
        &["config", "unset", "providers.exa.timeout"],
        &[("FORAGER_PROVIDERS__EXA__TIMEOUT", "52")],
        None,
    );

    assert_eq!(
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).contains("env"),
        ),
        (Some(0), true)
    );
}

#[test]
fn config_list_rejects_invalid_enums_ranges_and_provider_orders_with_locations() {
    for (document, key) in [
        ("[search]\nvalidation = \"slow\"\n", "validation"),
        ("[retry]\nmax_attempts = 0\n", "max_attempts"),
        ("[capabilities.web_fetch]\norder = []\n", "web_fetch"),
        ("[capabilities.web_fetch]\norder = [\"exa\"]\n", "web_fetch"),
    ] {
        let config_dir = tempfile::tempdir().expect("create config directory");
        let path = config_dir.path().join("config.toml");
        fs::write(&path, document).expect("write config");

        let output = run(config_dir.path(), &["config", "list"], &[], None);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(3), "stderr: {stderr}");
        assert!(stderr.contains(&path.display().to_string()), "{stderr}");
        assert!(stderr.contains(key), "{stderr}");
        assert!(stderr.contains("line"), "{stderr}");
    }
}

#[test]
fn config_list_accepts_the_documented_integer_retry_multiplier() {
    let config_dir = tempfile::tempdir().expect("create config directory");
    fs::write(
        config_dir.path().join("config.toml"),
        "[retry]\nmultiplier = 1\n",
    )
    .expect("write config");

    let output = run(config_dir.path(), &["config", "list"], &[], None);
    let view: Value = serde_json::from_slice(&output.stdout).expect("parse config view");

    assert_eq!(
        (output.status.code(), &view["retry"]["multiplier"],),
        (
            Some(0),
            &serde_json::json!({"value": 1.0, "source": "file"}),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn config_list_keeps_string_environment_values_as_strings_and_normalizes_keys() {
    let config_dir = tempfile::tempdir().expect("create config directory");
    let output = run(
        config_dir.path(),
        &["config", "ls"],
        &[
            ("FORAGER_JOURNAL__DIR", "true"),
            (
                "FORAGER_PROVIDERS__EXA__KEYS",
                "[\"\", \"one\", \"one\", \"two\"]",
            ),
        ],
        None,
    );
    let view: Value = serde_json::from_slice(&output.stdout).expect("parse config view");

    assert_eq!(
        (&view["journal"]["dir"], &view["providers"]["exa"]["keys"],),
        (
            &serde_json::json!({"value": "true", "source": "env"}),
            &serde_json::json!({
                "value": ["********", "********"],
                "source": "env",
                "configured": true,
                "key_count": 2
            }),
        )
    );
}

#[test]
fn config_list_redacts_credentials_embedded_in_urls() {
    let config_dir = tempfile::tempdir().expect("create config directory");
    let canary = "credential-canary-do-not-print";
    fs::write(
        config_dir.path().join("config.toml"),
        format!(
            "[providers.exa]\nurl = \"https://user:{canary}@example.test/api?safe=yes&token={canary}#fragment\"\n"
        ),
    )
    .expect("write config");

    let output = run(config_dir.path(), &["config", "list"], &[], None);
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let view: Value = serde_json::from_slice(&output.stdout).expect("parse config view");

    assert_eq!(
        (
            output.status.code(),
            &view["providers"]["exa"]["url"],
            combined.contains(canary),
        ),
        (
            Some(0),
            &serde_json::json!({
                "value": "https://example.test/api?safe=yes&token=********",
                "source": "file"
            }),
            false,
        )
    );
}

#[test]
fn config_list_locates_invalid_values_inside_inline_tables() {
    let config_dir = tempfile::tempdir().expect("create config directory");
    fs::write(
        config_dir.path().join("config.toml"),
        "retry = { max_attempts = 0 }\n",
    )
    .expect("write config");

    let output = run(config_dir.path(), &["config", "list"], &[], None);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        (
            output.status.code(),
            stderr.contains("retry.max_attempts"),
            stderr.contains("line 1, column"),
        ),
        (Some(3), true, true),
        "stderr: {stderr}"
    );
}

#[test]
fn config_set_help_warns_that_argument_values_enter_shell_history() {
    let config_dir = tempfile::tempdir().expect("create config directory");

    let output = run(config_dir.path(), &["config", "set", "--help"], &[], None);

    assert_eq!(
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).contains("shell history"),
        ),
        (Some(0), true)
    );
}

#[test]
fn config_set_refuses_to_overwrite_syntactically_damaged_toml() {
    let config_dir = tempfile::tempdir().expect("create config directory");
    let path = config_dir.path().join("config.toml");
    let damaged = "[providers.exa\nkeys = [\"credential-canary-do-not-print\"]\n";
    fs::write(&path, damaged).expect("write damaged config");

    let output = run(
        config_dir.path(),
        &["config", "set", "providers.exa.timeout", "30"],
        &[],
        None,
    );
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(3), "stderr: {stderr}");
    assert_eq!(fs::read_to_string(&path).expect("read config"), damaged);
    assert!(stderr.contains(&path.display().to_string()), "{stderr}");
    assert!(stderr.contains("line"), "{stderr}");
    assert!(
        !stderr.contains("credential-canary-do-not-print"),
        "{stderr}"
    );
}

#[test]
fn config_list_never_echoes_a_credential_from_a_type_error() {
    let config_dir = tempfile::tempdir().expect("create config directory");
    let canary = "credential-canary-do-not-print";
    fs::write(
        config_dir.path().join("config.toml"),
        format!("[providers.exa]\nkeys = \"{canary}\"\n"),
    )
    .expect("write config");

    let output = run(config_dir.path(), &["config", "list"], &[], None);
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(output.status.code(), Some(3));
    assert!(!combined.contains(canary), "{combined}");
    assert!(combined.contains("providers.exa.keys"), "{combined}");
}

#[cfg(unix)]
#[test]
fn config_set_enforces_private_directory_and_file_modes() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().expect("create root");
    let config_dir = root.path().join("config");
    let output = run(
        &config_dir,
        &["config", "set", "log.level", "debug"],
        &[],
        None,
    );

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        (
            config_dir
                .metadata()
                .expect("directory metadata")
                .permissions()
                .mode()
                & 0o777,
            config_dir
                .join("config.toml")
                .metadata()
                .expect("file metadata")
                .permissions()
                .mode()
                & 0o777,
        ),
        (0o700, 0o600)
    );
    assert_eq!(
        fs::read_dir(&config_dir)
            .expect("read config directory")
            .map(|entry| entry.expect("read directory entry").file_name())
            .collect::<Vec<_>>(),
        [std::ffi::OsString::from("config.toml")]
    );
}

fn run(
    config_dir: &std::path::Path,
    args: &[&str],
    env: &[(&str, &str)],
    stdin: Option<&[u8]>,
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_forager"));
    command
        .args(args)
        .env_clear()
        .env("FORAGER_CONFIG_DIR", config_dir)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in env {
        command.env(key, value);
    }
    let mut child = command.spawn().expect("run forager");
    if let Some(input) = stdin {
        child
            .stdin
            .take()
            .expect("open stdin")
            .write_all(input)
            .expect("write stdin");
    }
    child.wait_with_output().expect("wait for forager")
}
