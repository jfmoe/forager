mod support;

use std::fs;

use serde_json::Value;

use support::{Fixture, Response, RunEnvironment};

#[test]
fn search_uses_the_completed_xai_response_and_deduplicates_sources() {
    let fixture = Fixture::start(
        200,
        "text/event-stream",
        concat!(
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"output\":[{\"content\":[",
            "{\"type\":\"output_text\",\"text\":\"Final answer\",\"annotations\":[",
            "{\"type\":\"url_citation\",\"url\":\"https://example.test/a?token=secret#fragment\",\"title\":\"Source A\"},",
            "{\"type\":\"url_citation\",\"url\":\"https://example.test/a?token=secret\",\"title\":\"Duplicate\"}",
            "]}]}]}}\n\n"
        ),
    );
    let environment = RunEnvironment::new(&search_config(&fixture.url, false));

    let output = environment.run(&["search", "What changed?", "--capabilities", "none"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["answer"],
            &payload["sources"],
            &payload["capabilities"],
            &payload["journal_status"],
        ),
        (
            Some(0),
            &Value::String("Final answer".into()),
            &serde_json::json!([{
                "title": "Source A",
                "url": "https://example.test/a?token=********"
            }]),
            &serde_json::json!([]),
            &Value::String("disabled".into()),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(payload.get("provider_attempts").is_none());

    let request = fixture.finish();
    assert!(request.starts_with("POST /v1/responses "), "{request}");
    assert!(
        request.contains("authorization: Bearer xai-key"),
        "{request}"
    );
    assert!(request.contains("\"stream\":true"), "{request}");
    assert!(request.contains("\"model\":\"test-model\""), "{request}");
}

#[test]
fn search_formats_content_and_markdown_and_marks_json_tee_failures() {
    let body = completed_body("Rendered answer", "Rendered source");
    let fixture = Fixture::start_sequence(vec![
        Response::new(200, "text/event-stream", &body),
        Response::new(200, "text/event-stream", &body),
        Response::new(200, "text/event-stream", &body),
    ]);
    let environment = RunEnvironment::new(&search_config(&fixture.url, false));
    let markdown_tee = environment.state_dir.join("result.md");
    let markdown_tee_argument = markdown_tee.to_string_lossy().into_owned();

    let markdown = environment.run(&[
        "search",
        "Markdown",
        "--capabilities",
        "none",
        "--format",
        "markdown",
        "--output",
        &markdown_tee_argument,
    ]);
    let content = environment.run(&[
        "search",
        "Content",
        "--capabilities",
        "none",
        "--format",
        "content",
    ]);
    let invalid_output = environment.state_dir.to_string_lossy().into_owned();
    let failed_tee = environment.run(&[
        "search",
        "JSON",
        "--capabilities",
        "none",
        "--output",
        &invalid_output,
    ]);
    let failed_payload: Value =
        serde_json::from_slice(&failed_tee.stdout).expect("parse failed tee JSON");

    assert_eq!(markdown.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&markdown.stdout).contains("## Sources"));
    assert_eq!(
        fs::read(&markdown_tee).expect("read markdown tee"),
        markdown.stdout
    );
    assert_eq!(
        (content.status.code(), content.stdout.as_slice()),
        (Some(0), &b"Rendered answer\n"[..])
    );
    assert_eq!(
        (
            failed_tee.status.code(),
            &failed_payload["answer"],
            &failed_payload["output_status"],
            String::from_utf8_lossy(&failed_tee.stderr).contains("cannot write output"),
        ),
        (
            Some(3),
            &Value::String("Rendered answer".into()),
            &Value::String("failed".into()),
            true,
        )
    );
    fixture.finish_all();
}

#[test]
fn search_maps_non_completed_sse_terminals_to_stable_runtime_errors() {
    for body in [
        "data: {\"type\":\"response.failed\",\"error\":{\"message\":\"upstream failed\"}}\n\n",
        "data: {\"type\":\"response.incomplete\",\"response\":{}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n",
    ] {
        let fixture = Fixture::start(200, "text/event-stream", body);
        let environment = RunEnvironment::new(&search_config(&fixture.url, false));

        let output = environment.run(&["search", "Failure", "--capabilities", "none"]);
        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

        assert_eq!(
            (
                output.status.code(),
                &payload["error_kind"],
                &payload["attempts"]["total"],
                &payload["journal_status"],
            ),
            (
                Some(4),
                &Value::String("runtime".into()),
                &Value::from(1),
                &Value::String("disabled".into()),
            ),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        fixture.finish();
    }
}

#[test]
fn search_journals_each_success_and_failure_terminal_once() {
    let fixture = Fixture::start_sequence(vec![
        Response::new(
            200,
            "text/event-stream",
            &completed_body("answer", "Source"),
        ),
        Response::new(
            200,
            "text/event-stream",
            "data: {\"type\":\"response.failed\",\"error\":{\"message\":\"failed\"}}\n\n",
        ),
    ]);
    let environment = RunEnvironment::new(&search_config(&fixture.url, true));

    let success = environment.run(&["search", "Success", "--capabilities", "none"]);
    let failure = environment.run(&["search", "Failure", "--capabilities", "none"]);
    let success_payload: Value =
        serde_json::from_slice(&success.stdout).expect("parse success JSON");
    let failure_payload: Value =
        serde_json::from_slice(&failure.stdout).expect("parse failure JSON");

    assert_eq!(
        (
            success.status.code(),
            &success_payload["journal_status"],
            failure.status.code(),
            &failure_payload["journal_status"],
        ),
        (
            Some(0),
            &Value::String("written".into()),
            Some(4),
            &Value::String("written".into()),
        )
    );
    let journal_dir = environment.state_dir.join("forager/journal");
    let mut records = fs::read_dir(&journal_dir)
        .expect("read journal")
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|value| value == "json")
        })
        .map(|entry| {
            serde_json::from_slice::<Value>(&fs::read(entry.path()).expect("read journal record"))
                .expect("parse journal record")
        })
        .collect::<Vec<_>>();
    records.sort_by_key(|record| record["result"]["query"].as_str().map(str::to_owned));

    assert_eq!(records.len(), 2);
    assert_eq!(
        (
            &records[0]["result"]["status"],
            &records[0]["execution"]["terminal_attribution"],
            records[0]["execution"]["provider_attempts"]
                .as_array()
                .map(Vec::len),
            &records[1]["result"]["status"],
            &records[0]["execution"]["provider_attempts"][0]["model"],
            &records[0]["execution"]["provider_attempts"][0]["endpoint_host"],
            &records[1]["execution"]["deadline_budget"]["total_ms"],
            &records[1]["execution"]["deadline_budget"]["exhausted"],
        ),
        (
            &Value::String("error".into()),
            &Value::String("runtime".into()),
            Some(1),
            &Value::String("ok".into()),
            &Value::String("test-model".into()),
            &Value::String("127.0.0.1".into()),
            &Value::from(180_000),
            &Value::Bool(false),
        )
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        assert_eq!(
            fs::metadata(&journal_dir)
                .expect("journal metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        for entry in fs::read_dir(&journal_dir).expect("journal entries") {
            assert_eq!(
                entry
                    .expect("journal entry")
                    .metadata()
                    .expect("journal file metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
    fixture.finish_all();
}

#[test]
fn search_resolves_relative_journal_directories_from_the_config_directory() {
    let fixture = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Source"),
    );
    let config = search_config(&fixture.url, true)
        .replace("enabled = true", "enabled = true\ndir = \"records\"");
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["search", "Relative", "--capabilities", "none"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (output.status.code(), &payload["journal_status"]),
        (Some(0), &Value::String("written".into()))
    );
    assert_eq!(
        fs::read_dir(environment.config_dir.join("records"))
            .expect("relative journal directory")
            .count(),
        1
    );
    fixture.finish();
}

#[test]
fn search_journal_failure_is_non_fatal_and_reported_in_json() {
    let fixture = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Source"),
    );
    let environment = RunEnvironment::new(&search_config(&fixture.url, true));
    let journal_path = environment.state_dir.join("forager/journal");
    fs::create_dir_all(journal_path.parent().expect("journal parent")).expect("create state");
    fs::write(&journal_path, "not a directory").expect("block journal directory");

    let output = environment.run(&["search", "Still succeeds", "--capabilities", "none"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["journal_status"],
            &payload["journal_ref"],
            String::from_utf8_lossy(&output.stderr)
                .matches("Search Result Journal warning:")
                .count(),
        ),
        (Some(0), &Value::String("failed".into()), &Value::Null, 1,)
    );
    fixture.finish();
}

#[test]
fn search_redacts_secrets_from_stdout_stderr_tee_and_journal() {
    let secret = "canary-secret";
    let fixture = Fixture::start(
        200,
        "text/event-stream",
        &completed_body(
            &format!("answer with {secret}"),
            &format!("title with {secret}"),
        ),
    );
    let config = search_config(&fixture.url, true).replace("xai-key", secret);
    let environment = RunEnvironment::new(&config);
    let tee = environment.state_dir.join("result.json");
    let tee_argument = tee.to_string_lossy().into_owned();

    let output = environment.run(&[
        "search",
        "Canary",
        "--capabilities",
        "none",
        "--verbose",
        "--output",
        &tee_argument,
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse verbose JSON");
    assert_eq!(
        payload["provider_attempts"].as_array().map(Vec::len),
        Some(1)
    );
    let journal = fs::read_dir(environment.state_dir.join("forager/journal"))
        .expect("read journal")
        .find_map(|entry| entry.ok())
        .map(|entry| fs::read_to_string(entry.path()).expect("read journal"))
        .expect("journal record");

    for (name, bytes) in [
        ("stdout", output.stdout),
        ("stderr", output.stderr),
        ("tee", fs::read(&tee).expect("read tee")),
        ("journal", journal.into_bytes()),
    ] {
        assert!(
            !String::from_utf8_lossy(&bytes).contains(secret),
            "{name} leaked the canary"
        );
    }
    fixture.finish();
}

fn search_config(xai_url: &str, journal_enabled: bool) -> String {
    let xai_url = format!("{xai_url}/v1");
    format!(
        r#"
[providers.xai]
url = {xai_url:?}
keys = ["xai-key"]
model = "test-model"
tools = ["web_search"]

[retry]
max_attempts = 1
multiplier = 1
max_wait = 0

[journal]
enabled = {journal_enabled}
"#
    )
}

fn completed_body(answer: &str, title: &str) -> String {
    format!(
        "data: {}\n\n",
        serde_json::json!({
            "type": "response.completed",
            "response": {
                "output": [{
                    "content": [{
                        "type": "output_text",
                        "text": answer,
                        "annotations": [{
                            "type": "url_citation",
                            "url": "https://example.test/source?token=canary-secret",
                            "title": title
                        }]
                    }]
                }]
            }
        })
    )
}
