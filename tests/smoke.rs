use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

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
        fs::write(config_dir.join("config.toml"), config(&journal_dir)).expect("write config");
        make_private(&config_dir, 0o700);
        make_private(&config_dir.join("config.toml"), 0o600);
        Self {
            _root: root,
            config_dir,
            state_dir,
            home_dir,
            journal_dir,
        }
    }

    fn run(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_forager"))
            .args(arguments)
            .env_clear()
            .env(
                "XDG_CONFIG_HOME",
                self.config_dir.parent().expect("XDG config home"),
            )
            .env("XDG_STATE_HOME", &self.state_dir)
            .env("HOME", &self.home_dir)
            .output()
            .expect("run forager")
    }
}

#[test]
fn offline_smoke_reports_local_readiness_without_contacting_provider_endpoints() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind network canary");
    let endpoint = format!(
        "http://{}",
        listener.local_addr().expect("listener address")
    );
    let environment = SmokeEnvironment::new(|journal_dir| complete_config(&endpoint, journal_dir));

    let output = environment.run(&["smoke"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse smoke JSON");
    listener
        .set_nonblocking(true)
        .expect("make network canary nonblocking");
    let network_was_contacted = listener.accept().is_ok();
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

fn minimal_config(endpoint: &str, journal_dir: &Path) -> String {
    format!(
        "[providers.xai]\nurl = {endpoint:?}\nkeys = [\"xai-secret\"]\n[journal]\ndir = {journal_dir:?}\n"
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

#[cfg(unix)]
fn make_private(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("set private permissions");
}

#[cfg(windows)]
fn make_private(path: &Path, _mode: u32) {
    if path.is_dir() {
        forager::config::ensure_private_directory(path)
            .expect("restrict Windows directory permissions");
    } else {
        forager::config::create_private_file(path).expect("restrict Windows file permissions");
    }
}

#[cfg(not(any(unix, windows)))]
fn make_private(_path: &Path, _mode: u32) {}
