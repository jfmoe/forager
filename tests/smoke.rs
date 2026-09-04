mod support;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use support::{Fixture, Response, run_command};

struct SmokeEnvironment {
    _root: tempfile::TempDir,
    config_dir: PathBuf,
    state_dir: PathBuf,
    home_dir: PathBuf,
    journal_dir: PathBuf,
}

impl SmokeEnvironment {
    fn new(config: impl FnOnce(&Path) -> String) -> Self {
        let root = tempfile::tempdir().expect("create isolated root");
        let config_home = root.path().join("xdg-config");
        let config_dir = config_home.join("forager");
        let state_dir = root.path().join("xdg-state");
        let home_dir = root.path().join("home");
        let journal_dir = root.path().join("journal");
        fs::create_dir_all(&config_dir).expect("create config directory");
        fs::create_dir_all(&home_dir).expect("create home directory");
        forager::config::ensure_private_directory(&config_dir)
            .expect("restrict config directory permissions");
        let mut config_file = forager::config::create_private_file(&config_dir.join("config.toml"))
            .expect("create private config file");
        config_file
            .write_all(config(&journal_dir).as_bytes())
            .expect("write config");
        Self {
            _root: root,
            config_dir,
            state_dir,
            home_dir,
            journal_dir,
        }
    }

    fn run(&self, arguments: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_forager"));
        command
            .args(arguments)
            .env_clear()
            .env(
                "XDG_CONFIG_HOME",
                self.config_dir.parent().expect("XDG config home"),
            )
            .env("XDG_STATE_HOME", &self.state_dir)
            .env("HOME", &self.home_dir)
            .env("NO_PROXY", "127.0.0.1,localhost");
        #[cfg(windows)]
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", system_root);
        }
        run_command(&mut command, None)
    }
}

#[test]
fn offline_smoke_reports_local_readiness_without_contacting_provider_endpoints() {
    let fixture = Fixture::start_canary();
    let environment =
        SmokeEnvironment::new(|journal_dir| complete_config(&fixture.url, journal_dir));

    let output = environment.run(&["smoke"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse smoke JSON");
    let network_was_contacted = !fixture.finish_all().is_empty();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(
        (
            output.status.code(),
            &payload["mode"],
            &payload["ok"],
            &payload["registry"],
            payload["providers"].as_array().map(Vec::len),
            &payload["providers"][0]["keys"],
            &payload["classifier"]["keys"],
            &payload["journal"]["writable"],
            &payload["credential_cursor"]["writable"],
            &payload["permissions"]["ok"],
            network_was_contacted,
        ),
        (
            Some(0),
            &Value::String("offline".into()),
            &Value::Bool(true),
            &json!({"ok": true, "provider_count": 8}),
            Some(8),
            &json!(["********"]),
            &json!(["********"]),
            &Value::Bool(true),
            &Value::Bool(true),
            &Value::Bool(true),
            false,
        ),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for secret in provider_secrets() {
        assert!(!combined.contains(secret), "smoke leaked {secret}");
    }
}

#[test]
fn live_smoke_lists_exactly_the_specification_case_registry_without_l0_doctor_gates() {
    let endpoint = "http://127.0.0.1:9";
    let environment = SmokeEnvironment::new(|journal_dir| minimal_config(endpoint, journal_dir));

    let output = environment.run(&["smoke", "--live", "--list"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse live registry JSON");
    let expected = json!([
        "P1", "P2", "C01", "C02", "C03", "C04", "C05", "C06", "C07", "C08", "C09", "C10", "C11",
        "C12", "C13", "C14", "C15", "C16", "C17"
    ]);

    assert_eq!(
        (
            output.status.code(),
            &payload["mode"],
            &payload["registered_case_ids"],
            &payload["specification_case_ids"],
        ),
        (
            Some(0),
            &Value::String("live_registry".into()),
            &expected,
            &expected,
        ),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(payload["cases"].as_array().is_some_and(|cases| {
        cases
            .iter()
            .all(|case| case["id"].as_str().is_some_and(|id| !id.starts_with("L0")))
    }));
}

#[test]
fn live_smoke_retries_configured_cases_and_distinguishes_failure_deferral_and_unconfigured_cases() {
    let endpoint = "http://127.0.0.1:9";
    let environment = SmokeEnvironment::new(|journal_dir| minimal_config(endpoint, journal_dir));

    let failed = environment.run(&["smoke", "--live", "--timeout", "5"]);
    let failed_payload: Value =
        serde_json::from_slice(&failed.stdout).expect("parse failed live smoke JSON");
    let failed_c01 = case(&failed_payload, "C01");

    assert_eq!(
        (
            failed.status.code(),
            &failed_payload["mode"],
            &failed_payload["ok"],
            &failed_payload["summary"],
            &failed_c01["status"],
            &failed_c01["attempts"],
            &case(&failed_payload, "C02")["status"],
            &case(&failed_payload, "P1")["status"],
        ),
        (
            Some(4),
            &Value::String("live".into()),
            &Value::Bool(false),
            &json!({"passed": 0, "failed": 1, "deferred": 0, "unconfigured": 18}),
            &Value::String("failed".into()),
            &Value::Number(3.into()),
            &Value::String("unconfigured".into()),
            &Value::String("unconfigured".into()),
        ),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&failed.stdout),
        String::from_utf8_lossy(&failed.stderr)
    );

    let outage_evidence = format!("C01={endpoint}?token=outage-secret");
    let deferred = environment.run(&[
        "smoke",
        "--live",
        "--timeout",
        "15",
        "--outage-evidence",
        &outage_evidence,
    ]);
    let deferred_payload: Value =
        serde_json::from_slice(&deferred.stdout).expect("parse deferred live smoke JSON");
    let deferred_c01 = case(&deferred_payload, "C01");

    assert_eq!(
        (
            deferred.status.code(),
            &deferred_payload["ok"],
            &deferred_payload["summary"],
            &deferred_c01["status"],
            &deferred_c01["attempts"],
            &deferred_c01["outage_evidence"],
            deferred_c01["checked_at_unix_seconds"].as_u64().is_some(),
        ),
        (
            Some(4),
            &Value::Bool(false),
            &json!({"passed": 0, "failed": 0, "deferred": 1, "unconfigured": 18}),
            &Value::String("deferred".into()),
            &Value::Number(3.into()),
            &Value::String("http://127.0.0.1:9?token=********".into()),
            true,
        ),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&deferred.stdout),
        String::from_utf8_lossy(&deferred.stderr)
    );
    assert!(!String::from_utf8_lossy(&deferred.stdout).contains("outage-secret"));
}

#[test]
fn independent_outage_probe_reports_a_failed_same_endpoint_request() {
    let endpoint = "http://127.0.0.1:9";
    let environment = SmokeEnvironment::new(|journal_dir| minimal_config(endpoint, journal_dir));

    let output = environment.run(&[
        "smoke",
        "--probe",
        "OUTAGE",
        "--probe-url",
        endpoint,
        "--probe-timeout",
        "1",
    ]);

    let payload = serde_json::from_slice::<Value>(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "parse outage probe JSON: {error}; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert_eq!(
        (output.status.code(), payload,),
        (Some(4), json!({"outage": true})),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn internal_smoke_probe_does_not_add_a_top_level_command() {
    let mut command = Command::new(env!("CARGO_BIN_EXE_forager"));
    command.arg("__smoke-probe");
    let output = run_command(&mut command, None);

    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn live_smoke_passes_a_configured_case_only_after_a_zero_parseable_nonempty_terminal() {
    let response_body = format!(
        "data: {}\n\n",
        json!({
            "type": "response.completed",
            "response": {
                "output": [{
                    "content": [{
                        "type": "output_text",
                        "text": "Rust stable release",
                        "annotations": [{
                            "type": "url_citation",
                            "url": "https://example.test/rust",
                            "title": "Rust"
                        }]
                    }]
                }]
            }
        })
    );
    let fixture = Fixture::start(200, "text/event-stream", &response_body);
    let endpoint = format!("{}/v1", fixture.url);
    let environment = SmokeEnvironment::new(|journal_dir| minimal_config(&endpoint, journal_dir));

    let output = environment.run(&["smoke", "--live", "--timeout", "2"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse live smoke JSON");

    assert_eq!(
        (
            output.status.code(),
            &payload["summary"],
            &case(&payload, "C01")["status"],
            &case(&payload, "C01")["attempts"],
        ),
        (
            Some(4),
            &json!({"passed": 1, "failed": 0, "deferred": 0, "unconfigured": 18}),
            &Value::String("passed".into()),
            &Value::Number(1.into()),
        ),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.finish();
}

#[test]
fn live_smoke_drains_large_child_output_without_false_timeout() {
    let response_body = format!(
        "data: {}\n\n",
        json!({
            "type": "response.completed",
            "response": {
                "output": [{
                    "content": [{
                        "type": "output_text",
                        "text": "x".repeat(256 * 1024),
                        "annotations": [{
                            "type": "url_citation",
                            "url": "https://example.test/rust",
                            "title": "Rust"
                        }]
                    }]
                }]
            }
        })
    );
    let fixture = Fixture::start(200, "text/event-stream", &response_body);
    let endpoint = format!("{}/v1", fixture.url);
    let environment = SmokeEnvironment::new(|journal_dir| minimal_config(&endpoint, journal_dir));

    let output = environment.run(&["smoke", "--live", "--timeout", "3"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse live smoke JSON");

    assert_eq!(
        (
            output.status.code(),
            &case(&payload, "C01")["status"],
            &case(&payload, "C01")["attempts"],
        ),
        (
            Some(4),
            &Value::String("passed".into()),
            &Value::Number(1.into()),
        ),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.finish();
}

#[test]
fn live_smoke_enforces_one_hard_deadline_across_retries() {
    let fixture = Fixture::start_sequence(vec![
        Response::json(200, "").with_delay(Duration::from_secs(3)),
    ]);
    let endpoint = format!("{}/v1", fixture.url);
    let environment = SmokeEnvironment::new(|journal_dir| minimal_config(&endpoint, journal_dir));

    let started = Instant::now();
    let output = environment.run(&["smoke", "--live", "--timeout", "1"]);
    let elapsed = started.elapsed();
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse live smoke JSON");

    assert_eq!(
        (
            output.status.code(),
            &case(&payload, "C01")["status"],
            &case(&payload, "C01")["attempts"],
        ),
        (
            Some(4),
            &Value::String("failed".into()),
            &Value::Number(1.into()),
        ),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        elapsed < Duration::from_millis(2500),
        "live smoke exceeded its hard deadline: {elapsed:?}"
    );
    fixture.finish();
}

#[test]
fn live_smoke_p2_does_not_write_evidence_into_the_configured_journal() {
    let classifier_response = json!({
        "choices": [{
            "message": {
                "content": json!({
                    "plan_version": 1,
                    "intent_signals": {
                        "recency_requirement": "none",
                        "docs_api_intent": false,
                        "source_authority_need": "normal",
                        "claim_risk": "medium",
                        "cross_validation_need": "normal"
                    },
                    "decomposition": [{
                        "id": "sq1",
                        "question": "What evidence is available?",
                        "reason": "Gather relevant evidence",
                        "required_capabilities": []
                    }]
                })
                .to_string()
            }
        }]
    })
    .to_string();
    let classifier = Fixture::start_sequence(
        (0..9)
            .map(|_| Response::json(200, &classifier_response))
            .collect(),
    );
    let environment = SmokeEnvironment::new(|journal_dir| p2_config(&classifier.url, journal_dir));

    let output = environment.run(&["smoke", "--live", "--timeout", "3"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse live smoke JSON");

    assert_eq!(
        (
            &case(&payload, "P2")["status"],
            &case(&payload, "P2")["attempts"]
        ),
        (&Value::String("failed".into()), &Value::Number(3.into())),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !environment
            .journal_dir
            .join("live-smoke-p2-evidence")
            .exists(),
        "P2 smoke evidence polluted the configured journal"
    );
    classifier.finish_all();
}

#[test]
fn offline_smoke_returns_config_error_for_invalid_configuration() {
    let secret = "invalid-config-secret";
    let environment = SmokeEnvironment::new(|_| {
        format!("[providers.xai]\nkeys = [\"{secret}\"]\nunknown = true\n")
    });

    let output = environment.run(&["smoke"]);
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(output.status.code(), Some(3), "{combined}");
    assert!(output.stdout.is_empty(), "{combined}");
    assert!(combined.contains("config_error"), "{combined}");
    assert!(!combined.contains(secret), "smoke leaked {secret}");
}

fn case<'a>(payload: &'a Value, id: &str) -> &'a Value {
    payload["cases"]
        .as_array()
        .expect("live cases")
        .iter()
        .find(|case| case["id"] == id)
        .expect("registered live case")
}

#[test]
fn offline_smoke_returns_config_error_when_no_main_search_credential_is_present() {
    let environment = SmokeEnvironment::new(|journal_dir| {
        format!("[providers.exa]\nkeys = [\"exa-secret\"]\n[journal]\ndir = {journal_dir:?}\n")
    });

    let output = environment.run(&["smoke"]);

    assert_eq!(output.status.code(), Some(3));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("search.backends has no configured credentials")
    );
}

#[test]
fn offline_smoke_reports_journal_write_failure_as_a_stable_local_terminal() {
    let endpoint = "http://127.0.0.1:9";
    let environment = SmokeEnvironment::new(|journal_dir| minimal_config(endpoint, journal_dir));
    fs::write(&environment.journal_dir, "not a directory").expect("block journal directory");

    let output = environment.run(&["smoke"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse smoke JSON");

    assert_eq!(
        (
            output.status.code(),
            &payload["ok"],
            &payload["journal"]["writable"],
        ),
        (Some(4), &Value::Bool(false), &Value::Bool(false)),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn offline_smoke_reports_credential_cursor_write_failure_as_a_stable_local_terminal() {
    let endpoint = "http://127.0.0.1:9";
    let environment = SmokeEnvironment::new(|journal_dir| minimal_config(endpoint, journal_dir));
    fs::write(&environment.state_dir, "not a directory").expect("block credential state");

    let output = environment.run(&["smoke"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse smoke JSON");

    assert_eq!(
        (
            output.status.code(),
            &payload["ok"],
            &payload["credential_cursor"]["writable"],
        ),
        (Some(4), &Value::Bool(false), &Value::Bool(false)),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn offline_smoke_rejects_overly_broad_configuration_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let endpoint = "http://127.0.0.1:9";
    let environment = SmokeEnvironment::new(|journal_dir| minimal_config(endpoint, journal_dir));
    fs::set_permissions(
        environment.config_dir.join("config.toml"),
        fs::Permissions::from_mode(0o644),
    )
    .expect("broaden config permissions");

    let output = environment.run(&["smoke"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse smoke JSON");

    assert_eq!(
        (
            output.status.code(),
            &payload["ok"],
            &payload["permissions"]["ok"],
        ),
        (Some(4), &Value::Bool(false), &Value::Bool(false)),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(windows)]
#[test]
fn offline_smoke_rejects_configuration_access_granted_beyond_the_windows_owner() {
    use winapi::um::winnt::{FILE_ALL_ACCESS, PSID};
    use windows_acl::acl::{ACL, AceType};
    use windows_acl::helper::string_to_sid;

    let endpoint = "http://127.0.0.1:9";
    let environment = SmokeEnvironment::new(|journal_dir| minimal_config(endpoint, journal_dir));
    let config_file = environment.config_dir.join("config.toml");
    let mut acl = ACL::from_file_path(config_file.to_str().expect("Unicode test path"), false)
        .expect("read config ACL");
    let everyone = string_to_sid("S-1-1-0").expect("create Everyone SID");
    acl.add_entry(
        everyone.as_ptr() as PSID,
        AceType::AccessAllow,
        0,
        FILE_ALL_ACCESS,
    )
    .expect("broaden config ACL");

    let output = environment.run(&["smoke"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse smoke JSON");

    assert_eq!(
        (
            output.status.code(),
            &payload["ok"],
            &payload["permissions"]["ok"],
        ),
        (Some(4), &Value::Bool(false), &Value::Bool(false)),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// Debug path formatting supplies the quoted and escaped TOML string literal required by fixtures.
#[expect(clippy::unnecessary_debug_formatting)]
fn complete_config(endpoint: &str, journal_dir: &Path) -> String {
    format!(
        r#"
[classifier]
url = {endpoint:?}
keys = ["classifier-secret"]
model = "classifier-model"

[providers.xai]
url = {endpoint:?}
keys = ["xai-secret"]

[providers.openai_compatible]
url = {endpoint:?}
keys = ["openai-secret"]

[providers.exa]
url = {endpoint:?}
keys = ["exa-secret"]

[providers.tavily]
url = {endpoint:?}
keys = ["tavily-secret"]

[providers.firecrawl]
url = {endpoint:?}
keys = ["firecrawl-secret"]

[providers.jina]
url = {endpoint:?}
keys = ["jina-secret"]

[providers.context7]
url = {endpoint:?}
keys = ["context7-secret"]

[providers.anysearch]
url = {endpoint:?}
keys = ["anysearch-secret"]

[journal]
dir = {journal_dir:?}
"#
    )
}

// Debug path formatting supplies the quoted and escaped TOML string literal required by fixtures.
#[expect(clippy::unnecessary_debug_formatting)]
fn minimal_config(endpoint: &str, journal_dir: &Path) -> String {
    format!(
        "[providers.xai]\nurl = {endpoint:?}\nkeys = [\"xai-secret\"]\n[journal]\ndir = {journal_dir:?}\n"
    )
}

// Debug path formatting supplies the quoted and escaped TOML string literal required by fixtures.
#[expect(clippy::unnecessary_debug_formatting)]
fn p2_config(classifier_url: &str, journal_dir: &Path) -> String {
    format!(
        r#"
[classifier]
url = {classifier_url:?}
keys = ["classifier-secret"]
model = "classifier-model"

[providers.xai]
url = "http://127.0.0.1:9"
keys = ["xai-secret"]

[providers.exa]
url = "http://127.0.0.1:9"
keys = ["exa-secret"]

[providers.anysearch]
url = "http://127.0.0.1:9"
keys = ["anysearch-secret"]

[providers.jina]
url = "http://127.0.0.1:9"
keys = ["jina-secret"]

[capabilities.docs_search]
order = ["exa"]

[capabilities.vertical_search]
order = ["anysearch"]

[capabilities.web_fetch]
order = ["jina"]

[journal]
dir = {journal_dir:?}
"#
    )
}

fn provider_secrets() -> [&'static str; 9] {
    [
        "classifier-secret",
        "xai-secret",
        "openai-secret",
        "exa-secret",
        "tavily-secret",
        "firecrawl-secret",
        "jina-secret",
        "context7-secret",
        "anysearch-secret",
    ]
}
