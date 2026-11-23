mod support;

use std::fs;
use std::process::Output;

use serde_json::Value;
use support::samples::sample;
use support::{Fixture, forager_command, run_command};

#[test]
fn exa_search_returns_normalized_results_through_the_real_http_stack() {
    let fixture = Fixture::start_json(200, &sample("exa/search.json"));

    let output = run(
        &fixture,
        &[
            "exa",
            "search",
            "rust async drop",
            "--num-results",
            "2",
            "--include-text",
            "--include-highlights",
            "--search-type",
            "neural",
            "--start-published-date",
            "2026-07-01",
            "--include-domains",
            "rust-lang.org,docs.rs",
            "--exclude-domains",
            "example.com",
            "--category",
            "research paper",
        ],
        &["first-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["provider"],
            &payload["query"],
            &payload["results"][0]["title"],
            &payload["results"][0]["url"],
            &payload["results"][0]["text"],
            &payload["results"][0]["highlights"][0],
        ),
        (
            Some(0),
            &Value::String("exa".into()),
            &Value::String("rust async drop".into()),
            &Value::String("Fixture result".into()),
            &Value::String("https://example.test/result?api_key=********&safe=1".into()),
            &Value::String("Fixture body ********".into()),
            &Value::String("Fixture highlight ********".into()),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let request = fixture.finish();
    assert!(request.contains("\"numResults\":2"), "{request}");
    assert!(request.contains("\"type\":\"neural\""), "{request}");
    assert!(
        request.contains("\"text\":{\"maxCharacters\":3000}"),
        "{request}"
    );
    assert!(request.contains("\"highlights\":true"), "{request}");
    assert!(
        request.contains("\"startPublishedDate\":\"2026-07-01\""),
        "{request}"
    );
    assert!(
        request.contains("\"includeDomains\":[\"rust-lang.org\",\"docs.rs\"]"),
        "{request}"
    );
    assert!(
        request.contains("\"excludeDomains\":[\"example.com\"]"),
        "{request}"
    );
    assert!(
        request.contains("\"category\":\"research paper\""),
        "{request}"
    );
    assert!(request.contains("x-api-key: first-key"), "{request}");
}

#[test]
fn exa_similar_returns_normalized_results_through_the_real_http_stack() {
    let fixture = Fixture::start_json(200, &sample("exa/similar.json"));

    let output = run(
        &fixture,
        &[
            "exa",
            "similar",
            "https://example.test/source",
            "--num-results",
            "2",
        ],
        &["first-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["provider"],
            &payload["url"],
            &payload["results"][0]["title"],
            &payload["results"][0]["url"],
        ),
        (
            Some(0),
            &Value::String("exa".into()),
            &Value::String("https://example.test/source".into()),
            &Value::String("Similar fixture".into()),
            &Value::String("https://example.test/similar?api_key=********&safe=1".into()),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let request = fixture.finish();
    assert!(request.starts_with("POST /findSimilar "), "{request}");
    assert!(
        request.contains("\"url\":\"https://example.test/source\""),
        "{request}"
    );
    assert!(request.contains("\"numResults\":2"), "{request}");
    assert!(request.contains("x-api-key: first-key"), "{request}");
}

#[test]
fn exa_search_and_similar_share_the_persistent_credential_cursor_between_processes() {
    let first_fixture = Fixture::start_json(200, &no_results());
    let environment = RunEnvironment::new(
        &first_fixture.url,
        &["", " first-key ", "first-key", "second-key"],
    );

    let first_output = environment.run(&["exa", "search", "first invocation"]);
    let first_request = first_fixture.finish();
    let second_fixture = Fixture::start_json(200, &sample("exa/similar.json"));
    environment.set_url(&second_fixture.url);
    let second_output = environment.run(&["exa", "similar", "https://example.test/source"]);
    let second_request = second_fixture.finish();
    let state_path = environment
        .state_dir
        .join("forager/credential_pool_state.json");
    let state: Value =
        serde_json::from_slice(&fs::read(&state_path).expect("read credential state"))
            .expect("parse credential state");

    assert_eq!(
        (
            first_output.status.code(),
            second_output.status.code(),
            first_request.contains("x-api-key: first-key"),
            second_request.contains("x-api-key: second-key"),
            second_request.starts_with("POST /findSimilar "),
            &state["schema_version"],
            &state["providers"]["exa"]["next_index"],
        ),
        (
            Some(0),
            Some(0),
            true,
            true,
            true,
            &Value::from(1),
            &Value::from(0),
        ),
        "first stderr: {}; second stderr: {}",
        String::from_utf8_lossy(&first_output.stderr),
        String::from_utf8_lossy(&second_output.stderr)
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(state_path)
                .expect("credential state metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn exa_search_supports_markdown_and_output_tee() {
    let fixture = Fixture::start_json(
        200,
        r#"{"results":[{"title":"Tee result","url":"https://example.test"}]}"#,
    );
    let output_file = tempfile::NamedTempFile::new().expect("create output file");
    let output_path = output_file.path().to_string_lossy().into_owned();

    let output = run(
        &fixture,
        &[
            "exa",
            "search",
            "tee",
            "--format",
            "markdown",
            "--output",
            &output_path,
        ],
        &["only-key"],
    );

    assert_eq!(
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).contains("[Tee result]"),
            fs::read(output_file.path()).expect("read tee output"),
        ),
        (Some(0), true, output.stdout.clone()),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.finish();
}

/// The stored search body without results.
fn no_results() -> String {
    let mut body: Value =
        serde_json::from_str(&sample("exa/search.json")).expect("stored Exa JSON");
    body["results"] = serde_json::json!([]);
    body.to_string()
}

fn run(fixture: &Fixture, arguments: &[&str], keys: &[&str]) -> Output {
    RunEnvironment::new(&fixture.url, keys).run(arguments)
}

struct RunEnvironment {
    root: tempfile::TempDir,
    config_dir: std::path::PathBuf,
    state_dir: std::path::PathBuf,
    keys: Vec<String>,
}

impl RunEnvironment {
    fn new(url: &str, keys: &[&str]) -> Self {
        let root = tempfile::tempdir().expect("create isolated root");
        let config_dir = root.path().join("config");
        let state_dir = root.path().join("state");
        fs::create_dir_all(&config_dir).expect("create config directory");
        let environment = Self {
            root,
            config_dir,
            state_dir,
            keys: keys.iter().map(|key| (*key).to_owned()).collect(),
        };
        environment.set_url(url);
        environment
    }

    fn set_url(&self, url: &str) {
        let keys = self
            .keys
            .iter()
            .map(|key| format!("{key:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        fs::write(
            self.config_dir.join("config.toml"),
            format!(
                "[providers.exa]\nurl = {url:?}\nkeys = [{keys}]\ntimeout = 2\n[journal]\nenabled = false\n"
            ),
        )
        .expect("write config");
    }

    fn run(&self, arguments: &[&str]) -> Output {
        let mut command = forager_command();
        command
            .args(arguments)
            .env("FORAGER_CONFIG_DIR", &self.config_dir)
            .env("XDG_STATE_HOME", &self.state_dir)
            .env("HOME", self.root.path());
        run_command(&mut command, None)
    }
}
