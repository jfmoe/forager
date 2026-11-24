use std::fs;
use std::process::Output;

use crate::support::exa_samples::no_results;
use crate::support::samples::sample;
use crate::support::{Fixture, RunEnvironment};
use serde_json::Value;

#[test]
fn an_exa_search_sends_the_command_options_and_the_configured_key() {
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
            &payload["query"]
        ),
        (
            Some(0),
            &Value::String("exa".into()),
            &Value::String("rust async drop".into()),
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
fn an_exa_similar_sends_the_command_options_and_the_configured_key() {
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
        (output.status.code(), &payload["provider"], &payload["url"]),
        (
            Some(0),
            &Value::String("exa".into()),
            &Value::String("https://example.test/source".into()),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let request = fixture.finish();
    assert!(
        request.contains("\"url\":\"https://example.test/source\""),
        "{request}"
    );
    assert!(request.contains("\"numResults\":2"), "{request}");
    assert!(request.contains("x-api-key: first-key"), "{request}");
}

#[test]
fn exa_search_and_similar_share_the_persistent_credential_cursor_between_processes() {
    let keys = ["", " first-key ", "first-key", "second-key"];
    let first_fixture = Fixture::start_json(200, &no_results());
    let environment = RunEnvironment::new(&config(&first_fixture.url, &keys));

    let first_output = environment.run(&["exa", "search", "first invocation"]);
    let first_request = first_fixture.finish();
    let second_fixture = Fixture::start_json(200, &sample("exa/similar.json"));
    fs::write(
        environment.config_dir.join("config.toml"),
        config(&second_fixture.url, &keys),
    )
    .expect("write config");
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
    let fixture = Fixture::start_json(200, &sample("exa/search.json"));
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
            String::from_utf8_lossy(&output.stdout)
                .contains("[Fixture result](https://example.test/result?api_key=********&safe=1)"),
            fs::read(output_file.path()).expect("read tee output"),
        ),
        (Some(0), true, output.stdout.clone()),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.finish();
}

/// The stored search body without results.
fn run(fixture: &Fixture, arguments: &[&str], keys: &[&str]) -> Output {
    RunEnvironment::new(&config(&fixture.url, keys)).run(arguments)
}

/// Sends Exa requests to `url` with `keys`, and keeps the journal off.
fn config(url: &str, keys: &[&str]) -> String {
    let keys = keys
        .iter()
        .map(|key| format!("{key:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "[providers.exa]\nurl = {url:?}\nkeys = [{keys}]\ntimeout = 2\n[journal]\nenabled = false\n"
    )
}
