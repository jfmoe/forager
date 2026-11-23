mod support;

use std::fs;

use serde_json::Value;

use support::main_search_samples::answer_with;
use support::x_samples::completed_events;
use support::{Fixture, Response, RunEnvironment};

#[test]
fn search_formats_content_and_markdown_and_marks_json_tee_failures() {
    let body = completed_body(
        concat!(
            "<think>hidden</think>\n",
            "Rendered answer\n\n",
            "References:\n",
            "- [Text source](https://example.test/text)"
        ),
        "Rendered source",
    );
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
    assert!(String::from_utf8_lossy(&markdown.stdout).contains("## Primary Sources"));
    assert!(String::from_utf8_lossy(&markdown.stdout).contains("[Text source]"));
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
fn a_search_with_the_journal_off_writes_no_journal() {
    let fixture = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Source"),
    );
    let environment = RunEnvironment::new(&search_config(&fixture.url, false));

    let output = environment.run(&["search", "Success", "--capabilities", "none"]);
    fixture.finish();
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["journal_status"],
            &payload["journal_ref"],
            environment.state_dir.join("forager/journal").exists(),
        ),
        (
            Some(0),
            &Value::String("disabled".into()),
            &Value::Null,
            false
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
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
        .filter_map(std::result::Result::ok)
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
fn search_canary_exempts_answer_content_but_redacts_protected_outputs() {
    let secret = "canary-secret";
    let query_secret = "query-url-secret";
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body(
            &format!("answer with {secret}"),
            &format!("title with {secret}"),
        ),
    );
    let config = search_config(&main.url, true).replace("xai-key", secret);
    let environment = RunEnvironment::new(&config);
    let tee = environment.state_dir.join("result.json");
    let tee_argument = tee.to_string_lossy().into_owned();

    let output = environment.run(&[
        "search",
        &format!("Canary https://example.test/query?token={query_secret}"),
        "--verbose",
        "--output",
        &tee_argument,
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse verbose JSON");
    assert_eq!(
        (
            &payload["answer"],
            &payload["sources"][0]["title"],
            &payload["sources"][0]["url"],
            payload["provider_attempts"].as_array().map(Vec::len),
        ),
        (
            &Value::String(format!("answer with {secret}")),
            &Value::String("title with ********".into()),
            &Value::String("https://example.test/source?token=********".into()),
            Some(1),
        )
    );
    let journal: Value = fs::read_dir(environment.state_dir.join("forager/journal"))
        .expect("read journal")
        .find_map(std::result::Result::ok)
        .map(|entry| {
            serde_json::from_slice(&fs::read(entry.path()).expect("read journal"))
                .expect("parse journal")
        })
        .expect("journal record");

    let tee_payload: Value =
        serde_json::from_slice(&fs::read(&tee).expect("read tee")).expect("parse tee JSON");
    assert_eq!(tee_payload, payload);
    assert_eq!(
        &journal["result"]["answer"],
        &Value::String(format!("answer with {secret}"))
    );
    assert_eq!(
        &journal["result"]["query"],
        &Value::String("Canary https://example.test/query?token=********".into())
    );
    let mut protected_journal = journal;
    protected_journal["result"]
        .as_object_mut()
        .expect("journal result")
        .remove("answer");
    let protected_journal =
        serde_json::to_string(&protected_journal).expect("encode protected journal");
    assert!(
        !protected_journal.contains(secret) && !protected_journal.contains(query_secret),
        "journal protected fields leaked the canary"
    );
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains(secret),
        "diagnostic leaked the canary"
    );
    main.finish();
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
    completed_events(&answer_with(
        "xai",
        answer,
        &[("https://example.test/source?token=canary-secret", title)],
    ))
}
