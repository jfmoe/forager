use std::fs;

use serde_json::{Value, json};

use crate::support::doctor::shallow_config;
use crate::support::{Fixture, RunEnvironment, object_keys};

fn provider<'a>(payload: &'a Value, name: &str) -> &'a Value {
    payload["providers"]
        .as_array()
        .expect("providers")
        .iter()
        .find(|provider| provider["provider"] == name)
        .expect("provider status")
}

#[test]
fn offline_doctor_sends_no_request_and_reports_no_reachability() {
    let fixture = Fixture::start_canary();
    let environment = RunEnvironment::new(&shallow_config(&fixture.url));
    let config_list = environment.run(&["config", "list"]);

    let output = environment.run(&["doctor", "--offline"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");
    let config: Value = serde_json::from_slice(&config_list.stdout).expect("parse config JSON");

    assert_eq!(
        (
            output.status.code(),
            object_keys(&payload),
            &payload["mode"],
            &payload["ok"],
            payload["config"] == config,
            payload["providers"]
                .as_array()
                .expect("providers")
                .iter()
                .any(|provider| provider.get("reachable").is_some()),
        ),
        (
            Some(0),
            vec![
                "config",
                "mode",
                "ok",
                "permission_warnings",
                "providers",
                "state_directories",
            ],
            &json!("offline"),
            &json!(true),
            true,
            false,
        ),
        "{payload}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        fixture.finish_all().is_empty(),
        "offline doctor sent a request"
    );
}

#[cfg(unix)]
#[test]
fn offline_doctor_reports_key_presence_the_browser_preflight_and_writable_state() {
    let stand_in = crate::support::browser::StandIn::failing("must not start");
    let environment = RunEnvironment::new(&format!(
        "[providers.exa]\nkeys = [\"exa-secret\"]\n{}",
        stand_in.xiaohongshu_config("[\"xiaohongshu_browser\"]")
    ));

    let output = environment.run(&["doctor", "--offline"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");
    let xiaohongshu = provider(&payload, "xiaohongshu_browser");

    assert_eq!(
        (
            output.status.code(),
            provider(&payload, "exa"),
            &provider(&payload, "openai")["configured"],
            (
                &xiaohongshu["configured"],
                &xiaohongshu["preflight"],
                &xiaohongshu["message"],
            ),
            &payload["state_directories"],
            stand_in.launches().len(),
        ),
        (
            Some(0),
            &json!({"provider": "exa", "configured": true, "key_count": 1, "source": "file"}),
            &json!(false),
            (
                &json!(true),
                &json!(false),
                &json!(
                    "the xiaohongshu Browser Profile has never been logged in; run `forager browser login xiaohongshu`"
                ),
            ),
            &json!({"state": {"writable": true}, "journal": {"enabled": true, "writable": true}}),
            0,
        ),
        "{payload}"
    );
}

#[cfg(unix)]
#[test]
fn a_permission_warning_leaves_offline_doctor_ok() {
    use std::os::unix::fs::PermissionsExt;

    let environment = RunEnvironment::new("");
    fs::set_permissions(
        environment.config_dir.join("config.toml"),
        fs::Permissions::from_mode(0o644),
    )
    .expect("broaden config permissions");

    let output = environment.run(&["doctor", "--offline"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");

    assert_eq!(
        (
            output.status.code(),
            &payload["ok"],
            payload["permission_warnings"]
                .as_array()
                .expect("permission warnings")
                .iter()
                .any(|warning| warning
                    .as_str()
                    .is_some_and(|text| text.contains("config file"))),
        ),
        (Some(0), &json!(true), true),
        "{payload}"
    );
}

#[test]
fn offline_doctor_passes_a_configuration_without_main_search() {
    let environment = RunEnvironment::new("");

    let output = environment.run(&["doctor", "--offline"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");

    assert_eq!(
        (
            output.status.code(),
            &payload["ok"],
            &provider(&payload, "xai")["configured"],
        ),
        (Some(0), &json!(true), &json!(false)),
        "{payload}"
    );
}

#[test]
fn offline_doctor_fails_when_the_state_directory_is_not_writable() {
    let environment = RunEnvironment::new("");
    fs::write(&environment.state_dir, "not a directory").expect("block the state directory");

    let output = environment.run(&["doctor", "--offline"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");

    assert_eq!(
        (
            output.status.code(),
            &payload["ok"],
            &payload["state_directories"]["state"]["writable"],
            &payload["state_directories"]["journal"]["writable"],
        ),
        (Some(4), &json!(false), &json!(false), &json!(false)),
        "{payload}"
    );
}
