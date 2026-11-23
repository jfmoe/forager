mod support;

use std::fs;
use std::time::Duration;

use serde_json::Value;

use support::{
    Fixture, JOURNAL_EXECUTION_KEYS, Response, Route, RunEnvironment, jina_response, object_keys,
    request_json,
};

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
fn undeclared_search_runs_main_search_and_no_supplemental_seam() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let supplemental = Fixture::start_canary();
    let config = format!(
        "{}\n[providers.tavily]\nurl = {url:?}\nkeys = [\"tavily-key\"]\ntimeout = 30\n\n[providers.exa]\nurl = {url:?}\nkeys = [\"exa-key\"]\ntimeout = 30\n\n[capabilities.web_search]\norder = [\"tavily\"]\n\n[capabilities.docs_search]\norder = [\"exa\"]\n",
        search_config(&main.url, true),
        url = supplemental.url,
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["search", "Ordinary bare search", "--verbose"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let journal = read_only_journal(&environment);
    let attempted_seams = payload["provider_attempts"]
        .as_array()
        .expect("provider attempts")
        .iter()
        .map(|attempt| attempt["seam"].as_str().expect("attempt seam"))
        .collect::<Vec<_>>();

    assert_eq!(
        (
            output.status.code(),
            attempted_seams,
            payload.get("extra_sources"),
            &journal["schema_version"],
            object_keys(&journal["execution"]),
            &journal["execution"]["plan_summary"],
            output.stderr.is_empty(),
        ),
        (
            Some(0),
            vec!["main_search"],
            None,
            &serde_json::json!(1),
            JOURNAL_EXECUTION_KEYS.to_vec(),
            &serde_json::json!({
                "source": "default_main_search_only",
                "capabilities": []
            }),
            true,
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    main.finish();
    assert_eq!(supplemental.finish_all(), Vec::<String>::new());
}

#[test]
fn declared_web_search_without_a_target_requests_its_local_default_of_three() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let tavily = Fixture::start(
        200,
        "application/json",
        r#"{"results":[{"title":"Default source","url":"https://example.test/default"}]}"#,
    );
    let config = format!(
        "{}\n[providers.tavily]\nurl = {:?}\nkeys = [\"tavily-key\"]\ntimeout = 30\n\n[capabilities.web_search]\norder = [\"tavily\"]\n",
        search_config(&main.url, true),
        tavily.url,
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["search", "Ordinary search", "--capabilities", "web_search"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["extra_sources"][0]["url"],
            output.stderr.is_empty(),
        ),
        (
            Some(0),
            &Value::String("https://example.test/default".into()),
            true,
        )
    );
    main.finish();
    let request = tavily.finish();
    let request_body = request_json(&request);
    assert_eq!(request_body["max_results"], 3);
}

#[test]
fn declared_search_records_the_caller_as_its_plan_source() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let environment = RunEnvironment::new(&search_config(&main.url, true));

    let output = environment.run(&[
        "search",
        "Caller owns the decision",
        "--capabilities",
        "none",
        "--verbose",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let journal = read_only_journal(&environment);

    assert_eq!(
        (
            output.status.code(),
            payload["provider_attempts"].as_array().map(Vec::len),
            object_keys(&journal["execution"]),
            &journal["execution"]["plan_summary"],
            output.stderr.is_empty(),
        ),
        (
            Some(0),
            Some(1),
            JOURNAL_EXECUTION_KEYS.to_vec(),
            &serde_json::json!({"source": "caller", "capabilities": []}),
            true,
        )
    );
    main.finish();
}

#[test]
fn declared_web_search_adds_normalized_extra_sources_in_configured_order() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let summary = "provider-native summary ".repeat(20);
    let tavily_body = serde_json::json!({
        "results": [
            {"title": "Missing URL must be skipped", "content": "Invalid candidate"},
            {"title": "Null URL must be skipped", "url": null},
            {"title": "Blank URL must be skipped", "url": "   "},
            {
                "title": "Primary duplicate",
                "url": "https://example.test/source?token=canary-secret"
            },
            {
                "title": "Supplemental",
                "url": "  https://example.test/extra  ",
                "content": summary
            }
        ]
    })
    .to_string();
    let tavily = Fixture::start(200, "application/json", &tavily_body);
    let config = format!(
        "{}\n[providers.tavily]\nurl = {:?}\nkeys = [\"tavily-key\"]\ntimeout = 30\n\n[capabilities.web_search]\norder = [\"tavily\"]\n",
        search_config(&main.url, true),
        tavily.url
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "search",
        "Current information",
        "--capabilities",
        "web_search",
        "--extra-sources",
        "20",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let journal = read_only_journal(&environment);

    assert_eq!(
        (
            output.status.code(),
            &payload["answer"],
            &payload["extra_sources"],
            &payload["sources"],
            &payload["capability_gaps"],
            &journal["result"]["sources"],
            &journal["result"]["extra_sources"],
        ),
        (
            Some(0),
            &Value::String("answer".into()),
            &serde_json::json!([{
                "provider": "tavily",
                "capability": "web_search",
                "title": "Supplemental",
                "url": "https://example.test/extra",
                "summary": summary,
                "provider_data": {}
            }]),
            &serde_json::json!([{
                "title": "Primary",
                "url": "https://example.test/source?token=********"
            }]),
            &serde_json::json!([]),
            &serde_json::json!([{
                "title": "Primary",
                "url": "https://example.test/source?token=********"
            }]),
            &serde_json::json!([{
                "provider": "tavily",
                "capability": "web_search",
                "title": "Supplemental",
                "url": "https://example.test/extra",
                "summary": summary,
                "provider_data": {}
            }]),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    main.finish();
    let request = tavily.finish();
    assert!(request.starts_with("POST /search "), "{request}");
    let request_body = request_json(&request);
    assert_eq!(
        request_body,
        serde_json::json!({
            "query": "Current information",
            "max_results": 20,
            "search_depth": "advanced",
            "chunks_per_source": 1,
            "include_raw_content": false,
            "include_answer": false
        })
    );
}

#[test]
fn empty_tavily_results_fall_back_to_a_nonempty_firecrawl_result() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let tavily = Fixture::start(200, "application/json", r#"{"results":[]}"#);
    let firecrawl = Fixture::start(
        200,
        "application/json",
        r#"{"data":{"web":[{"title":"Fallback","url":"https://example.test/fallback"}]}}"#,
    );
    let config = format!(
        "{}\n[providers.tavily]\nurl = {:?}\nkeys = [\"tavily-key\"]\ntimeout = 30\n\n[providers.firecrawl]\nurl = {:?}\nkeys = [\"firecrawl-key\"]\ntimeout = 30\n\n[capabilities.web_search]\norder = [\"tavily\", \"firecrawl\"]\n",
        search_config(&main.url, true),
        tavily.url,
        firecrawl.url,
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "search",
        "Current information",
        "--capabilities",
        "web_search",
        "--verbose",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let web_attempts = payload["provider_attempts"]
        .as_array()
        .expect("provider attempts")
        .iter()
        .filter(|attempt| attempt["seam"] == "web_search")
        .map(|attempt| attempt["provider"].as_str().expect("attempt provider"))
        .collect::<Vec<_>>();

    assert_eq!(
        (
            output.status.code(),
            &payload["extra_sources"],
            &payload["capability_gaps"],
            web_attempts,
        ),
        (
            Some(0),
            &serde_json::json!([{
                "provider": "firecrawl",
                "capability": "web_search",
                "title": "Fallback",
                "url": "https://example.test/fallback",
                "summary": null,
                "provider_data": {}
            }]),
            &serde_json::json!([]),
            vec!["tavily", "firecrawl"],
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    main.finish();
    tavily.finish();
    firecrawl.finish();
}

#[test]
fn non_http_tavily_result_is_skipped_while_valid_results_are_kept() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let tavily = Fixture::start(
        200,
        "application/json",
        r#"{"results":[{"title":"Invalid","url":"ftp://example.test/result"},{"title":"Valid","url":"https://example.test/valid"}]}"#,
    );
    let config = format!(
        "{}\n[providers.tavily]\nurl = {:?}\nkeys = [\"tavily-key\"]\ntimeout = 30\n\n[capabilities.web_search]\norder = [\"tavily\"]\n",
        search_config(&main.url, true),
        tavily.url,
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
                "provider": "tavily",
                "capability": "web_search",
                "title": "Valid",
                "url": "https://example.test/valid",
                "summary": null,
                "provider_data": {}
            }]),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    main.finish();
    tavily.finish();
}

#[test]
fn a_fully_empty_supplemental_chain_succeeds_with_all_attempts() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let tavily = Fixture::start(200, "application/json", r#"{"results":[]}"#);
    let firecrawl = Fixture::start(200, "application/json", r#"{"data":{"web":[]}}"#);
    let config = format!(
        "{}\n[providers.tavily]\nurl = {:?}\nkeys = [\"tavily-key\"]\ntimeout = 30\n\n[providers.firecrawl]\nurl = {:?}\nkeys = [\"firecrawl-key\"]\ntimeout = 30\n\n[capabilities.web_search]\norder = [\"tavily\", \"firecrawl\"]\n",
        search_config(&main.url, true),
        tavily.url,
        firecrawl.url,
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "search",
        "Current information",
        "--capabilities",
        "web_search",
        "--verbose",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let web_attempts = payload["provider_attempts"]
        .as_array()
        .expect("provider attempts")
        .iter()
        .filter(|attempt| attempt["seam"] == "web_search")
        .map(|attempt| attempt["provider"].as_str().expect("attempt provider"))
        .collect::<Vec<_>>();

    assert_eq!(
        (
            output.status.code(),
            payload.get("extra_sources"),
            &payload["capability_gaps"],
            web_attempts,
        ),
        (
            Some(0),
            None,
            &serde_json::json!([]),
            vec!["tavily", "firecrawl"],
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    main.finish();
    tavily.finish();
    firecrawl.finish();
}

#[test]
fn fallback_off_returns_the_chain_heads_empty_result() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let tavily = Fixture::start(200, "application/json", r#"{"results":[]}"#);
    let firecrawl = Fixture::start_canary();
    let config = format!(
        "{}\n[search]\nfallback = \"off\"\n\n[providers.tavily]\nurl = {:?}\nkeys = [\"tavily-key\"]\ntimeout = 30\n\n[providers.firecrawl]\nurl = {:?}\nkeys = [\"firecrawl-key\"]\ntimeout = 30\n\n[capabilities.web_search]\norder = [\"tavily\", \"firecrawl\"]\n",
        search_config(&main.url, true),
        tavily.url,
        firecrawl.url,
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "search",
        "Current information",
        "--capabilities",
        "web_search",
        "--verbose",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let web_attempts = payload["provider_attempts"]
        .as_array()
        .expect("provider attempts")
        .iter()
        .filter(|attempt| attempt["seam"] == "web_search")
        .map(|attempt| attempt["provider"].as_str().expect("attempt provider"))
        .collect::<Vec<_>>();

    assert_eq!(
        (
            output.status.code(),
            payload.get("extra_sources"),
            &payload["capability_gaps"],
            web_attempts,
        ),
        (Some(0), None, &serde_json::json!([]), vec!["tavily"],),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    main.finish();
    tavily.finish();
    assert!(firecrawl.finish_all().is_empty());
}

#[test]
fn filtered_only_results_do_not_count_as_a_legal_empty_array() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let tavily = Fixture::start(
        200,
        "application/json",
        r#"{"results":[{"url":null},{"title":"missing"},{"url":"  "}]}"#,
    );
    let firecrawl = Fixture::start(200, "application/json", r#"{"data":{}}"#);
    let config = format!(
        "{}\n[providers.tavily]\nurl = {:?}\nkeys = [\"tavily-key\"]\ntimeout = 30\n\n[providers.firecrawl]\nurl = {:?}\nkeys = [\"firecrawl-key\"]\ntimeout = 30\n\n[capabilities.web_search]\norder = [\"tavily\", \"firecrawl\"]\n",
        search_config(&main.url, true),
        tavily.url,
        firecrawl.url,
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "search",
        "Current information",
        "--capabilities",
        "web_search",
        "--verbose",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let error_kinds = payload["provider_attempts"]
        .as_array()
        .expect("provider attempts")
        .iter()
        .filter(|attempt| attempt["seam"] == "web_search")
        .map(|attempt| attempt["error_kind"].as_str().expect("attempt error kind"))
        .collect::<Vec<_>>();

    assert_eq!(
        (
            output.status.code(),
            payload.get("extra_sources"),
            &payload["capability_gaps"],
            error_kinds,
        ),
        (
            Some(0),
            None,
            &serde_json::json!([{
                "capability": "web_search",
                "reason": "all_attempts_failed",
                "providers_skipped": []
            }]),
            vec!["evidence", "runtime"],
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    main.finish();
    tavily.finish();
    firecrawl.finish();
}

#[test]
fn malformed_tavily_results_are_runtime_failures_that_fall_back() {
    for body in [
        r"{}",
        r#"{"results":{}}"#,
        r#"{"results":[{"url":42}]}"#,
        r#"{"results":["not an object"]}"#,
    ] {
        let main = Fixture::start(
            200,
            "text/event-stream",
            &completed_body("answer", "Primary"),
        );
        let tavily = Fixture::start(200, "application/json", body);
        let firecrawl = Fixture::start(
            200,
            "application/json",
            r#"{"data":{"web":[{"url":"https://example.test/fallback"}]}}"#,
        );
        let config = format!(
            "{}\n[providers.tavily]\nurl = {:?}\nkeys = [\"tavily-key\"]\ntimeout = 30\n\n[providers.firecrawl]\nurl = {:?}\nkeys = [\"firecrawl-key\"]\ntimeout = 30\n\n[capabilities.web_search]\norder = [\"tavily\", \"firecrawl\"]\n",
            search_config(&main.url, true),
            tavily.url,
            firecrawl.url,
        );
        let environment = RunEnvironment::new(&config);

        let output = environment.run(&[
            "search",
            "Current information",
            "--capabilities",
            "web_search",
            "--verbose",
        ]);
        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
        let web_attempts = payload["provider_attempts"]
            .as_array()
            .expect("provider attempts")
            .iter()
            .filter(|attempt| attempt["seam"] == "web_search")
            .collect::<Vec<_>>();

        assert_eq!(
            (
                output.status.code(),
                &payload["extra_sources"][0]["url"],
                &web_attempts[0]["error_kind"],
                &web_attempts[1]["provider"],
            ),
            (
                Some(0),
                &Value::String("https://example.test/fallback".into()),
                &Value::String("runtime".into()),
                &Value::String("firecrawl".into()),
            ),
            "body: {body}; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        main.finish();
        tavily.finish();
        firecrawl.finish();
    }
}

#[test]
fn malformed_firecrawl_containers_are_runtime_failures_that_fall_back() {
    for body in [
        r"{}",
        r#"{"data":[]}"#,
        r#"{"data":{}}"#,
        r#"{"data":{"web":{}}}"#,
    ] {
        let main = Fixture::start(
            200,
            "text/event-stream",
            &completed_body("answer", "Primary"),
        );
        let firecrawl = Fixture::start(200, "application/json", body);
        let tavily = Fixture::start(
            200,
            "application/json",
            r#"{"results":[{"url":"https://example.test/fallback"}]}"#,
        );
        let config = format!(
            "{}\n[providers.firecrawl]\nurl = {:?}\nkeys = [\"firecrawl-key\"]\ntimeout = 30\n\n[providers.tavily]\nurl = {:?}\nkeys = [\"tavily-key\"]\ntimeout = 30\n\n[capabilities.web_search]\norder = [\"firecrawl\", \"tavily\"]\n",
            search_config(&main.url, true),
            firecrawl.url,
            tavily.url,
        );
        let environment = RunEnvironment::new(&config);

        let output = environment.run(&[
            "search",
            "Current information",
            "--capabilities",
            "web_search",
            "--verbose",
        ]);
        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
        let web_attempts = payload["provider_attempts"]
            .as_array()
            .expect("provider attempts")
            .iter()
            .filter(|attempt| attempt["seam"] == "web_search")
            .collect::<Vec<_>>();

        assert_eq!(
            (
                output.status.code(),
                &payload["extra_sources"][0]["url"],
                &web_attempts[0]["error_kind"],
                &web_attempts[1]["provider"],
            ),
            (
                Some(0),
                &Value::String("https://example.test/fallback".into()),
                &Value::String("runtime".into()),
                &Value::String("tavily".into()),
            ),
            "body: {body}; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        main.finish();
        firecrawl.finish();
        tavily.finish();
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
fn declared_capability_without_credentials_is_advisory_and_reported_as_a_gap() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let environment = RunEnvironment::new(&search_config(&main.url, false));

    let output = environment.run(&[
        "search",
        "Current information",
        "--capabilities",
        "web_search",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["capability_gaps"],
            String::from_utf8_lossy(&output.stderr).contains("web_search"),
        ),
        (
            Some(0),
            &serde_json::json!([{
                "capability": "web_search",
                "reason": "no_configured_provider",
                "providers_skipped": ["tavily", "firecrawl"]
            }]),
            true,
        )
    );
    main.finish();
}

#[test]
fn declared_web_search_honors_fallback_off_at_the_chain_head() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let config = format!(
        "{}\n[search]\nfallback = \"off\"\n\n[providers.tavily]\nurl = \"http://127.0.0.1:9\"\nkeys = [\"tavily-key\"]\ntimeout = 30\n\n[capabilities.web_search]\norder = [\"firecrawl\", \"tavily\"]\n",
        search_config(&main.url, false),
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "search",
        "Current information",
        "--capabilities",
        "web_search",
        "--verbose",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let providers = payload["provider_attempts"]
        .as_array()
        .expect("provider attempts")
        .iter()
        .map(|attempt| attempt["provider"].as_str().expect("attempt provider"))
        .collect::<Vec<_>>();

    assert_eq!(
        (output.status.code(), providers, &payload["capability_gaps"],),
        (
            Some(0),
            vec!["xai", "firecrawl"],
            &serde_json::json!([{
                "capability": "web_search",
                "reason": "all_attempts_failed",
                "providers_skipped": ["tavily"]
            }]),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    main.finish();
}

#[test]
fn declared_web_fetch_adds_a_content_preview_candidate_without_changing_the_main_answer() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let content = "Fetched evidence line one.\nFetched evidence line two.\n".repeat(12);
    let tavily = Fixture::start(
        200,
        "application/json",
        r#"{"results":[{"raw_content":"thin"}]}"#,
    );
    let firecrawl = Fixture::start(
        200,
        "application/json",
        r#"{"success":true,"data":{"markdown":"also thin"}}"#,
    );
    let jina = Fixture::start(200, "application/json", &jina_response(&content));
    let config = format!(
        "{}\n[providers.tavily]\nurl = {:?}\nkeys = [\"tavily-key\"]\ntimeout = 30\n\n[providers.firecrawl]\nurl = {:?}\nkeys = [\"firecrawl-key\"]\ntimeout = 30\n\n[providers.jina]\nurl = {:?}\nkeys = [\"jina-key\"]\ntimeout = 30\n\n[capabilities.web_fetch]\norder = [\"tavily\", \"firecrawl\", \"jina\"]\n",
        search_config(&main.url, true),
        tavily.url,
        firecrawl.url,
        jina.url
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "search",
        "Verify https://example.test/article",
        "--capabilities",
        "web_fetch",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let journal = read_only_journal(&environment);
    let preview = payload["extra_sources"][0]["summary"]
        .as_str()
        .expect("fetched content preview");

    assert_eq!(
        (
            output.status.code(),
            &payload["answer"],
            &payload["extra_sources"][0]["url"],
            &payload["extra_sources"][0]["provider"],
            preview.chars().count(),
            content.starts_with(preview),
            &payload["capability_gaps"],
            &journal["result"]["extra_sources"],
        ),
        (
            Some(0),
            &Value::String("answer".into()),
            &Value::String("https://example.test/article".into()),
            &Value::String("jina".into()),
            300,
            true,
            &serde_json::json!([]),
            &payload["extra_sources"],
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    main.finish();
    tavily.finish();
    firecrawl.finish();
    let request = jina.finish();
    assert!(
        request.starts_with("GET /https://example.test/article "),
        "{request}"
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
fn declared_web_fetch_reports_a_partial_gap_when_one_target_succeeds() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let content = "Fetched evidence line one.\nFetched evidence line two.\n".repeat(12);
    let jina = Fixture::start_routed(vec![
        Route::new(
            "GET",
            "/https://example.test/one",
            Response::new(200, "application/json", &jina_response(&content)),
        ),
        Route::new(
            "GET",
            "/https://example.test/two",
            Response::new(503, "text/plain", "unavailable"),
        ),
    ]);
    let config = format!(
        "{}\n[providers.jina]\nurl = {:?}\nkeys = [\"jina-key\"]\ntimeout = 30\n\n[capabilities.web_fetch]\norder = [\"jina\"]\n",
        search_config(&main.url, false),
        jina.url
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "search",
        "Verify https://example.test/one https://example.test/two",
        "--capabilities",
        "web_fetch",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let candidate = &payload["extra_sources"][0];

    assert_eq!(
        (
            output.status.code(),
            &candidate["url"],
            &candidate["provider"],
            &payload["capability_gaps"],
        ),
        (
            Some(0),
            &Value::String("https://example.test/one".into()),
            &Value::String("jina".into()),
            &serde_json::json!([{
                "capability": "web_fetch",
                "reason": "partial_failure",
                "providers_skipped": []
            }]),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    main.finish();
    jina.finish_all();
}

#[test]
fn declared_web_fetch_runs_known_urls_concurrently_and_preserves_url_order() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let content = "Fetched evidence line one.\nFetched evidence line two.\n".repeat(12);
    let jina = Fixture::start_synchronized_sequence(vec![
        Response::new(200, "application/json", &jina_response(&content)),
        Response::new(200, "application/json", &jina_response(&content)),
    ]);
    let config = format!(
        "{}\n[providers.jina]\nurl = {:?}\nkeys = [\"jina-key\"]\ntimeout = 30\n\n[capabilities.web_fetch]\norder = [\"jina\"]\n",
        search_config(&main.url, false),
        jina.url
    );
    let environment = RunEnvironment::new(&config);
    let output = environment.run(&[
        "search",
        "Verify https://example.test/one https://example.test/two",
        "--capabilities",
        "web_fetch",
        "--timeout",
        "3",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            payload["extra_sources"]
                .as_array()
                .expect("extra sources")
                .iter()
                .map(|source| source["url"].as_str().expect("source URL"))
                .collect::<Vec<_>>(),
            &payload["capability_gaps"],
        ),
        (
            Some(0),
            vec!["https://example.test/one", "https://example.test/two"],
            &serde_json::json!([]),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    main.finish();
    assert_eq!(jina.finish_all().len(), 2);
}

#[test]
fn concurrent_web_fetch_rate_limits_rotate_once_per_branch_with_a_bounded_burst() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let content = "Rotated evidence line one.\nRotated evidence line two.\n".repeat(12);
    let responses = (0..4)
        .map(|_| {
            Response::new(429, "application/json", r#"{"message":"rate limited"}"#)
                .with_delay(Duration::from_millis(200))
        })
        .chain((0..4).map(|_| Response::new(200, "application/json", &jina_response(&content))))
        .collect();
    let jina = Fixture::start_parallel_sequence(responses);
    let config = format!(
        "{}\n[providers.jina]\nurl = {:?}\nkeys = [\"jina-key-a\", \"jina-key-b\"]\ntimeout = 30\n\n[capabilities.web_fetch]\norder = [\"jina\"]\n",
        search_config(&main.url, false),
        jina.url
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "search",
        concat!(
            "Verify https://example.test/one https://example.test/two ",
            "https://example.test/three https://example.test/four"
        ),
        "--capabilities",
        "web_fetch",
        "--timeout",
        "5",
        "--verbose",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let fetch_attempts = payload["provider_attempts"]
        .as_array()
        .expect("provider attempts")
        .iter()
        .filter(|attempt| attempt["seam"] == "web_fetch")
        .collect::<Vec<_>>();

    assert_eq!(
        (
            output.status.code(),
            payload["extra_sources"].as_array().map(Vec::len),
            fetch_attempts.len(),
            fetch_attempts
                .iter()
                .filter(|attempt| attempt["error_kind"] == "rate_limited")
                .count(),
            fetch_attempts
                .iter()
                .filter_map(|attempt| attempt["rotation_count"].as_u64())
                .max(),
        ),
        (Some(0), Some(4), 8, 4, Some(1)),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    main.finish();
    assert_eq!(jina.finish_all().len(), 8);
}

#[test]
fn combined_declaration_executes_only_declared_seams_in_vocabulary_order() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let exa = Fixture::start(
        200,
        "application/json",
        r#"{"results":[{"title":"Documentation","url":"https://example.test/docs"}]}"#,
    );
    let tavily = Fixture::start(
        200,
        "application/json",
        r#"{"results":[{"title":"Current source","url":"https://example.test/current"}]}"#,
    );
    let config = format!(
        "{}\n[providers.exa]\nurl = {:?}\nkeys = [\"exa-key\"]\ntimeout = 30\n\n[providers.tavily]\nurl = {:?}\nkeys = [\"tavily-key\"]\ntimeout = 30\n\n[providers.jina]\nurl = \"http://127.0.0.1:9\"\nkeys = [\"jina-key\"]\ntimeout = 30\n\n[capabilities.docs_search]\norder = [\"exa\"]\n[capabilities.web_search]\norder = [\"tavily\"]\n[capabilities.web_fetch]\norder = [\"jina\"]\n",
        search_config(&main.url, false),
        exa.url,
        tavily.url,
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "search",
        "Read https://example.test/known",
        "--capabilities",
        "web_search,docs_search",
        "--verbose",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let seams = payload["provider_attempts"]
        .as_array()
        .expect("provider attempts")
        .iter()
        .map(|attempt| attempt["seam"].as_str().expect("attempt seam"))
        .collect::<Vec<_>>();

    assert_eq!(
        (output.status.code(), payload.get("capabilities"), seams,),
        (
            Some(0),
            None,
            vec!["main_search", "docs_search", "web_search"],
        )
    );
    main.finish();
    exa.finish();
    tavily.finish();
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
fn supplemental_capabilities_share_the_deadline_and_merge_in_declaration_order() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let mut fixtures = Fixture::start_synchronized_sequences(vec![
        vec![Response::new(
            200,
            "application/json",
            r#"{"results":[{"title":"Documentation","url":"https://example.test/docs"}]}"#,
        )],
        vec![Response::new(
            200,
            "application/json",
            r#"{"results":[{"title":"Current source","url":"https://example.test/current"}]}"#,
        )],
    ]);
    let tavily = fixtures.pop().expect("Tavily fixture");
    let exa = fixtures.pop().expect("Exa fixture");
    let config = format!(
        "{}\n[providers.exa]\nurl = {:?}\nkeys = [\"exa-key\"]\ntimeout = 30\n\n[providers.tavily]\nurl = {:?}\nkeys = [\"tavily-key\"]\ntimeout = 30\n\n[capabilities.docs_search]\norder = [\"exa\"]\n[capabilities.web_search]\norder = [\"tavily\"]\n",
        search_config(&main.url, false),
        exa.url,
        tavily.url,
    );
    let environment = RunEnvironment::new(&config);
    let output = environment.run(&[
        "search",
        "Compare sources",
        "--capabilities",
        "docs_search,web_search",
        "--timeout",
        "3",
        "--verbose",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let supplemental_attempts = payload["provider_attempts"]
        .as_array()
        .expect("provider attempts")
        .iter()
        .skip(1)
        .map(|attempt| attempt["seam"].as_str().expect("attempt seam"))
        .collect::<Vec<_>>();

    assert_eq!(
        (
            output.status.code(),
            payload["extra_sources"]
                .as_array()
                .expect("extra sources")
                .iter()
                .map(|source| source["url"].as_str().expect("source URL"))
                .collect::<Vec<_>>(),
            supplemental_attempts,
        ),
        (
            Some(0),
            vec!["https://example.test/docs", "https://example.test/current"],
            vec!["docs_search", "web_search"],
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    main.finish();
    exa.finish();
    tavily.finish();
}

#[test]
fn failed_declared_seam_is_advisory_and_preserves_all_attempts() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let tavily = Fixture::start(503, "application/json", r#"{"error":"unavailable"}"#);
    let config = format!(
        "{}\n[providers.tavily]\nurl = {:?}\nkeys = [\"tavily-key\"]\ntimeout = 30\n\n[capabilities.web_search]\norder = [\"tavily\"]\n",
        search_config(&main.url, true),
        tavily.url
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "search",
        "Current information",
        "--capabilities",
        "web_search",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let journal = read_only_journal(&environment);

    assert_eq!(
        (
            output.status.code(),
            &payload["capability_gaps"],
            journal["execution"]["provider_attempts"]
                .as_array()
                .map(Vec::len),
            journal["execution"]["provider_attempts"][1]
                .as_object()
                .is_some_and(|attempt| !attempt.contains_key("model")),
            journal["execution"]["provider_attempts"][1]
                .as_object()
                .is_some_and(|attempt| !attempt.contains_key("endpoint_host")),
            &journal["execution"]["capability_gaps"],
        ),
        (
            Some(0),
            &serde_json::json!([{
                "capability": "web_search",
                "reason": "all_attempts_failed",
                "providers_skipped": []
            }]),
            Some(2),
            true,
            true,
            &payload["capability_gaps"],
        )
    );
    main.finish();
    tavily.finish();
}

#[test]
fn supplemental_search_redacts_urls_in_non_success_responses() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let tavily = Fixture::start(
        400,
        "application/json",
        r#"{"error":"upstream rejected https://user:password@example.test/private?token=response-secret"}"#,
    );
    let config = format!(
        "{}\n[providers.tavily]\nurl = {:?}\nkeys = [\"tavily-key\"]\ntimeout = 30\n\n[capabilities.web_search]\norder = [\"tavily\"]\n",
        search_config(&main.url, false),
        tavily.url
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "search",
        "Current information",
        "--capabilities",
        "web_search",
        "--verbose",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let tavily_attempt = payload["provider_attempts"]
        .as_array()
        .expect("provider attempts")
        .iter()
        .find(|attempt| attempt["provider"] == "tavily")
        .expect("tavily attempt");

    assert_eq!(
        (output.status.code(), &tavily_attempt["message"]),
        (
            Some(0),
            &Value::String(
                r#"{"error":"upstream rejected https://example.test/private?token=********"}"#
                    .into(),
            ),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    main.finish();
    tavily.finish();
}

#[test]
fn search_uses_the_completed_xai_response_and_deduplicates_sources() {
    let answer = concat!(
        "<think>Hidden [[9]](https://hidden.example/path)</think>\n",
        "Final answer [[1]](https://example.test/inline)\n\n",
        "Sources:\n",
        "- [Tail](https://example.test/tail)"
    );
    let fixture = Fixture::start(
        200,
        "text/event-stream",
        &format!(
            ": keepalive\n\ndata:\n\ndata: [DONE]\n\ndata: {}\n\n",
            serde_json::json!({
                "type": "response.completed",
                "response": {"output": [{"content": [{
                    "type": "output_text",
                    "text": answer,
                    "annotations": [{
                        "type": "url_citation",
                        "url": "https://example.test/a?token=secret#fragment",
                        "title": "Source A"
                    }]
                }]}]}
            })
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
            &payload["journal_status"],
        ),
        (
            Some(0),
            &Value::String("Final answer [[1]](https://example.test/inline)".into()),
            &serde_json::json!([
                {
                    "title": "Source A",
                    "url": "https://example.test/a?token=********"
                },
                {"url": "https://example.test/inline"},
                {"title": "Tail", "url": "https://example.test/tail"}
            ]),
            &Value::String("disabled".into()),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(payload.get("provider_attempts").is_none());
    assert!(!payload.to_string().contains("hidden.example"));

    let request = fixture.finish();
    assert!(request.starts_with("POST /v1/responses "), "{request}");
    assert!(
        request.contains("authorization: Bearer xai-key"),
        "{request}"
    );
    assert!(request.contains("\"stream\":true"), "{request}");
    assert!(request.contains("\"model\":\"test-model\""), "{request}");
    let request_body = request_json(&request);
    assert_eq!(
        &request_body["input"][0]["role"],
        &Value::String("user".into())
    );
    assert!(request_body["instructions"].as_str().is_some());
    assert!(
        request_body["input"][0]["content"]
            .as_str()
            .is_some_and(|content| content.contains("What changed?"))
    );
}

#[test]
fn search_fails_when_xai_normalization_removes_the_answer() {
    let xai = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("<think>hidden-only</think>", "Hidden source"),
    );
    let environment = RunEnvironment::new(&search_config(&xai.url, false));

    let output = environment.run(&["search", "Hidden", "--capabilities", "none"]);
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
fn search_preserves_answer_content_while_redacting_sources() {
    let fixture = Fixture::start(
        200,
        "text/event-stream",
        &completed_body(
            "xai-key https://example.test/answer?token=answer-secret",
            "xai-key source title",
        ),
    );
    let environment = RunEnvironment::new(&search_config(&fixture.url, false));

    let output = environment.run(&["search", "Answer exemption", "--capabilities", "none"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["answer"],
            &payload["sources"][0]["title"],
            &payload["sources"][0]["url"],
        ),
        (
            Some(0),
            &Value::String("xai-key https://example.test/answer?token=answer-secret".into()),
            &Value::String("******** source title".into()),
            &Value::String("https://example.test/source?token=********".into()),
        )
    );
    fixture.finish();
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
fn primary_source_without_title_omits_json_title() {
    let fixture = Fixture::start(
        200,
        "text/event-stream",
        concat!(
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"answer\"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"output\":[{\"content\":[",
            "{\"type\":\"output_text\",\"text\":\"answer\",\"annotations\":[",
            "{\"type\":\"url_citation\",\"url\":\"https://example.test/untitled\"}",
            "]}]}]}}\n\n"
        ),
    );
    let environment = RunEnvironment::new(&search_config(&fixture.url, false));

    let output = environment.run(&["search", "Untitled source", "--capabilities", "none"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (output.status.code(), &payload["sources"]),
        (
            Some(0),
            &serde_json::json!([{"url": "https://example.test/untitled"}]),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.finish();
}

#[test]
fn search_fails_when_the_raw_sse_stream_exceeds_four_mibibytes() {
    let body = format!("data: {}\n\n", "x".repeat(4 * 1024 * 1024));
    let fixture = Fixture::start(200, "text/event-stream", &body);
    let environment = RunEnvironment::new(&search_config(&fixture.url, false));

    let output = environment.run(&["search", "Runaway stream", "--capabilities", "none"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            &payload["message"],
        ),
        (
            Some(4),
            &Value::String("runtime".into()),
            &Value::String("response exceeded 4 MiB".into()),
        )
    );
    fixture.finish();
}

#[test]
fn search_rejects_an_xai_sse_body_that_exceeds_four_mibibytes_after_completion() {
    let body = format!(
        "{}data: {}\n\n",
        completed_body("Partial answer", "Partial source"),
        "x".repeat(4 * 1024 * 1024)
    );
    let fixture = Fixture::start(200, "text/event-stream", &body);
    let environment = RunEnvironment::new(&search_config(&fixture.url, false));

    let output = environment.run(&["search", "Oversized tail", "--capabilities", "none"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            &payload["message"],
            payload.get("answer"),
            payload.get("sources"),
        ),
        (
            Some(4),
            &Value::String("runtime".into()),
            &Value::String("response exceeded 4 MiB".into()),
            None,
            None,
        )
    );
    fixture.finish();
}

#[test]
fn search_accepts_a_complete_xai_sse_body_at_four_mibibytes() {
    let completed = completed_body("Boundary answer", "Boundary source");
    let padding = "x".repeat(4 * 1024 * 1024 - completed.len() - 3);
    let body = format!(":{padding}\n\n{completed}");
    let fixture = Fixture::start(200, "text/event-stream", &body);
    let environment = RunEnvironment::new(&search_config(&fixture.url, false));

    let output = environment.run(&["search", "Boundary SSE", "--capabilities", "none"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (body.len(), output.status.code(), &payload["answer"]),
        (
            4 * 1024 * 1024,
            Some(0),
            &Value::String("Boundary answer".into()),
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
fn search_maps_unknown_xai_terminals_missing_completion_and_malformed_frames_to_runtime() {
    for body in [
        "data: {\"type\":\"response.failed\",\"error\":{\"message\":\"upstream failed\"}}\n\n",
        "data: {\"type\":\"response.incomplete\",\"response\":{}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n",
        "data: {not-json}\n\n",
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
fn search_xai_structured_terminals_drive_retry_and_credential_rotation() {
    let retry = Fixture::start_sequence(vec![
        Response::sse(
            200,
            "data: {\"type\":\"response.failed\",\"error\":{\"code\":\"server_error\"}}\n\n",
        ),
        Response::sse(200, &completed_body("Retried answer", "Retried source")),
    ]);
    let retry_environment = RunEnvironment::new(
        &search_config(&retry.url, false).replace("max_attempts = 1", "max_attempts = 2"),
    );

    let retry_output =
        retry_environment.run(&["search", "Retry", "--capabilities", "none", "--verbose"]);
    let retry_payload: Value =
        serde_json::from_slice(&retry_output.stdout).expect("parse retry stdout");

    assert_eq!(
        (
            retry_output.status.code(),
            &retry_payload["answer"],
            &retry_payload["provider_attempts"][0]["error_kind"],
            &retry_payload["provider_attempts"][1]["error_kind"],
        ),
        (
            Some(0),
            &Value::String("Retried answer".into()),
            &Value::String("network".into()),
            &Value::Null,
        )
    );
    assert_eq!(retry.finish_all().len(), 2);

    let rotation = Fixture::start_sequence(vec![
        Response::sse(
            200,
            "data: {\"type\":\"response.failed\",\"error\":{\"code\":\"rate_limit_exceeded\"}}\n\n",
        ),
        Response::sse(200, &completed_body("Rotated answer", "Rotated source")),
    ]);
    let rotation_environment = RunEnvironment::new(
        &search_config(&rotation.url, false)
            .replace(r#"keys = ["xai-key"]"#, r#"keys = ["key-1", "key-2"]"#),
    );

    let rotation_output =
        rotation_environment.run(&["search", "Rotation", "--capabilities", "none", "--verbose"]);
    let rotation_payload: Value =
        serde_json::from_slice(&rotation_output.stdout).expect("parse rotation stdout");

    assert_eq!(
        (
            rotation_output.status.code(),
            &rotation_payload["answer"],
            &rotation_payload["provider_attempts"][0]["error_kind"],
            &rotation_payload["provider_attempts"][1]["rotation_count"],
        ),
        (
            Some(0),
            &Value::String("Rotated answer".into()),
            &Value::String("rate_limited".into()),
            &Value::from(1),
        )
    );
    let requests = rotation.finish_all();
    assert!(requests[0].contains("authorization: Bearer key-1"));
    assert!(requests[1].contains("authorization: Bearer key-2"));
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
fn search_credential_rotation_does_not_consume_network_retry_quota() {
    let xai = Fixture::start_sequence(vec![
        Response::json(429, r#"{"error":"rate limited"}"#),
        Response::json(429, r#"{"error":"rate limited"}"#),
        Response::json(503, r#"{"error":"network failure"}"#),
        Response::json(503, r#"{"error":"network failure"}"#),
    ]);
    let config = search_config(&xai.url, false)
        .replace(
            r#"keys = ["xai-key"]"#,
            r#"keys = ["key-1", "key-2", "key-3"]"#,
        )
        .replace("max_attempts = 1", "max_attempts = 2");
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "search",
        "Independent quotas",
        "--capabilities",
        "none",
        "--verbose",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            &payload["attempts"]["total"],
        ),
        (Some(4), &Value::String("network".into()), &Value::from(4),),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let requests = xai.finish_all();
    assert_eq!(requests.len(), 4);
    assert!(requests[0].contains("authorization: Bearer key-1"));
    assert!(requests[1].contains("authorization: Bearer key-2"));
    assert!(requests[2].contains("authorization: Bearer key-3"));
    assert!(requests[3].contains("authorization: Bearer key-3"));
}

#[test]
fn search_retries_after_a_truncated_response_body() {
    let xai = Fixture::start_sequence(vec![
        Response::sse(200, "").with_declared_content_length(1),
        Response::sse(200, &completed_body("Retried answer", "Retried source")),
    ]);
    let config = search_config(&xai.url, true).replace("max_attempts = 1", "max_attempts = 2");
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "search",
        "Truncated response retry",
        "--capabilities",
        "none",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let journal = read_only_journal(&environment);
    let attempts = journal["execution"]["provider_attempts"]
        .as_array()
        .expect("journal attempts");

    assert_eq!(
        (output.status.code(), &payload["answer"], attempts.len()),
        (Some(0), &Value::String("Retried answer".into()), 2,),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        (
            &attempts[0]["error_kind"],
            &attempts[0]["retry_count"],
            &attempts[1]["error_kind"],
            &attempts[1]["retry_count"],
        ),
        (
            &Value::String("network".into()),
            &Value::from(0),
            &Value::Null,
            &Value::from(1),
        )
    );
    assert_eq!(xai.finish_all().len(), 2);
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

fn tavily_search_config(xai_url: &str, tavily_url: &str) -> String {
    format!(
        "{}\n[providers.tavily]\nurl = {tavily_url:?}\nkeys = [\"tavily-key\"]\ntimeout = 30\n\n[capabilities.web_search]\norder = [\"tavily\"]\n",
        search_config(xai_url, true),
    )
}

#[test]
fn failed_main_search_still_reports_supplemental_candidates() {
    let main = Fixture::start(401, "application/json", r#"{"error":"bad key"}"#);
    let tavily = Fixture::start(
        200,
        "application/json",
        r#"{"results":[{"title":"Candidate","url":"https://example.test/candidate","content":"Candidate summary"}]}"#,
    );
    let environment = RunEnvironment::new(&tavily_search_config(&main.url, &tavily.url));

    let output = environment.run(&[
        "search",
        "Current information",
        "--capabilities",
        "web_search",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let journal = read_only_journal(&environment);

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            &payload["extra_sources"],
            &payload["extra_sources_truncated"],
            &journal["result"]["extra_sources"][0]["summary"],
        ),
        (
            Some(4),
            &Value::String("auth".into()),
            &serde_json::json!([{
                "provider": "tavily",
                "capability": "web_search",
                "title": "Candidate",
                "url": "https://example.test/candidate",
                "provider_data": {}
            }]),
            &Value::Bool(false),
            &Value::String("Candidate summary".into()),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    main.finish();
    tavily.finish();
}

#[test]
fn failed_main_search_bounds_candidates_to_the_failure_payload_target() {
    let main = Fixture::start(401, "application/json", r#"{"error":"bad key"}"#);
    let results = (0..20)
        .map(|index| {
            serde_json::json!({
                "title": format!("{index} {}", "long title ".repeat(30)),
                "url": format!("https://example.test/candidate/{index}")
            })
        })
        .collect::<Vec<_>>();
    let tavily = Fixture::start(
        200,
        "application/json",
        &serde_json::json!({ "results": results }).to_string(),
    );
    let environment = RunEnvironment::new(&tavily_search_config(&main.url, &tavily.url));

    let output = environment.run(&[
        "search",
        "Current information",
        "--capabilities",
        "web_search",
        "--extra-sources",
        "20",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let kept = payload["extra_sources"].as_array().map_or(0, Vec::len);

    assert_eq!(
        (
            output.status.code(),
            &payload["extra_sources_truncated"],
            output.stdout.len() <= 4096,
            (1..20).contains(&kept),
        ),
        (Some(4), &Value::Bool(true), true, true),
        "kept {kept}; stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    main.finish();
    tavily.finish();
}

#[test]
fn supplemental_capabilities_run_while_main_search_is_in_flight() {
    let mut fixtures = Fixture::start_synchronized_sequences(vec![
        vec![Response::new(
            200,
            "text/event-stream",
            &completed_body("answer", "Primary"),
        )],
        vec![Response::new(
            200,
            "application/json",
            r#"{"results":[{"title":"Candidate","url":"https://example.test/candidate"}]}"#,
        )],
    ]);
    let tavily = fixtures.pop().expect("tavily fixture");
    let main = fixtures.pop().expect("main fixture");
    let environment = RunEnvironment::new(&tavily_search_config(&main.url, &tavily.url));

    let output = environment.run(&[
        "search",
        "Current information",
        "--capabilities",
        "web_search",
    ]);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    main.finish();
    tavily.finish();
}

#[test]
fn failed_main_search_reports_supplemental_capability_gaps() {
    let main = Fixture::start(401, "application/json", r#"{"error":"bad key"}"#);
    let environment = RunEnvironment::new(&search_config(&main.url, true));

    let output = environment.run(&[
        "search",
        "Current information",
        "--capabilities",
        "docs_search",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let journal = read_only_journal(&environment);

    assert_eq!(
        (
            output.status.code(),
            &payload["capability_gaps"][0]["capability"],
            &journal["execution"]["capability_gaps"][0]["reason"],
        ),
        (
            Some(4),
            &Value::String("docs_search".into()),
            &Value::String("no_configured_provider".into()),
        ),
        "stdout: {payload}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    main.finish();
}
