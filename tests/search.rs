mod support;

use std::fs;
use std::time::Duration;

use serde_json::Value;

use support::main_search_samples::answer_with;
use support::x_samples::completed_events;
use support::{Fixture, Response, RunEnvironment, jina_response};

#[test]
fn caller_capability_declaration_is_normalized_in_vocabulary_order() {
    let fixture = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Source"),
    );
    let environment = RunEnvironment::new(&search_config(&fixture.url, true));

    let output = environment.run(&[
        "search",
        "Declared",
        "--capabilities",
        "WEB_SEARCH,docs_search,web_search",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let journal = read_only_journal(&environment);

    assert_eq!(
        (
            output.status.code(),
            payload.get("capabilities"),
            &journal["execution"]["plan_summary"]["capabilities"],
        ),
        (
            Some(0),
            None,
            &serde_json::json!(["docs_search", "web_search"])
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.finish();
}

#[test]
fn caller_capability_declaration_rejects_unknown_empty_and_mixed_none_values() {
    let environment = RunEnvironment::new("");

    for declaration in ["unknown", "", "docs_search,", "none,web_search"] {
        let output = environment.run(&[
            "search",
            "Invalid declaration",
            "--capabilities",
            declaration,
        ]);

        assert_eq!(
            output.status.code(),
            Some(2),
            "{declaration:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn search_rejects_extra_sources_above_twenty_before_loading_config() {
    let environment = RunEnvironment::new("invalid = [");

    let output = environment.run(&[
        "search",
        "Out of range",
        "--capabilities",
        "none",
        "--extra-sources",
        "21",
    ]);

    assert_eq!(
        output.status.code(),
        Some(2),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn search_markdown_renders_fetched_previews_as_extra_sources() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let content = "Fetched evidence line one.\nFetched evidence line two.\n".repeat(12);
    let jina = Fixture::start(200, "application/json", &jina_response(&content));
    let config = format!(
        "{}\n[providers.jina]\nurl = {:?}\nkeys = [\"jina-key\"]\ntimeout = 30\n\n[capabilities.web_fetch]\norder = [\"jina\"]\n",
        search_config(&main.url, false),
        jina.url
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "search",
        "Verify https://example.test/article",
        "--capabilities",
        "web_fetch",
        "--format",
        "markdown",
    ]);
    let markdown = String::from_utf8(output.stdout).expect("UTF-8 markdown");

    assert_eq!(output.status.code(), Some(0));
    assert!(markdown.contains("## Primary Sources"), "{markdown}");
    assert!(markdown.contains("## Extra Sources"), "{markdown}");
    assert!(
        markdown.contains("[https://example.test/article](https://example.test/article) — jina"),
        "{markdown}"
    );
    assert!(markdown.contains(&content[..120]), "{markdown}");
    main.finish();
    jina.finish();
}

#[test]
fn supplemental_candidate_without_title_serializes_a_null_title() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let firecrawl = Fixture::start(
        200,
        "application/json",
        r#"{"data":{"web":[{"url":"https://example.test/untitled","description":"Firecrawl summary","publishedDate":"2026-08-06","author":"Reporter"}]}}"#,
    );
    let config = format!(
        "{}\n[providers.firecrawl]\nurl = {:?}\nkeys = [\"firecrawl-key\"]\ntimeout = 30\n\n[capabilities.web_search]\norder = [\"firecrawl\"]\n",
        search_config(&main.url, false),
        firecrawl.url,
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "search",
        "Current information",
        "--capabilities",
        "web_search",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (output.status.code(), &payload["extra_sources"]),
        (
            Some(0),
            &serde_json::json!([{
                "provider": "firecrawl",
                "capability": "web_search",
                "title": null,
                "url": "https://example.test/untitled",
                "summary": "Firecrawl summary",
                "provider_data": {}
            }]),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    main.finish();
    firecrawl.finish();
}

#[test]
fn search_markdown_separates_primary_sources_from_untitled_extra_sources() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let firecrawl = Fixture::start(
        200,
        "application/json",
        r#"{"data":{"web":[{"url":"https://example.test/untitled","description":"Firecrawl summary"}]}}"#,
    );
    let config = format!(
        "{}\n[providers.firecrawl]\nurl = {:?}\nkeys = [\"firecrawl-key\"]\ntimeout = 30\n\n[capabilities.web_search]\norder = [\"firecrawl\"]\n",
        search_config(&main.url, false),
        firecrawl.url,
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "search",
        "Current information",
        "--capabilities",
        "web_search",
        "--format",
        "markdown",
    ]);
    let markdown = String::from_utf8(output.stdout).expect("UTF-8 markdown");

    assert_eq!(
        (output.status.code(), markdown),
        (
            Some(0),
            "# Search result\n\nanswer\n\n## Primary Sources\n\n- [Primary](https://example.test/source?token=********)\n\n## Extra Sources\n\n- [https://example.test/untitled](https://example.test/untitled) — firecrawl\n\n  Firecrawl summary\n".into(),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    main.finish();
    firecrawl.finish();
}

#[test]
fn search_default_json_moves_invocation_echoes_to_the_journal() {
    let answer = concat!(
        "<think>hidden</think>\n",
        "Focused answer [[1]](https://example.test/inline)\n\n",
        "Sources:\n",
        "- [Text source](https://example.test/text)"
    );
    let fixture = Fixture::start(200, "text/event-stream", &completed_body(answer, "Primary"));
    let environment = RunEnvironment::new(&search_config(&fixture.url, true));

    let output = environment.run(&["search", "Focused stdout", "--capabilities", "none"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let journal = read_only_journal(&environment);
    let keys = payload
        .as_object()
        .expect("search payload object")
        .keys()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(
        (
            output.status.code(),
            keys,
            &payload["answer"],
            &payload["sources"],
            &journal["result"]["answer"],
            &journal["result"]["sources"],
            &journal["result"]["query"],
            &journal["execution"]["plan_summary"]["capabilities"],
            &journal["execution"]["provider_attempts"][0]["provider"],
            &journal["execution"]["provider_attempts"][0]["model"],
        ),
        (
            Some(0),
            std::collections::BTreeSet::from([
                "answer",
                "capability_gaps",
                "journal_ref",
                "journal_status",
                "sources",
            ]),
            &Value::String("Focused answer [[1]](https://example.test/inline)".into()),
            &serde_json::json!([
                {
                    "title": "Primary",
                    "url": "https://example.test/source?token=********"
                },
                {"url": "https://example.test/inline"},
                {"title": "Text source", "url": "https://example.test/text"}
            ]),
            &Value::String("Focused answer [[1]](https://example.test/inline)".into()),
            &serde_json::json!([
                {
                    "title": "Primary",
                    "url": "https://example.test/source?token=********"
                },
                {"url": "https://example.test/inline"},
                {"title": "Text source", "url": "https://example.test/text"}
            ]),
            &Value::String("Focused stdout".into()),
            &serde_json::json!([]),
            &Value::String("xai".into()),
            &Value::String("test-model".into()),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.finish();
}

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
fn search_projects_terminal_attempts_without_changing_stdout() {
    let fixture = Fixture::start_sequence(vec![
        Response::json(503, r#"{"error":"retry"}"#),
        Response::sse(200, &completed_body("answer", "Source")),
        Response::json(503, r#"{"error":"retry"}"#),
        Response::sse(200, &completed_body("answer", "Source")),
        Response::json(503, r#"{"error":"retry"}"#),
        Response::sse(200, &completed_body("answer", "Source")),
        Response::json(503, r#"{"error":"retry"}"#),
        Response::sse(200, &completed_body("answer", "Source")),
        Response::json(503, r#"{"error":"retry"}"#),
        Response::sse(200, &completed_body("answer", "Source")),
        Response::json(503, r#"{"error":"retry"}"#),
        Response::sse(200, &completed_body("answer", "Source")),
    ]);
    let outputs = ["info", "debug", "trace"].map(|level| {
        let config = format!(
            "{}\n[log]\nlevel = {level:?}\n",
            search_config(&fixture.url, false).replace("max_attempts = 1", "max_attempts = 2")
        );
        RunEnvironment::new(&config).run(&["search", "Retry projection", "--capabilities", "none"])
    });

    assert_eq!(outputs[0].status.code(), Some(0));
    assert_eq!(outputs[0].stdout, outputs[1].stdout);
    assert_eq!(outputs[1].stdout, outputs[2].stdout);
    let stderr = outputs.map(|output| String::from_utf8(output.stderr).expect("UTF-8 stderr"));
    assert!(stderr[0].is_empty(), "{}", stderr[0]);
    assert_eq!(stderr[1].lines().count(), 1, "{}", stderr[1]);
    assert!(stderr[1].starts_with("forager attempts: "));
    assert_eq!(stderr[2].lines().count(), 3, "{}", stderr[2]);
    assert!(
        stderr[2]
            .lines()
            .next()
            .is_some_and(|line| line.starts_with("forager attempts: "))
    );
    assert!(
        stderr[2]
            .lines()
            .skip(1)
            .all(|line| line.starts_with("forager attempt: "))
    );

    let journals = ["info", "debug", "trace"].map(|level| {
        let config = format!(
            "{}\n[log]\nlevel = {level:?}\n",
            search_config(&fixture.url, true).replace("max_attempts = 1", "max_attempts = 2")
        );
        let environment = RunEnvironment::new(&config);
        let output = environment.run(&["search", "Retry projection", "--capabilities", "none"]);
        assert_eq!(output.status.code(), Some(0));
        normalized_journal_bytes(read_only_journal(&environment))
    });
    assert_eq!(journals[0], journals[1]);
    assert_eq!(journals[1], journals[2]);
    assert_eq!(fixture.finish_all().len(), 12);
}

#[test]
fn search_budget_exhaustion_preserves_the_attempt_chain_in_the_journal() {
    let xai = Fixture::start_sequence(vec![
        Response::sse(200, &completed_body("too late", "Late source"))
            .with_delay(std::time::Duration::from_secs(2)),
    ]);
    let environment = RunEnvironment::new(&search_config(&xai.url, true));

    let output = environment.run(&[
        "search",
        "Budget",
        "--capabilities",
        "none",
        "--timeout",
        "1",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let journal = read_only_journal(&environment);

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            journal["execution"]["provider_attempts"]
                .as_array()
                .map(Vec::len),
            &journal["execution"]["deadline_budget"]["exhausted"],
        ),
        (
            Some(4),
            &Value::String("timeout".into()),
            Some(1),
            &Value::Bool(true),
        )
    );
    xai.finish();
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
fn search_retention_only_removes_expired_owned_regular_files() {
    let fixture = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Source"),
    );
    let environment = RunEnvironment::new(&search_config(&fixture.url, true));
    let journal_dir = environment.state_dir.join("forager/journal");
    fs::create_dir_all(&journal_dir).expect("create journal directory");
    let stale_time = std::time::SystemTime::now()
        .checked_sub(Duration::from_hours(960))
        .expect("calculate stale modification time");
    let stale_owned = journal_dir.join("search_result_1_2_3.json");
    let fresh_owned = journal_dir.join("search_result_4_5_6.json");
    let foreign_json = journal_dir.join("foreign.json");
    let similar_name = journal_dir.join("search_result_1_2_bad.json");
    for path in [&stale_owned, &fresh_owned, &foreign_json, &similar_name] {
        fs::write(path, "{}").expect("create retention fixture");
    }
    for path in [&stale_owned, &foreign_json, &similar_name] {
        fs::File::open(path)
            .expect("open retention fixture")
            .set_times(fs::FileTimes::new().set_modified(stale_time))
            .expect("set retention fixture modification time");
    }
    let owned_directory = journal_dir.join("search_result_7_8_9.json");
    fs::create_dir(&owned_directory).expect("create owned-name directory");
    #[cfg(unix)]
    let owned_symlink = {
        let path = journal_dir.join("search_result_10_11_12.json");
        std::os::unix::fs::symlink(&foreign_json, &path).expect("create owned-name symlink");
        path
    };

    let output = environment.run(&["search", "Retention", "--capabilities", "none"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["answer"],
            &payload["journal_status"],
        ),
        (
            Some(0),
            &Value::String("answer".into()),
            &Value::String("written".into()),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!stale_owned.exists());
    for path in [fresh_owned, foreign_json, similar_name] {
        assert!(path.exists(), "retention removed {}", path.display());
    }
    assert!(owned_directory.is_dir());
    #[cfg(unix)]
    assert!(owned_symlink.symlink_metadata().is_ok());
    let journal_ref = payload["journal_ref"].as_str().expect("journal reference");
    assert!(std::path::Path::new(journal_ref).is_file());
    fixture.finish();
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
fn search_fails_when_xai_completed_response_has_no_output_text() {
    let xai = Fixture::start(
        200,
        "text/event-stream",
        concat!(
            "data: {\"type\":\"response.completed\",\"response\":{\"output\":[",
            "{\"content\":[{\"type\":\"reasoning\",\"text\":\"hidden\"}]}]}}\n\n"
        ),
    );
    let environment = RunEnvironment::new(&search_config(&xai.url, false));

    let output = environment.run(&["search", "No text", "--capabilities", "none"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            payload.get("answer"),
        ),
        (Some(4), &Value::String("runtime".into()), None),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    xai.finish();
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

fn read_only_journal(environment: &RunEnvironment) -> Value {
    let entry = fs::read_dir(environment.state_dir.join("forager/journal"))
        .expect("read journal")
        .next()
        .expect("journal entry")
        .expect("valid journal entry");
    serde_json::from_slice(&fs::read(entry.path()).expect("read journal record"))
        .expect("parse journal record")
}

fn normalized_journal_bytes(mut journal: Value) -> Vec<u8> {
    journal["recorded_at_unix_ms"] = Value::Null;
    journal["execution"]["deadline_budget"]["consumed_ms"] = Value::Null;
    for attempt in journal["execution"]["provider_attempts"]
        .as_array_mut()
        .expect("journal attempts")
    {
        attempt["duration_ms"] = Value::Null;
    }
    serde_json::to_vec(&journal).expect("encode normalized journal")
}
