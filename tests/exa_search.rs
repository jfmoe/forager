mod support;

use std::fs::{self, OpenOptions};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use fs2::FileExt;
use serde_json::Value;
use support::{Fixture, Response};

#[test]
fn exa_search_returns_normalized_results_through_the_real_http_stack() {
    let fixture = Fixture::start_json(
        200,
        r#"{
            "results": [{
                "id": "https://example.test/result",
                "title": "Fixture result",
                "url": "https://example.test/result?api_key=source-secret&safe=1#fragment",
                "publishedDate": "2026-07-25T00:00:00.000Z",
                "author": "Example",
                "text": "Fixture body first-key",
                "highlights": ["Fixture highlight first-key"]
            }]
        }"#,
    );

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
    assert!(request.contains("\"text\":true"), "{request}");
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
    let fixture = Fixture::start_json(
        200,
        r#"{
            "results": [{
                "title": "Similar fixture",
                "url": "https://example.test/similar?api_key=source-secret&safe=1#fragment",
                "publishedDate": "2026-07-25T00:00:00.000Z",
                "author": "Example"
            }]
        }"#,
    );

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
fn exa_search_treats_an_empty_result_set_as_success() {
    let fixture = Fixture::start_json(200, r#"{"results":[]}"#);

    let output = run(&fixture, &["exa", "search", "no matches"], &["only-key"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (output.status.code(), &payload["results"]),
        (Some(0), &serde_json::json!([])),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.finish();
}

#[test]
fn exa_similar_treats_an_empty_result_set_as_success() {
    let fixture = Fixture::start_json(200, r#"{"results":[]}"#);

    let output = run(
        &fixture,
        &["exa", "similar", "https://example.test/source"],
        &["only-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (output.status.code(), &payload["results"]),
        (Some(0), &serde_json::json!([])),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.finish();
}

#[test]
fn exa_similar_rejects_an_invalid_result_count_before_network_or_config() {
    let output = Command::new(env!("CARGO_BIN_EXE_forager"))
        .args([
            "exa",
            "similar",
            "https://example.test/source",
            "--num-results",
            "0",
        ])
        .env_clear()
        .output()
        .expect("run forager");

    assert_eq!(
        (
            output.status.code(),
            output.stdout.is_empty(),
            String::from_utf8_lossy(&output.stderr).contains("--num-results"),
        ),
        (Some(2), true, true)
    );
}

#[test]
fn exa_similar_classifies_authentication_failures() {
    let fixture = Fixture::start_json(401, r#"{"error":{"message":"invalid credential"}}"#);

    let output = run(
        &fixture,
        &["exa", "similar", "https://example.test/source"],
        &["only-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (output.status.code(), &payload["error_kind"]),
        (Some(4), &Value::String("auth".into())),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.finish();
}

#[test]
fn exa_similar_rotates_credentials_after_a_rate_limit() {
    let fixture = Fixture::start_sequence(vec![
        Response::json(429, r#"{"error":{"message":"rate limited"}}"#),
        Response::json(200, r#"{"results":[]}"#),
    ]);

    let output = run(
        &fixture,
        &["exa", "similar", "https://example.test/source", "--verbose"],
        &["first-key", "second-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let requests = fixture.finish_all();

    assert_eq!(
        (
            output.status.code(),
            &payload["provider_attempts"][0]["error_kind"],
            &payload["provider_attempts"][0]["seam"],
            requests[0].contains("x-api-key: first-key"),
            requests[1].contains("x-api-key: second-key"),
        ),
        (
            Some(0),
            &Value::String("rate_limited".into()),
            &Value::String("similar".into()),
            true,
            true,
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn exa_similar_obeys_the_command_deadline() {
    let fixture = Fixture::start_sequence(vec![
        Response::json(200, r#"{"results":[]}"#).with_delay(Duration::from_millis(1500)),
    ]);

    let output = run(
        &fixture,
        &[
            "exa",
            "similar",
            "https://example.test/source",
            "--timeout",
            "1",
        ],
        &["only-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (output.status.code(), &payload["error_kind"]),
        (Some(4), &Value::String("timeout".into())),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.finish_all();
}

#[test]
fn exa_search_keeps_default_failure_json_small_and_secret_free() {
    let canary = "credential-canary-do-not-print";
    let message = format!("{canary} {}", "failure detail ".repeat(900));
    let body = format!(r#"{{"error":{{"message":{message:?}}}}}"#);
    let fixture = Fixture::start_json(401, &body);

    let output = run(&fixture, &["exa", "search", "auth failure"], &[canary]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            payload["attempts"]["total"].as_u64(),
            payload.get("provider_attempts"),
            output.stdout.len() <= 4096,
            combined.contains(canary),
        ),
        (
            Some(4),
            &Value::String("auth".into()),
            Some(1),
            None,
            true,
            false,
        )
    );
    fixture.finish();
}

#[test]
fn exa_search_redacts_urls_in_non_success_responses() {
    let message =
        "upstream rejected https://user:password@example.test/private?api_key=response-secret";
    let body = format!(r#"{{"error":{{"message":{message:?}}}}}"#);
    let fixture = Fixture::start_json(400, &body);

    let output = run(&fixture, &["exa", "search", "redaction"], &["only-key"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (output.status.code(), &payload["message"]),
        (
            Some(4),
            &Value::String(
                "upstream rejected https://example.test/private?api_key=********".into(),
            ),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.finish();
}

#[test]
fn exa_search_verbose_attempts_can_exceed_the_default_payload_limit() {
    let message = "rate limit detail ".repeat(100);
    let body = format!(r#"{{"error":{{"message":{message:?}}}}}"#);
    let responses = (0..10).map(|_| Response::json(429, &body)).collect();
    let fixture = Fixture::start_sequence(responses);
    let keys = (0..10)
        .map(|index| format!("key-{index}"))
        .collect::<Vec<_>>();
    let key_refs = keys.iter().map(String::as_str).collect::<Vec<_>>();

    let output = run(
        &fixture,
        &["exa", "search", "verbose failure", "--verbose"],
        &key_refs,
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let requests = fixture.finish_all();

    assert_eq!(
        (
            output.status.code(),
            payload["provider_attempts"].as_array().map(Vec::len),
            requests.len(),
            output.stdout.len() > 4096,
        ),
        (Some(4), Some(10), 10, true)
    );
}

#[test]
fn exa_search_rotates_credentials_before_retrying_a_rate_limit() {
    let fixture = Fixture::start_sequence(vec![
        Response::json(429, r#"{"error":{"message":"rate limited"}}"#),
        Response::json(200, r#"{"results":[]}"#),
    ]);

    let output = run(
        &fixture,
        &["exa", "search", "rotate", "--verbose"],
        &["first-key", "second-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let requests = fixture.finish_all();

    assert_eq!(
        (
            output.status.code(),
            &payload["provider_attempts"][0]["error_kind"],
            &payload["provider_attempts"][0]["credential_index"],
            &payload["provider_attempts"][0]["rotation_count"],
            &payload["provider_attempts"][1]["rotation_count"],
            requests[0].contains("x-api-key: first-key"),
            requests[1].contains("x-api-key: second-key"),
        ),
        (
            Some(0),
            &Value::String("rate_limited".into()),
            &Value::from(0),
            &Value::from(0),
            &Value::from(1),
            true,
            true,
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn exa_search_rotates_credentials_before_retrying_exhausted_quota() {
    let fixture = Fixture::start_sequence(vec![
        Response::json(429, r#"{"error":{"message":"monthly quota exhausted"}}"#),
        Response::json(200, r#"{"results":[]}"#),
    ]);

    let output = run(
        &fixture,
        &["exa", "search", "rotate quota", "--verbose"],
        &["first-key", "second-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let requests = fixture.finish_all();

    assert_eq!(
        (
            output.status.code(),
            &payload["provider_attempts"][0]["error_kind"],
            requests[0].contains("x-api-key: first-key"),
            requests[1].contains("x-api-key: second-key"),
        ),
        (
            Some(0),
            &Value::String("quota_exhausted".into()),
            true,
            true,
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn exa_search_retries_a_transient_failure_without_rotating() {
    let fixture = Fixture::start_sequence(vec![
        Response::json(503, r#"{"error":{"message":"temporarily unavailable"}}"#),
        Response::json(200, r#"{"results":[]}"#),
    ]);

    let output = run(&fixture, &["exa", "search", "retry"], &["only-key"]);
    let requests = fixture.finish_all();

    assert_eq!(
        (
            output.status.code(),
            requests[0].contains("x-api-key: only-key"),
            requests[1].contains("x-api-key: only-key"),
        ),
        (Some(0), true, true),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn exa_search_and_similar_share_the_persistent_credential_cursor_between_processes() {
    let first_fixture = Fixture::start_json(200, r#"{"results":[]}"#);
    let environment = RunEnvironment::new(
        &first_fixture.url,
        &["", " first-key ", "first-key", "second-key"],
    );

    let first_output = environment.run(&["exa", "search", "first invocation"]);
    let first_request = first_fixture.finish();
    let second_fixture = Fixture::start_json(200, r#"{"results":[]}"#);
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
fn exa_search_uses_bounded_optimistic_selection_when_the_cursor_lock_is_busy() {
    let fixture = Fixture::start_json(200, r#"{"results":[]}"#);
    let environment = RunEnvironment::new(&fixture.url, &["first-key", "second-key"]);
    let state_directory = environment.state_dir.join("forager");
    fs::create_dir_all(&state_directory).expect("create credential state directory");
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(state_directory.join("credential_pool_state.lock"))
        .expect("open credential lock");
    lock.lock_exclusive().expect("hold credential lock");

    let started = Instant::now();
    let output = environment.run(&["exa", "search", "busy lock"]);
    let elapsed = started.elapsed();
    FileExt::unlock(&lock).expect("unlock credential state");
    let request = fixture.finish();
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["results"],
            request.contains("x-api-key: first-key"),
            String::from_utf8_lossy(&output.stderr).contains("optimistic selection"),
            elapsed < Duration::from_secs(2),
        ),
        (Some(0), &serde_json::json!([]), true, true, true)
    );
}

#[test]
fn exa_search_resets_only_its_corrupt_cursor_and_persists_the_repair() {
    let fixture = Fixture::start_json(200, r#"{"results":[]}"#);
    let environment = RunEnvironment::new(&fixture.url, &["first-key", "second-key"]);
    let state_directory = environment.state_dir.join("forager");
    fs::create_dir_all(&state_directory).expect("create credential state directory");
    let state_path = state_directory.join("credential_pool_state.json");
    fs::write(
        &state_path,
        r#"{"schema_version":1,"providers":{"exa":{"next_index":"bad"},"other":{"next_index":7}}}"#,
    )
    .expect("write corrupt provider cursor");

    let output = environment.run(&["exa", "search", "repair cursor"]);
    fixture.finish();
    let state: Value = serde_json::from_slice(&fs::read(state_path).expect("read repaired state"))
        .expect("parse repaired state");

    assert_eq!(
        (
            output.status.code(),
            &state["providers"]["exa"]["next_index"],
            &state["providers"]["other"]["next_index"],
            String::from_utf8_lossy(&output.stderr).contains("exa was reset"),
        ),
        (Some(0), &Value::from(1), &Value::from(7), true)
    );
}

#[test]
fn exa_search_obeys_the_command_deadline() {
    let fixture = Fixture::start_sequence(vec![
        Response::json(200, r#"{"results":[]}"#).with_delay(Duration::from_millis(1500)),
    ]);

    let output = run(
        &fixture,
        &["exa", "search", "timeout", "--timeout", "1"],
        &["only-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (output.status.code(), &payload["error_kind"]),
        (Some(4), &Value::String("timeout".into())),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.finish_all();
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

#[test]
fn exa_search_marks_a_tee_write_failure_and_exits_three() {
    let fixture = Fixture::start_json(200, r#"{"results":[]}"#);
    let directory = tempfile::tempdir().expect("create unwritable output target");
    let output_path = directory.path().to_string_lossy().into_owned();

    let output = run(
        &fixture,
        &["exa", "search", "tee failure", "--output", &output_path],
        &["only-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["output_status"],
            String::from_utf8_lossy(&output.stderr).contains("cannot write output"),
        ),
        (Some(3), &Value::String("failed".into()), true)
    );
    fixture.finish();
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
        Command::new(env!("CARGO_BIN_EXE_forager"))
            .args(arguments)
            .env_clear()
            .env("FORAGER_CONFIG_DIR", &self.config_dir)
            .env("XDG_STATE_HOME", &self.state_dir)
            .env("HOME", self.root.path())
            .output()
            .expect("run forager")
    }
}
