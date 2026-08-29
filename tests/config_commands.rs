mod support;

use std::fs::{self, OpenOptions};
use std::process::{Command, Output, Stdio};

use fs2::FileExt;
use serde_json::Value;
use support::run_command;

#[test]
fn config_list_reports_the_complete_default_effective_view() {
    let config_dir = tempfile::tempdir().expect("create config directory");

    let output = run(config_dir.path(), &["config", "list"], &[], None);
    let view: Value = serde_json::from_slice(&output.stdout).expect("parse config view");

    assert_eq!(
        (
            output.status.code(),
            count_effective_leaves(&view),
            view["search"].get("validation"),
            &view["providers"]["exa"]["keys"],
            &view["http"]["ssl_verify"],
        ),
        (
            Some(0),
            47,
            None,
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
fn integer_file_values_share_the_toml_range_and_report_the_full_path() {
    let cases = [
        ("-1", Some(3)),
        ("0", Some(3)),
        (&i64::MAX.to_string(), Some(0)),
        (
            &(u64::try_from(i64::MAX).expect("positive maximum") + 1).to_string(),
            Some(3),
        ),
    ];

    for (raw, expected_status) in cases {
        let config_dir = tempfile::tempdir().expect("create config directory");
        fs::write(
            config_dir.path().join("config.toml"),
            format!("[providers.exa]\ntimeout = {raw}\n"),
        )
        .expect("write config");

        let output = run(config_dir.path(), &["config", "list"], &[], None);
        assert_eq!(
            output.status.code(),
            expected_status,
            "raw={raw}: {output:?}"
        );
        if expected_status != Some(0) {
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("providers.exa.timeout"),
                "raw={raw}, stderr: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

#[test]
fn negative_file_integer_fails_before_an_environment_override() {
    let config_dir = tempfile::tempdir().expect("create config directory");
    fs::write(
        config_dir.path().join("config.toml"),
        "[providers.exa]\ntimeout = -1\n",
    )
    .expect("write config");

    let output = run(
        config_dir.path(),
        &["config", "list"],
        &[("FORAGER_PROVIDERS__EXA__TIMEOUT", "30")],
        None,
    );

    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("providers.exa.timeout"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn integer_environment_values_share_the_toml_range() {
    let cases = [
        ("-1", Some(3)),
        ("0", Some(3)),
        (&i64::MAX.to_string(), Some(0)),
        (
            &(u64::try_from(i64::MAX).expect("positive maximum") + 1).to_string(),
            Some(3),
        ),
    ];

    for (raw, expected_status) in cases {
        let config_dir = tempfile::tempdir().expect("create config directory");
        let output = run(
            config_dir.path(),
            &["config", "list"],
            &[("FORAGER_PROVIDERS__EXA__TIMEOUT", raw)],
            None,
        );
        assert_eq!(
            output.status.code(),
            expected_status,
            "raw={raw}: {output:?}"
        );
    }
}

#[test]
fn config_set_integer_values_share_the_toml_range() {
    let cases = [
        ("-1", Some(2)),
        ("0", Some(2)),
        (&i64::MAX.to_string(), Some(0)),
        (
            &(u64::try_from(i64::MAX).expect("positive maximum") + 1).to_string(),
            Some(2),
        ),
    ];

    for (raw, expected_status) in cases {
        let config_dir = tempfile::tempdir().expect("create config directory");
        let output = run(
            config_dir.path(),
            &["config", "set", "providers.exa.timeout", raw],
            &[],
            None,
        );
        assert_eq!(
            output.status.code(),
            expected_status,
            "raw={raw}: {output:?}"
        );
    }
}

#[test]
fn zero_is_accepted_for_non_negative_integers_at_all_three_entries() {
    let config_dir = tempfile::tempdir().expect("create config directory");
    fs::write(
        config_dir.path().join("config.toml"),
        "[retry]\nmax_wait = 0\n",
    )
    .expect("write config");
    let file = run(config_dir.path(), &["config", "list"], &[], None);
    let environment = run(
        config_dir.path(),
        &["config", "list"],
        &[("FORAGER_RETRY__MAX_WAIT", "0")],
        None,
    );
    let set = run(
        config_dir.path(),
        &["config", "set", "retry.max_wait", "0"],
        &[],
        None,
    );

    assert_eq!(
        (
            file.status.code(),
            environment.status.code(),
            set.status.code(),
        ),
        (Some(0), Some(0), Some(0))
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
fn config_set_and_unset_edit_inline_tables() {
    let config_dir = tempfile::tempdir().expect("create config directory");
    let path = config_dir.path().join("config.toml");
    fs::write(&path, "retry = { max_attempts = 2, max_wait = 5 }\n").expect("write config");

    let set = run(
        config_dir.path(),
        &["config", "set", "retry.max_attempts", "4"],
        &[],
        None,
    );
    let unset = run(
        config_dir.path(),
        &["config", "unset", "retry.max_wait"],
        &[],
        None,
    );
    let document: toml::Value =
        toml::from_str(&fs::read_to_string(path).expect("read edited inline table"))
            .expect("parse edited inline table");

    assert_eq!(
        (
            set.status.code(),
            unset.status.code(),
            document["retry"]["max_attempts"].as_integer(),
            document["retry"].get("max_wait"),
        ),
        (Some(0), Some(0), Some(4), None)
    );
}

#[test]
fn config_edits_time_out_without_writing_when_the_config_lock_is_held() {
    let config_dir = tempfile::tempdir().expect("create config directory");
    let config_path = config_dir.path().join("config.toml");
    fs::write(&config_path, "[log]\nlevel = \"info\"\n").expect("write config");
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(config_dir.path().join(".config.lock"))
        .expect("open config lock");
    lock.lock_exclusive().expect("hold config lock");

    let set = run(
        config_dir.path(),
        &["config", "set", "log.level", "debug"],
        &[],
        None,
    );
    let unset = run(
        config_dir.path(),
        &["config", "unset", "log.level"],
        &[],
        None,
    );

    for output in [set, unset] {
        assert_eq!(output.status.code(), Some(3), "{output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("config lock timed out"),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(
        fs::read_to_string(config_path).expect("read preserved config"),
        "[log]\nlevel = \"info\"\n"
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
fn config_list_rejects_removed_validation_file_and_environment_inputs() {
    let config_dir = tempfile::tempdir().expect("create config directory");
    fs::write(
        config_dir.path().join("config.toml"),
        "[search]\nvalidation = \"strict\"\n",
    )
    .expect("write config");

    let file_error = run(config_dir.path(), &["config", "list"], &[], None);
    fs::write(config_dir.path().join("config.toml"), "").expect("clear config");
    let env_error = run(
        config_dir.path(),
        &["config", "list"],
        &[("FORAGER_SEARCH__VALIDATION", "strict")],
        None,
    );

    assert_eq!(
        (
            file_error.status.code(),
            String::from_utf8_lossy(&file_error.stderr).contains("validation"),
            env_error.status.code(),
            String::from_utf8_lossy(&env_error.stderr).contains("FORAGER_SEARCH__VALIDATION"),
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
fn config_set_rejects_a_non_finite_retry_multiplier() {
    let config_dir = tempfile::tempdir().expect("create config directory");

    let output = run(
        config_dir.path(),
        &["config", "set", "retry.multiplier", "inf"],
        &[],
        None,
    );

    assert_eq!(
        output.status.code(),
        Some(2),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
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
        ("[search]\nfallback = \"sometimes\"\n", "fallback"),
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
fn capability_orders_reject_duplicate_providers_at_every_input_boundary() {
    let file_dir = tempfile::tempdir().expect("create file config directory");
    fs::write(
        file_dir.path().join("config.toml"),
        "[capabilities.web_fetch]\norder = [\"tavily\", \"tavily\"]\n",
    )
    .expect("write duplicate file order");
    let file = run(file_dir.path(), &["config", "list"], &[], None);

    let env_dir = tempfile::tempdir().expect("create env config directory");
    let environment = run(
        env_dir.path(),
        &["config", "list"],
        &[(
            "FORAGER_CAPABILITIES__WEB_FETCH__ORDER",
            "[\"tavily\",\"tavily\"]",
        )],
        None,
    );

    let set_dir = tempfile::tempdir().expect("create set config directory");
    let set = run(
        set_dir.path(),
        &[
            "config",
            "set",
            "capabilities.web_fetch.order",
            "[\"tavily\",\"tavily\"]",
        ],
        &[],
        None,
    );

    assert_eq!(
        (
            file.status.code(),
            environment.status.code(),
            set.status.code()
        ),
        (Some(3), Some(3), Some(2))
    );
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
fn config_list_rejects_a_non_finite_retry_multiplier() {
    let config_dir = tempfile::tempdir().expect("create config directory");
    fs::write(
        config_dir.path().join("config.toml"),
        "[retry]\nmultiplier = inf\n",
    )
    .expect("write config");

    let output = run(config_dir.path(), &["config", "list"], &[], None);

    assert_eq!(
        output.status.code(),
        Some(3),
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
            config_dir
                .join(".config.lock")
                .metadata()
                .expect("lock metadata")
                .permissions()
                .mode()
                & 0o777,
        ),
        (0o700, 0o600, 0o600)
    );
    let mut entries = fs::read_dir(&config_dir)
        .expect("read config directory")
        .map(|entry| entry.expect("read directory entry").file_name())
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(
        entries,
        [
            std::ffi::OsString::from(".config.lock"),
            std::ffi::OsString::from("config.toml"),
        ]
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
    run_command(&mut command, stdin)
}
