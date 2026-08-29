mod support;

use std::fs;
use std::time::{Duration, Instant};

use serde_json::Value;

use support::{Fixture, Response, RunEnvironment, jina_response, request_json};

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
        "VERTICAL_SEARCH,docs_search,vertical_search",
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
            &serde_json::json!(["docs_search", "vertical_search"])
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
fn bare_search_uses_classifier_complete_capability_decision() {
    let classifier = Fixture::start(
        200,
        "application/json",
        r#"{"choices":[{"message":{"content":"{\"required_capabilities\":[\"web_search\"]}"}}]}"#,
    );
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let tavily = Fixture::start(
        200,
        "application/json",
        r#"{"results":[{"title":"Current source","url":"https://example.test/current"}]}"#,
    );
    let config = format!(
        "{}\n[classifier]\nurl = {:?}\nkeys = [\"classifier-key\"]\nmodel = \"classifier-model\"\nfallback_models = []\ntimeout = 30\n\n[providers.tavily]\nurl = {:?}\nkeys = [\"tavily-key\"]\ntimeout = 30\n\n[capabilities.web_search]\norder = [\"tavily\"]\n",
        search_config(&main.url, true),
        classifier.url,
        tavily.url,
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["search", "What changed today?", "--verbose"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let journal = read_only_journal(&environment);

    assert_eq!(
        (
            output.status.code(),
            &payload["extra_sources"][0]["url"],
            &journal["execution"]["plan_summary"]["capabilities"],
            &journal["execution"]["plan_summary"]["source"],
        ),
        (
            Some(0),
            &Value::String("https://example.test/current".into()),
            &serde_json::json!(["web_search"]),
            &Value::String("classifier".into()),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let classifier_request = classifier.finish();
    assert!(
        classifier_request.starts_with("POST /chat/completions "),
        "{classifier_request}"
    );
    assert!(
        classifier_request.contains("authorization: Bearer classifier-key"),
        "{classifier_request}"
    );
    assert!(classifier_request.contains("\"model\":\"classifier-model\""));
    main.finish();
    tavily.finish();
}

#[test]
fn bare_search_without_classifier_uses_default_web_search_chain() {
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

    let output = environment.run(&["search", "Ordinary bare search"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let journal = read_only_journal(&environment);

    assert_eq!(
        (
            output.status.code(),
            &payload["extra_sources"][0]["url"],
            &journal["execution"]["plan_summary"],
            output.stderr.is_empty(),
        ),
        (
            Some(0),
            &Value::String("https://example.test/default".into()),
            &serde_json::json!({
                "source": "default_web_search",
                "capabilities": ["web_search"],
                "classifier_degraded": false
            }),
            true,
        )
    );
    main.finish();
    let request = tavily.finish();
    let request_body = request_json(&request);
    assert_eq!(request_body["max_results"], 3);
}

#[test]
fn classifier_invalid_schema_degrades_with_warning_and_journal_trace() {
    let classifier = Fixture::start(
        200,
        "application/json",
        r#"{"choices":[{"message":{"content":"{\"required_capabilities\":[\"provider:tavily\"]}"}}]}"#,
    );
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let tavily = Fixture::start(
        200,
        "application/json",
        r#"{"results":[{"title":"Degraded source","url":"https://example.test/degraded"}]}"#,
    );
    let config = format!(
        "{}\n[classifier]\nurl = {:?}\nkeys = [\"classifier-key\"]\nmodel = \"classifier-model\"\ntimeout = 30\n\n[providers.tavily]\nurl = {:?}\nkeys = [\"tavily-key\"]\ntimeout = 30\n\n[capabilities.web_search]\norder = [\"tavily\"]\n",
        search_config(&main.url, true),
        classifier.url,
        tavily.url,
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["search", "Bare search after invalid classifier"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let journal = read_only_journal(&environment);

    assert_eq!(
        (
            output.status.code(),
            &payload["extra_sources"][0]["url"],
            &journal["execution"]["plan_summary"]["capabilities"],
            &journal["execution"]["plan_summary"]["source"],
            &journal["execution"]["plan_summary"]["classifier_degraded"],
            journal["execution"]["provider_attempts"]
                .as_array()
                .and_then(|attempts| attempts.first())
                .and_then(|attempt| attempt["provider"].as_str()),
        ),
        (
            Some(0),
            &Value::String("https://example.test/degraded".into()),
            &serde_json::json!(["web_search"]),
            &Value::String("classifier_degraded".into()),
            &Value::Bool(true),
            Some("classifier"),
        )
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Classifier warning"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    classifier.finish();
    main.finish();
    tavily.finish();
}

#[test]
fn classifier_rejects_duplicate_capabilities_and_degrades_to_web_search() {
    let classifier = Fixture::start(
        200,
        "application/json",
        r#"{"choices":[{"message":{"content":"{\"required_capabilities\":[\"docs_search\",\"docs_search\"]}"}}]}"#,
    );
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let tavily = Fixture::start(
        200,
        "application/json",
        r#"{"results":[{"title":"Fallback source","url":"https://example.test/fallback"}]}"#,
    );
    let config = format!(
        "{}\n[classifier]\nurl = {:?}\nkeys = [\"classifier-key\"]\nmodel = \"classifier-model\"\ntimeout = 30\n\n[providers.tavily]\nurl = {:?}\nkeys = [\"tavily-key\"]\ntimeout = 30\n\n[capabilities.web_search]\norder = [\"tavily\"]\n",
        search_config(&main.url, true),
        classifier.url,
        tavily.url,
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["search", "Duplicate classifier decision"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let journal = read_only_journal(&environment);

    assert_eq!(
        (
            output.status.code(),
            &payload["extra_sources"][0]["url"],
            &journal["execution"]["plan_summary"]["capabilities"],
            &journal["execution"]["plan_summary"]["source"],
        ),
        (
            Some(0),
            &Value::String("https://example.test/fallback".into()),
            &serde_json::json!(["web_search"]),
            &Value::String("classifier_degraded".into()),
        )
    );
    classifier.finish();
    main.finish();
    tavily.finish();
}

#[test]
fn invalid_classifier_content_does_not_leak_credentials_through_diagnostics_or_journal() {
    let secret = "classifier-canary-secret";
    let classifier = Fixture::start(
        200,
        "application/json",
        &format!(
            r#"{{"choices":[{{"message":{{"content":"{{\"required_capabilities\":[\"{secret}\"]}}"}}}}]}}"#
        ),
    );
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let config = format!(
        "{}\n[classifier]\nurl = {:?}\nkeys = [{secret:?}]\nmodel = \"classifier-model\"\ntimeout = 30\n",
        search_config(&main.url, true),
        classifier.url,
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["search", "Secret-bearing invalid classifier response"]);
    let journal = read_only_journal(&environment).to_string();

    for (name, content) in [
        (
            "stdout",
            String::from_utf8_lossy(&output.stdout).into_owned(),
        ),
        (
            "stderr",
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ),
        ("journal", journal),
    ] {
        assert!(!content.contains(secret), "{name} leaked classifier secret");
    }
    classifier.finish();
    main.finish();
}

#[test]
fn classifier_transport_failure_degrades_without_changing_search_terminal() {
    let classifier = Fixture::start(
        503,
        "application/json",
        r#"{"error":"classifier unavailable"}"#,
    );
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let tavily = Fixture::start(
        200,
        "application/json",
        r#"{"results":[{"title":"Fallback source","url":"https://example.test/fallback"}]}"#,
    );
    let config = format!(
        "{}\n[classifier]\nurl = {:?}\nkeys = [\"classifier-key\"]\nmodel = \"classifier-model\"\ntimeout = 30\n\n[providers.tavily]\nurl = {:?}\nkeys = [\"tavily-key\"]\ntimeout = 30\n\n[capabilities.web_search]\norder = [\"tavily\"]\n",
        search_config(&main.url, true),
        classifier.url,
        tavily.url,
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["search", "Bare search after classifier outage"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let journal = read_only_journal(&environment);

    assert_eq!(
        (
            output.status.code(),
            &payload["answer"],
            &journal["execution"]["plan_summary"]["capabilities"],
            &journal["execution"]["plan_summary"]["classifier_degraded"],
            &journal["execution"]["provider_attempts"][0]["error_kind"],
        ),
        (
            Some(0),
            &Value::String("answer".into()),
            &serde_json::json!(["web_search"]),
            &Value::Bool(true),
            &Value::String("network".into()),
        )
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("Classifier warning"));
    classifier.finish();
    main.finish();
    tavily.finish();
}

#[test]
fn classifier_rotates_credentials_and_uses_fallback_models_even_when_search_fallback_is_off() {
    let classifier = Fixture::start_sequence(vec![
        Response::new(429, "application/json", r#"{"error":"rate limited"}"#),
        Response::new(
            503,
            "application/json",
            r#"{"error":"primary unavailable"}"#,
        ),
        Response::new(
            200,
            "application/json",
            r#"{"choices":[{"message":{"content":"{\"required_capabilities\":[]}"}}]}"#,
        ),
    ]);
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let config = format!(
        "{}\n[search]\nfallback = \"off\"\n\n[classifier]\nurl = {:?}\nkeys = [\"classifier-key-a\", \"classifier-key-b\"]\nmodel = \"primary-classifier\"\nfallback_models = [\"fallback-classifier\"]\ntimeout = 30\n",
        search_config(&main.url, true),
        classifier.url,
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["search", "Classify with fallback", "--verbose"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let attempts = payload["provider_attempts"]
        .as_array()
        .expect("provider attempts");

    assert_eq!(
        (
            output.status.code(),
            attempts[0]["model"].as_str(),
            attempts[0]["rotation_count"].as_u64(),
            attempts[1]["model"].as_str(),
            attempts[2]["model"].as_str(),
        ),
        (
            Some(0),
            Some("primary-classifier"),
            Some(0),
            Some("primary-classifier"),
            Some("fallback-classifier"),
        )
    );
    let requests = classifier.finish_all();
    assert!(requests[0].contains("authorization: Bearer classifier-key-a"));
    assert!(requests[1].contains("authorization: Bearer classifier-key-b"));
    assert!(requests[2].contains("\"model\":\"fallback-classifier\""));
    main.finish();
}

#[test]
fn classifier_skips_a_model_when_its_budget_slice_is_below_five_seconds() {
    let classifier = Fixture::start(
        200,
        "application/json",
        r#"{"choices":[{"message":{"content":"{\"required_capabilities\":[]}"}}]}"#,
    );
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let config = format!(
        "{}\n[classifier]\nurl = {:?}\nkeys = [\"classifier-key\"]\nmodel = \"skipped-model\"\nfallback_models = [\"executable-model\"]\ntimeout = 9\n",
        search_config(&main.url, false),
        classifier.url,
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["search", "Budgeted classification", "--verbose"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let attempts = payload["provider_attempts"]
        .as_array()
        .expect("provider attempts");

    assert_eq!(
        (
            output.status.code(),
            attempts[0]["provider"].as_str(),
            attempts[0]["model"].as_str(),
            attempts[0]["duration_ms"].as_u64(),
            attempts[1]["model"].as_str(),
        ),
        (
            Some(0),
            Some("classifier"),
            Some("skipped-model"),
            Some(0),
            Some("executable-model"),
        )
    );
    let request = classifier.finish();
    assert!(!request.contains("\"model\":\"skipped-model\""));
    assert!(request.contains("\"model\":\"executable-model\""));
    main.finish();
}

#[test]
fn caller_capability_declaration_skips_configured_classifier() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let config = format!(
        "{}\n[classifier]\nurl = \"http://127.0.0.1:9\"\nkeys = [\"classifier-key\"]\nmodel = \"classifier-model\"\ntimeout = 30\n",
        search_config(&main.url, true),
    );
    let environment = RunEnvironment::new(&config);

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
            &journal["execution"]["plan_summary"]["capabilities"],
            &journal["execution"]["plan_summary"]["source"],
            output.stderr.is_empty(),
        ),
        (
            Some(0),
            Some(1),
            &serde_json::json!([]),
            &Value::String("caller".into()),
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
fn non_http_tavily_url_falls_back_to_a_valid_firecrawl_candidate() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let tavily = Fixture::start(
        200,
        "application/json",
        r#"{"results":[{"title":"Invalid","url":"ftp://example.test/result"}]}"#,
    );
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
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    main.finish();
    tavily.finish();
    firecrawl.finish();
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
fn declared_docs_search_uses_the_configured_registry_chain() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let exa = Fixture::start(
        200,
        "application/json",
        r#"{"results":[{"title":"Rust documentation","url":"https://doc.rust-lang.org/book/"}]}"#,
    );
    let config = format!(
        "{}\n[providers.exa]\nurl = {:?}\nkeys = [\"exa-key\"]\ntimeout = 30\n\n[capabilities.docs_search]\norder = [\"exa\"]\n",
        search_config(&main.url, false),
        exa.url
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "search",
        "Rust ownership documentation",
        "--capabilities",
        "docs_search",
        "--extra-sources",
        "0",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["extra_sources"],
            &payload["capability_gaps"],
        ),
        (
            Some(0),
            &serde_json::json!([{
                "provider": "exa",
                "capability": "docs_search",
                "title": "Rust documentation",
                "url": "https://doc.rust-lang.org/book/",
                "summary": null,
                "provider_data": {}
            }]),
            &serde_json::json!([]),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    main.finish();
    let request = exa.finish();
    assert!(request.starts_with("POST /search "), "{request}");
    assert!(request.contains(r#""type":"auto""#), "{request}");
    assert!(request.contains(r#""text":false"#), "{request}");
    assert!(request.contains(r#""highlights":true"#), "{request}");
    assert!(request.contains(r#""numResults":1"#), "{request}");
    assert!(!request.contains("useAutoprompt"), "{request}");
}

#[test]
fn declared_docs_search_filters_invalid_libraries_before_applying_the_limit() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let context7 = Fixture::start_sequence(vec![
        Response::new(
            200,
            "application/json",
            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{}}}"#,
        )
        .with_header("mcp-session-id", "library-session"),
        Response::new(202, "application/json", ""),
        Response::new(
            200,
            "application/json",
            r#"{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"results":[{"title":"Invalid","description":"Missing library id"},{"id":"/rust-lang/rust","title":"Rust","description":"Rust docs","totalSnippets":1234,"trustScore":9.8,"benchmarkScore":91,"stars":100000,"versions":["1.85","1.84"]}]},"content":[]}}"#,
        ),
    ]);
    let exa = Fixture::start_canary();
    let config = format!(
        "{}\n[providers.context7]\nurl = {:?}\nkeys = [\"context7-key\"]\ntimeout = 30\n\n[providers.exa]\nurl = {:?}\nkeys = [\"exa-key\"]\ntimeout = 30\n\n[capabilities.docs_search]\norder = [\"context7\", \"exa\"]\n",
        search_config(&main.url, false),
        context7.url,
        exa.url,
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "search",
        "Rust ownership",
        "--capabilities",
        "docs_search",
        "--verbose",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["extra_sources"],
            &payload["capability_gaps"],
            payload["provider_attempts"].as_array().map(Vec::len),
        ),
        (
            Some(0),
            &serde_json::json!([{
                "provider": "context7",
                "capability": "docs_search",
                "title": "Rust",
                "url": null,
                "summary": "Rust docs",
                "provider_data": {
                    "library_id": "/rust-lang/rust",
                    "total_snippets": 1234,
                    "trust_score": 9.8,
                    "benchmark_score": 91.0,
                    "stars": 100_000,
                    "versions": ["1.85", "1.84"]
                }
            }]),
            &serde_json::json!([]),
            Some(2),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    main.finish();
    let requests = context7.finish_all();
    assert_eq!(requests.len(), 3);
    assert!(requests[2].contains(r#""name":"resolve-library-id""#));
    assert!(exa.finish_all().is_empty());
}

#[test]
fn declared_docs_search_normalizes_a_text_context7_library_candidate() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let context7 = Fixture::start_sequence(vec![
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{}}}"#,
        )
        .with_session("text-library-session"),
        Response::json(202, ""),
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"- Title: Tokio\n- Context7-compatible library ID: /tokio-rs/tokio\n- Description: Async runtime\n- Code Snippets: 1,234\n- Trust Score: 9.7\n- Benchmark Score: 91\n- Stars: 28000\n- Versions: 1.47, 1.46"}]}}"#,
        ),
    ]);
    let config = format!(
        "{}\n[providers.context7]\nurl = {:?}\nkeys = [\"context7-key\"]\ntimeout = 30\n\n[capabilities.docs_search]\norder = [\"context7\"]\n",
        search_config(&main.url, false),
        context7.url
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["search", "Tokio runtime", "--capabilities", "docs_search"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (output.status.code(), &payload["extra_sources"]),
        (
            Some(0),
            &serde_json::json!([{
                "provider": "context7",
                "capability": "docs_search",
                "title": "Tokio",
                "url": null,
                "summary": "Async runtime",
                "provider_data": {
                    "library_id": "/tokio-rs/tokio",
                    "total_snippets": 1234,
                    "trust_score": 9.7,
                    "benchmark_score": 91.0,
                    "stars": 28000,
                    "versions": ["1.47", "1.46"]
                }
            }]),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    main.finish();
    let requests = context7.finish_all();
    assert_eq!(requests.len(), 3);
    assert!(requests[2].contains(r#""name":"resolve-library-id""#));
}

#[test]
fn declared_docs_search_falls_back_to_exa_when_context7_lacks_a_library_id() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let context7 = Fixture::start_sequence(vec![
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{},"serverInfo":{"name":"context7","version":"1"}}}"#,
        )
        .with_session("library-session"),
        Response::json(202, ""),
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"results":[{"title":"Rust","description":"Missing identity"}]},"content":[]}}"#,
        ),
    ]);
    let exa = Fixture::start(
        200,
        "application/json",
        r#"{"requestId":"wrapper-must-not-leak","results":[{"id":"exa-result-id","title":"Rust ownership","url":"https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html","publishedDate":"2026-08-06","author":"Rust Project","text":"Full body must not leak","highlights":["Ownership summary","Secondary detail"],"image":"https://example.test/image.png","favicon":"https://example.test/favicon.ico","unknownField":"must not leak"}]}"#,
    );
    let config = format!(
        "{}\n[providers.context7]\nurl = {:?}\nkeys = [\"context7-key\"]\ntimeout = 30\n\n[providers.exa]\nurl = {:?}\nkeys = [\"exa-key\"]\ntimeout = 30\n\n[capabilities.docs_search]\norder = [\"context7\", \"exa\"]\n",
        search_config(&main.url, false),
        context7.url,
        exa.url,
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "search",
        "Rust ownership",
        "--capabilities",
        "docs_search",
        "--verbose",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let docs_attempt_providers = payload["provider_attempts"]
        .as_array()
        .expect("provider attempts")
        .iter()
        .filter(|attempt| attempt["seam"] == "docs_search")
        .map(|attempt| attempt["provider"].as_str().expect("attempt provider"))
        .collect::<Vec<_>>();

    assert_eq!(
        (
            output.status.code(),
            &payload["extra_sources"],
            docs_attempt_providers,
        ),
        (
            Some(0),
            &serde_json::json!([{
                "provider": "exa",
                "capability": "docs_search",
                "title": "Rust ownership",
                "url": "https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html",
                "summary": "Ownership summary",
                "provider_data": {
                    "id": "exa-result-id",
                    "highlights": ["Ownership summary", "Secondary detail"],
                    "published_date": "2026-08-06",
                    "author": "Rust Project",
                    "image": "https://example.test/image.png",
                    "favicon": "https://example.test/favicon.ico"
                }
            }]),
            vec!["context7", "exa"],
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    main.finish();
    context7.finish_all();
    let exa_request = exa.finish();
    assert!(exa_request.contains(r#""type":"auto""#), "{exa_request}");
    assert!(exa_request.contains(r#""text":false"#), "{exa_request}");
    assert!(
        exa_request.contains(r#""highlights":true"#),
        "{exa_request}"
    );
    assert!(!exa_request.contains("useAutoprompt"), "{exa_request}");
}

#[test]
fn declared_docs_search_falls_back_from_an_exa_candidate_without_an_http_url() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let exa = Fixture::start(
        200,
        "application/json",
        r#"{"results":[{"title":"Invalid","url":"exa:result-id"}]}"#,
    );
    let context7 = Fixture::start_sequence(vec![
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{}}}"#,
        )
        .with_session("fallback-library-session"),
        Response::json(202, ""),
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"results":[{"id":"/tokio-rs/tokio","title":"Tokio","description":"Async runtime"}]},"content":[]}}"#,
        ),
    ]);
    let config = format!(
        "{}\n[providers.exa]\nurl = {:?}\nkeys = [\"exa-key\"]\ntimeout = 30\n\n[providers.context7]\nurl = {:?}\nkeys = [\"context7-key\"]\ntimeout = 30\n\n[capabilities.docs_search]\norder = [\"exa\", \"context7\"]\n",
        search_config(&main.url, false),
        exa.url,
        context7.url,
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "search",
        "Tokio runtime",
        "--capabilities",
        "docs_search",
        "--verbose",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let docs_attempt_providers = payload["provider_attempts"]
        .as_array()
        .expect("provider attempts")
        .iter()
        .filter(|attempt| attempt["seam"] == "docs_search")
        .map(|attempt| attempt["provider"].as_str().expect("attempt provider"))
        .collect::<Vec<_>>();

    assert_eq!(
        (
            output.status.code(),
            &payload["extra_sources"],
            docs_attempt_providers,
        ),
        (
            Some(0),
            &serde_json::json!([{
                "provider": "context7",
                "capability": "docs_search",
                "title": "Tokio",
                "url": null,
                "summary": "Async runtime",
                "provider_data": {
                    "library_id": "/tokio-rs/tokio"
                }
            }]),
            vec!["exa", "context7"],
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    main.finish();
    exa.finish();
    context7.finish_all();
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
            payload.get("validation_results"),
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
            None,
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
    let jina = Fixture::start_sequence(vec![
        Response::new(200, "application/json", &jina_response(&content)),
        Response::new(503, "text/plain", "unavailable"),
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
    assert_eq!(jina.finish_all().len(), 2);
}

#[test]
fn declared_web_fetch_runs_known_urls_concurrently_and_preserves_url_order() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let content = "Fetched evidence line one.\nFetched evidence line two.\n".repeat(12);
    let jina = Fixture::start_parallel_sequence(vec![
        Response::new(200, "application/json", &jina_response(&content))
            .with_delay(Duration::from_secs(2)),
        Response::new(200, "application/json", &jina_response(&content))
            .with_delay(Duration::from_secs(2)),
    ]);
    let config = format!(
        "{}\n[providers.jina]\nurl = {:?}\nkeys = [\"jina-key\"]\ntimeout = 30\n\n[capabilities.web_fetch]\norder = [\"jina\"]\n",
        search_config(&main.url, false),
        jina.url
    );
    let environment = RunEnvironment::new(&config);
    let started = Instant::now();

    let output = environment.run(&[
        "search",
        "Verify https://example.test/one https://example.test/two",
        "--capabilities",
        "web_fetch",
        "--timeout",
        "3",
    ]);
    let elapsed = started.elapsed();
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
        "elapsed: {elapsed:?}; stderr: {}",
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
fn declared_vertical_search_uses_the_unified_candidate_envelope() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let anysearch = Fixture::start_sequence(vec![
        Response::new(
            200,
            "application/json",
            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{}}}"#,
        )
        .with_header("mcp-session-id", "vertical-session"),
        Response::new(202, "application/json", ""),
        Response::new(
            200,
            "application/json",
            r####"{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"### 1. Academic result\n- **URL**: https://example.test/paper\nPaper summary"}]}}"####,
        ),
    ]);
    let tavily = Fixture::start(
        200,
        "application/json",
        r#"{"results":[{"title":"Supplemental duplicate","url":"https://example.test/paper","content":"Provider-native summary"}]}"#,
    );
    let config = format!(
        "{}\n[providers.anysearch]\nurl = {:?}\nkeys = [\"anysearch-key\"]\ntimeout = 30\n\n[providers.tavily]\nurl = {:?}\nkeys = [\"tavily-key\"]\ntimeout = 30\n\n[capabilities.web_search]\norder = [\"tavily\"]\n",
        search_config(&main.url, true),
        anysearch.url,
        tavily.url
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "search",
        "Academic retrieval paper",
        "--capabilities",
        "web_search,vertical_search",
        "--extra-sources",
        "0",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let journal = read_only_journal(&environment);

    assert_eq!(
        (
            output.status.code(),
            &payload["extra_sources"],
            payload.get("vertical_results"),
            payload["sources"].as_array().map(Vec::len),
            &payload["capability_gaps"],
            journal["result"].get("vertical_results"),
            &journal["result"]["extra_sources"],
        ),
        (
            Some(0),
            &serde_json::json!([{
                "provider": "anysearch",
                "capability": "vertical_search",
                "title": "Academic result",
                "url": "https://example.test/paper",
                "summary": "Paper summary",
                "provider_data": {}
            }]),
            None,
            Some(1),
            &serde_json::json!([]),
            None,
            &serde_json::json!([{
                "provider": "anysearch",
                "capability": "vertical_search",
                "title": "Academic result",
                "url": "https://example.test/paper",
                "summary": "Paper summary",
                "provider_data": {}
            }]),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    main.finish();
    let requests = anysearch.finish_all();
    tavily.finish();
    assert!(requests[2].contains(r#""name":"search""#));
    assert!(requests[2].contains(r#""max_results":1"#));
    assert!(!requests[2].contains(r#""domain""#));
}

#[test]
fn search_markdown_renders_vertical_candidates_as_extra_sources() {
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("answer", "Primary"),
    );
    let anysearch = Fixture::start_sequence(vec![
        Response::new(
            200,
            "application/json",
            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{}}}"#,
        )
        .with_header("mcp-session-id", "vertical-session"),
        Response::new(202, "application/json", ""),
        Response::new(
            200,
            "application/json",
            r####"{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"### 1. Academic result\n- **URL**: https://example.test/paper\nPaper summary"}]}}"####,
        ),
    ]);
    let config = format!(
        "{}\n[providers.anysearch]\nurl = {:?}\nkeys = [\"anysearch-key\"]\ntimeout = 30\n",
        search_config(&main.url, false),
        anysearch.url
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "search",
        "Academic retrieval paper",
        "--capabilities",
        "vertical_search",
        "--format",
        "markdown",
    ]);
    let markdown = String::from_utf8(output.stdout).expect("UTF-8 markdown");

    assert_eq!(output.status.code(), Some(0));
    assert!(markdown.contains("## Extra Sources"), "{markdown}");
    assert!(
        markdown.contains(
            "[Academic result](https://example.test/paper) — anysearch\n\n  Paper summary"
        ),
        "{markdown}"
    );
    assert!(!markdown.contains("## Vertical Results"), "{markdown}");
    main.finish();
    anysearch.finish_all();
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
        (
            output.status.code(),
            payload.get("capabilities"),
            seams,
            payload.get("validation_results"),
            payload.get("vertical_results"),
        ),
        (
            Some(0),
            None,
            vec!["main_search", "docs_search", "web_search"],
            None,
            None,
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
    let exa = Fixture::start_sequence(vec![
        Response::new(
            200,
            "application/json",
            r#"{"results":[{"title":"Documentation","url":"https://example.test/docs"}]}"#,
        )
        .with_delay(Duration::from_secs(2)),
    ]);
    let tavily = Fixture::start_sequence(vec![
        Response::new(
            200,
            "application/json",
            r#"{"results":[{"title":"Current source","url":"https://example.test/current"}]}"#,
        )
        .with_delay(Duration::from_secs(2)),
    ]);
    let config = format!(
        "{}\n[providers.exa]\nurl = {:?}\nkeys = [\"exa-key\"]\ntimeout = 30\n\n[providers.tavily]\nurl = {:?}\nkeys = [\"tavily-key\"]\ntimeout = 30\n\n[capabilities.docs_search]\norder = [\"exa\"]\n[capabilities.web_search]\norder = [\"tavily\"]\n",
        search_config(&main.url, false),
        exa.url,
        tavily.url,
    );
    let environment = RunEnvironment::new(&config);
    let started = Instant::now();

    let output = environment.run(&[
        "search",
        "Compare sources",
        "--capabilities",
        "docs_search,web_search",
        "--timeout",
        "3",
        "--verbose",
    ]);
    let elapsed = started.elapsed();
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
        "elapsed: {elapsed:?}; stderr: {}",
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
fn search_openai_stream_and_non_stream_share_completed_answer_normalization() {
    let answer = concat!(
        "<THINK>private</think>\n",
        "Shared answer [[1]](https://example.test/inline)\n\n",
        "References\n",
        "- https://example.test/tail?token=one"
    );
    let citations = serde_json::json!([
        {"url": "https://example.test/structured#fragment", "title": "Structured"},
        {"url": "https://example.test/structured", "title": "Duplicate"}
    ]);
    let non_stream = serde_json::json!({
        "choices": [{"message": {"content": answer, "citations": citations}}]
    })
    .to_string();
    let stream = format!(
        "data: {}\n\ndata: {}\n\n",
        serde_json::json!({
            "choices": [{"delta": {
                "content": "<THI",
                "citations": citations
            }}]
        }),
        serde_json::json!({
            "choices": [{
                "delta": {"content": &answer[4..]},
                "finish_reason": "stop"
            }]
        })
    );
    let openai = Fixture::start_sequence(vec![
        Response::new(200, "application/json", &non_stream),
        Response::new(200, "text/event-stream", &stream),
    ]);
    let config = main_fallback_config("http://127.0.0.1:9", &openai.url, false, &[], "auto", false)
        .replace(
            r#"backends = ["xai", "openai_compatible"]"#,
            r#"backends = ["openai_compatible"]"#,
        );
    let environment = RunEnvironment::new(&config);

    let non_stream_output = environment.run(&[
        "search",
        "Non-stream normalization",
        "--capabilities",
        "none",
    ]);
    let stream_output = environment.run_with_env(
        &["search", "Stream normalization", "--capabilities", "none"],
        &[("FORAGER_PROVIDERS__OPENAI_COMPATIBLE__STREAM", "true")],
    );
    let non_stream_payload: Value =
        serde_json::from_slice(&non_stream_output.stdout).expect("parse non-stream JSON");
    let stream_payload: Value =
        serde_json::from_slice(&stream_output.stdout).expect("parse stream JSON");
    let expected_sources = serde_json::json!([
        {"title": "Structured", "url": "https://example.test/structured"},
        {"url": "https://example.test/inline"},
        {"url": "https://example.test/tail?token=********"}
    ]);

    assert_eq!(non_stream_output.status.code(), Some(0));
    assert_eq!(stream_output.status.code(), Some(0));
    for payload in [&non_stream_payload, &stream_payload] {
        assert_eq!(
            (&payload["answer"], &payload["sources"]),
            (
                &Value::String("Shared answer [[1]](https://example.test/inline)".into()),
                &expected_sources,
            )
        );
    }
    assert_eq!(openai.finish_all().len(), 2);
}

#[test]
fn search_falls_back_when_xai_normalization_removes_the_answer() {
    let xai = Fixture::start(
        200,
        "text/event-stream",
        &completed_body("<think>hidden-only</think>", "Hidden source"),
    );
    let openai = Fixture::start(
        200,
        "application/json",
        r#"{"choices":[{"message":{"content":"Fallback answer"}}]}"#,
    );
    let environment = RunEnvironment::new(&main_fallback_config(
        &xai.url,
        &openai.url,
        false,
        &[],
        "auto",
        false,
    ));

    let output = environment.run(&["search", "Fallback", "--capabilities", "none"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["answer"],
            &payload["sources"]
        ),
        (
            Some(0),
            &Value::String("Fallback answer".into()),
            &serde_json::json!([]),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    xai.finish();
    openai.finish();
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
fn search_openai_compatible_returns_no_partial_answer_when_responses_exceed_four_mibibytes() {
    let stream_body = format!("data: {}\n\n", "x".repeat(4 * 1024 * 1024));
    let buffered_result = r#"{"choices":[{"message":{"content":"Partial answer"}}]}"#;
    let buffered_body = format!("{buffered_result}{}", " ".repeat(4 * 1024 * 1024));
    let openai = Fixture::start_sequence(vec![
        Response::new(200, "text/event-stream", &stream_body),
        Response::json(200, &buffered_body),
    ]);
    let config = main_fallback_config("http://127.0.0.1:9", &openai.url, true, &[], "auto", false)
        .replace(
            r#"backends = ["xai", "openai_compatible"]"#,
            r#"backends = ["openai_compatible"]"#,
        );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "search",
        "Oversized OpenAI",
        "--capabilities",
        "none",
        "--verbose",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            &payload["message"],
            &payload["attempts"]["total"],
            &payload["provider_attempts"][1]["breaker_event"],
            payload.get("diagnostic"),
            payload.get("answer"),
            payload.get("sources"),
        ),
        (
            Some(4),
            &Value::String("runtime".into()),
            &Value::String("response exceeded 4 MiB".into()),
            &Value::from(2),
            &Value::String("transport_fallback".into()),
            None,
            None,
            None,
        )
    );
    assert_eq!(openai.finish_all().len(), 2);
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
fn search_falls_back_when_xai_completed_response_has_no_output_text() {
    let xai = Fixture::start(
        200,
        "text/event-stream",
        concat!(
            "data: {\"type\":\"response.completed\",\"response\":{\"output\":[",
            "{\"content\":[{\"type\":\"reasoning\",\"text\":\"hidden\"}]}]}}\n\n"
        ),
    );
    let openai = Fixture::start(
        200,
        "application/json",
        r#"{"choices":[{"message":{"content":"Fallback answer"}}]}"#,
    );
    let environment = RunEnvironment::new(&main_fallback_config(
        &xai.url,
        &openai.url,
        false,
        &[],
        "auto",
        false,
    ));

    let output = environment.run(&["search", "Fallback", "--capabilities", "none"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (output.status.code(), &payload["answer"]),
        (Some(0), &Value::String("Fallback answer".into())),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    xai.finish();
    openai.finish();
}

#[test]
fn search_falls_back_from_xai_to_openai_compatible_non_stream() {
    let xai = Fixture::start(503, "application/json", r#"{"error":"xAI unavailable"}"#);
    let openai = Fixture::start(
        200,
        "application/json",
        r#"{"choices":[{"message":{"content":"Fallback answer","citations":[{"url":"https://example.test/fallback","title":"Fallback source"}]}}]}"#,
    );
    let environment = RunEnvironment::new(&main_fallback_config(
        &xai.url,
        &openai.url,
        false,
        &[],
        "auto",
        false,
    ));

    let output = environment.run(&["search", "Fallback", "--capabilities", "none"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["answer"],
            &payload["sources"],
        ),
        (
            Some(0),
            &Value::String("Fallback answer".into()),
            &serde_json::json!([{
                "title": "Fallback source",
                "url": "https://example.test/fallback"
            }]),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(xai.finish().starts_with("POST /v1/responses "));
    let openai_request = openai.finish();
    assert!(
        openai_request.starts_with("POST /v1/chat/completions "),
        "{openai_request}"
    );
    assert!(
        openai_request.contains("\"stream\":false"),
        "{openai_request}"
    );
    let request_body = request_json(&openai_request);
    assert_eq!(
        (
            &request_body["messages"][0]["role"],
            &request_body["messages"][1]["role"]
        ),
        (
            &Value::String("system".into()),
            &Value::String("user".into())
        )
    );
    assert!(request_body["messages"][0]["content"].as_str().is_some());
    assert!(
        request_body["messages"][1]["content"]
            .as_str()
            .is_some_and(|content| content.contains("Fallback"))
    );
}

#[test]
fn search_primary_backend_can_use_more_than_its_even_budget_share() {
    let xai = Fixture::start_sequence(vec![
        Response::sse(
            200,
            &completed_body("Slow primary answer", "Primary source"),
        )
        .with_delay(std::time::Duration::from_secs(6)),
    ]);
    let openai = Fixture::start_canary();
    let environment = RunEnvironment::new(&main_fallback_config(
        &xai.url,
        &openai.url,
        false,
        &[],
        "auto",
        false,
    ));

    let output = environment.run(&[
        "search",
        "Slow primary",
        "--capabilities",
        "none",
        "--timeout",
        "10",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (output.status.code(), &payload["answer"]),
        (Some(0), &Value::String("Slow primary answer".into()),),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    xai.finish();
    assert!(openai.finish_all().is_empty());
}

#[test]
fn search_openai_stream_falls_back_to_non_stream_with_the_same_adapter() {
    let openai = Fixture::start_sequence(vec![
        Response::new(200, "text/event-stream", "data: [DONE]\n\n"),
        Response::new(
            200,
            "application/json",
            r#"{"choices":[{"message":{"content":"HTTP fallback","citations":["https://example.test/http"]}}]}"#,
        ),
    ]);
    let config = main_fallback_config("http://127.0.0.1:9", &openai.url, true, &[], "auto", false)
        .replace(
            r#"backends = ["xai", "openai_compatible"]"#,
            r#"backends = ["openai_compatible"]"#,
        );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "search",
        "Transport fallback",
        "--capabilities",
        "none",
        "--model",
        "override-model",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["answer"],
            &payload["sources"][0]["url"],
        ),
        (
            Some(0),
            &Value::String("HTTP fallback".into()),
            &Value::String("https://example.test/http".into()),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let requests = openai.finish_all();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].contains("\"stream\":true"), "{}", requests[0]);
    assert!(requests[1].contains("\"stream\":false"), "{}", requests[1]);
}

#[test]
fn search_primary_sse_transport_can_use_the_models_full_remaining_budget() {
    let openai = Fixture::start_sequence(vec![
        Response::sse(
            200,
            concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"Slow SSE answer\"},",
                "\"finish_reason\":\"stop\"}]}\n\n"
            ),
        )
        .with_delay(std::time::Duration::from_secs(4)),
    ]);
    let config = main_fallback_config("http://127.0.0.1:9", &openai.url, true, &[], "auto", false)
        .replace(
            r#"backends = ["xai", "openai_compatible"]"#,
            r#"backends = ["openai_compatible"]"#,
        );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "search",
        "Slow SSE",
        "--capabilities",
        "none",
        "--timeout",
        "6",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (output.status.code(), &payload["answer"]),
        (Some(0), &Value::String("Slow SSE answer".into())),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(openai.finish_all().len(), 1);
}

#[test]
fn search_openai_stream_accepts_nonempty_clean_eof() {
    let openai = Fixture::start(
        200,
        "text/event-stream",
        "data: {\"choices\":[{\"delta\":{\"content\":\"Partial answer\"}}]}\n\n",
    );
    let config = main_fallback_config("http://127.0.0.1:9", &openai.url, true, &[], "auto", true)
        .replace(
            r#"backends = ["xai", "openai_compatible"]"#,
            r#"backends = ["openai_compatible"]"#,
        );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "search",
        "Truncated stream",
        "--capabilities",
        "none",
        "--verbose",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let journal = read_only_journal(&environment);

    assert_eq!(
        (
            output.status.code(),
            &payload["answer"],
            &payload["journal_status"],
            payload["provider_attempts"].as_array().map(Vec::len),
            journal["execution"]["provider_attempts"]
                .as_array()
                .map(Vec::len),
        ),
        (
            Some(0),
            &Value::String("Partial answer".into()),
            &Value::String("written".into()),
            Some(1),
            Some(1),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let requests = openai.finish_all();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].contains("\"stream\":true"));
}

#[test]
fn search_openai_stream_accepts_finish_reason_without_done_marker() {
    let openai = Fixture::start(
        200,
        "text/event-stream",
        concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Complete answer\"},",
            "\"finish_reason\":\"stop\"}]}\n\n"
        ),
    );
    let config = main_fallback_config("http://127.0.0.1:9", &openai.url, true, &[], "auto", false)
        .replace(
            r#"backends = ["xai", "openai_compatible"]"#,
            r#"backends = ["openai_compatible"]"#,
        );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["search", "Finish reason", "--capabilities", "none"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (output.status.code(), &payload["answer"]),
        (Some(0), &Value::String("Complete answer".into())),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    openai.finish();
}

#[test]
fn search_openai_buffered_sse_accepts_nonempty_clean_eof() {
    let openai = Fixture::start(
        200,
        "text/event-stream",
        "data: {\"choices\":[{\"delta\":{\"content\":\"Partial answer\"}}]}\n\n",
    );
    let config = main_fallback_config("http://127.0.0.1:9", &openai.url, false, &[], "auto", false)
        .replace(
            r#"backends = ["xai", "openai_compatible"]"#,
            r#"backends = ["openai_compatible"]"#,
        );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["search", "Buffered stream", "--capabilities", "none"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (output.status.code(), &payload["answer"]),
        (Some(0), &Value::String("Partial answer".into())),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    openai.finish();
}

#[test]
fn search_openai_buffered_sse_rejects_empty_eof_and_malformed_frames() {
    for body in [
        "",
        concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
            "data: {not-json}\n\n"
        ),
    ] {
        let openai = Fixture::start(200, "text/event-stream", body);
        let config =
            main_fallback_config("http://127.0.0.1:9", &openai.url, false, &[], "auto", false)
                .replace(
                    r#"backends = ["xai", "openai_compatible"]"#,
                    r#"backends = ["openai_compatible"]"#,
                );
        let environment = RunEnvironment::new(&config);

        let output = environment.run(&["search", "Invalid stream", "--capabilities", "none"]);
        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

        assert_eq!(
            (output.status.code(), &payload["error_kind"]),
            (Some(4), &Value::String("runtime".into())),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        openai.finish();
    }
}

#[test]
fn search_openai_stream_environment_override_uses_the_same_adapter() {
    let openai = Fixture::start(
        200,
        "text/event-stream",
        concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Env stream\",\"citations\":[",
            "{\"url\":\"https://example.test/env\",\"title\":\"Env source\"}]}}]}\n\n",
            "data: [DONE]\n\n"
        ),
    );
    let config = main_fallback_config("http://127.0.0.1:9", &openai.url, false, &[], "auto", false)
        .replace(
            r#"backends = ["xai", "openai_compatible"]"#,
            r#"backends = ["openai_compatible"]"#,
        );
    let environment = RunEnvironment::new(&config);

    let output = environment.run_with_env(
        &["search", "Env stream", "--capabilities", "none"],
        &[("FORAGER_PROVIDERS__OPENAI_COMPATIBLE__STREAM", "true")],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["answer"],
            &payload["sources"][0]["title"],
        ),
        (
            Some(0),
            &Value::String("Env stream".into()),
            &Value::String("Env source".into()),
        )
    );
    let request = openai.finish();
    assert!(request.contains("\"stream\":true"), "{request}");
}

#[test]
fn search_openai_falls_back_between_configured_models() {
    let openai = Fixture::start_sequence(vec![
        Response::new(503, "application/json", r#"{"error":"primary failed"}"#),
        Response::new(
            200,
            "application/json",
            r#"{"choices":[{"message":{"content":"Model fallback"}}]}"#,
        ),
    ]);
    let config = main_fallback_config(
        "http://127.0.0.1:9",
        &openai.url,
        false,
        &["fallback-model"],
        "auto",
        true,
    )
    .replace(
        r#"backends = ["xai", "openai_compatible"]"#,
        r#"backends = ["openai_compatible"]"#,
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["search", "Models", "--capabilities", "none"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["answer"],
            &payload["journal_status"],
        ),
        (
            Some(0),
            &Value::String("Model fallback".into()),
            &Value::String("written".into()),
        )
    );
    let requests = openai.finish_all();
    assert!(requests[0].contains("\"model\":\"primary-model\""));
    assert!(requests[1].contains("\"model\":\"fallback-model\""));
    let journal = read_only_journal(&environment);
    assert_eq!(
        journal["execution"]["provider_attempts"]
            .as_array()
            .expect("journal attempts")
            .iter()
            .map(|attempt| attempt["model"].as_str().expect("attempt model"))
            .collect::<Vec<_>>(),
        ["primary-model", "fallback-model"]
    );
}

#[test]
fn search_primary_model_can_use_more_than_its_even_budget_share() {
    let openai = Fixture::start_sequence(vec![
        Response::json(
            200,
            r#"{"choices":[{"message":{"content":"Slow model answer"}}]}"#,
        )
        .with_delay(std::time::Duration::from_secs(6)),
    ]);
    let config = main_fallback_config(
        "http://127.0.0.1:9",
        &openai.url,
        false,
        &["fallback-model"],
        "auto",
        false,
    )
    .replace(
        r#"backends = ["xai", "openai_compatible"]"#,
        r#"backends = ["openai_compatible"]"#,
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "search",
        "Slow model",
        "--capabilities",
        "none",
        "--timeout",
        "10",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (output.status.code(), &payload["answer"]),
        (Some(0), &Value::String("Slow model answer".into())),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(openai.finish_all().len(), 1);
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
fn search_model_override_disables_configured_model_fallbacks() {
    let openai = Fixture::start(503, "application/json", r#"{"error":"override failed"}"#);
    let config = main_fallback_config(
        "http://127.0.0.1:9",
        &openai.url,
        false,
        &["fallback-model"],
        "auto",
        false,
    )
    .replace(
        r#"backends = ["xai", "openai_compatible"]"#,
        r#"backends = ["openai_compatible"]"#,
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "search",
        "Override",
        "--capabilities",
        "none",
        "--model",
        "override-model",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["attempts"]["total"],
            &payload["attempts"]["providers"],
        ),
        (
            Some(4),
            &Value::from(1),
            &serde_json::json!(["openai_compatible"]),
        )
    );
    let request = openai.finish();
    assert!(
        request.contains("\"model\":\"override-model\""),
        "{request}"
    );
}

#[test]
fn search_fallback_off_stops_after_the_configured_chain_head() {
    let xai = Fixture::start(503, "application/json", r#"{"error":"head failed"}"#);
    let environment = RunEnvironment::new(&main_fallback_config(
        &xai.url,
        "http://127.0.0.1:9",
        false,
        &["fallback-model"],
        "off",
        false,
    ));

    let output = environment.run(&["search", "No fallback", "--capabilities", "none"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["attempts"]["providers"],
            &payload["attempts"]["total"],
        ),
        (Some(4), &serde_json::json!(["xai"]), &Value::from(1),)
    );
    xai.finish();
}

#[test]
fn search_fallback_off_does_not_skip_an_unconfigured_chain_head() {
    let config = main_fallback_config(
        "http://127.0.0.1:9",
        "http://127.0.0.1:9",
        false,
        &[],
        "off",
        false,
    )
    .replace(r#"keys = ["xai-key"]"#, "keys = []");
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["search", "Head only", "--capabilities", "none"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            &payload["attempts"]["providers"],
        ),
        (
            Some(4),
            &Value::String("auth".into()),
            &serde_json::json!(["xai"]),
        )
    );
}

#[test]
fn search_fallback_off_disables_openai_model_fallback() {
    let openai = Fixture::start(503, "application/json", r#"{"error":"primary failed"}"#);
    let config = main_fallback_config(
        "http://127.0.0.1:9",
        &openai.url,
        false,
        &["fallback-model"],
        "off",
        false,
    )
    .replace(
        r#"backends = ["xai", "openai_compatible"]"#,
        r#"backends = ["openai_compatible"]"#,
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["search", "One model", "--capabilities", "none"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["attempts"]["total"],
            &payload["attempts"]["providers"],
        ),
        (
            Some(4),
            &Value::from(1),
            &serde_json::json!(["openai_compatible"]),
        )
    );
    let request = openai.finish();
    assert!(request.contains("\"model\":\"primary-model\""), "{request}");
}

#[test]
fn search_budget_exhaustion_preserves_the_full_attempt_chain_in_the_journal() {
    let slow = Response::new(
        200,
        "application/json",
        r#"{"choices":[{"message":{"content":"too late"}}]}"#,
    )
    .with_delay(std::time::Duration::from_secs(2));
    let openai = Fixture::start_sequence(vec![slow]);
    let environment = RunEnvironment::new(&main_fallback_config(
        "http://127.0.0.1:9",
        &openai.url,
        false,
        &[],
        "auto",
        true,
    ));

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
            Some(2),
            &Value::Bool(true),
        )
    );
    openai.finish();
}

#[test]
fn search_attribution_uses_each_provider_final_attempt_and_error_priority() {
    let xai = Fixture::start(401, "application/json", r#"{"error":"bad xAI key"}"#);
    let openai = Fixture::start(503, "application/json", r#"{"error":"relay down"}"#);
    let environment = RunEnvironment::new(&main_fallback_config(
        &xai.url,
        &openai.url,
        false,
        &[],
        "auto",
        false,
    ));

    let output = environment.run(&["search", "Attribution", "--capabilities", "none"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            &payload["message"],
            &payload["attempts"]["providers"],
        ),
        (
            Some(4),
            &Value::String("auth".into()),
            &Value::String(r#"{"error":"bad xAI key"}"#.into()),
            &serde_json::json!(["openai_compatible", "xai"]),
        )
    );
    xai.finish();
    openai.finish();
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
    let foreign_jsonl = journal_dir.join("search_results_20260809.jsonl");
    let similar_name = journal_dir.join("search_result_1_2_bad.json");
    for path in [
        &stale_owned,
        &fresh_owned,
        &foreign_json,
        &foreign_jsonl,
        &similar_name,
    ] {
        fs::write(path, "{}").expect("create retention fixture");
    }
    for path in [&stale_owned, &foreign_json, &foreign_jsonl, &similar_name] {
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
    for path in [fresh_owned, foreign_json, foreign_jsonl, similar_name] {
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
    let classifier = Fixture::start(
        200,
        "application/json",
        &format!(
            r#"{{"choices":[{{"message":{{"content":"{{\"required_capabilities\":[\"{secret}\"]}}"}}}}]}}"#
        ),
    );
    let main = Fixture::start(
        200,
        "text/event-stream",
        &completed_body(
            &format!("answer with {secret}"),
            &format!("title with {secret}"),
        ),
    );
    let config = format!(
        "{}\n[classifier]\nurl = {:?}\nkeys = [{secret:?}]\nmodel = \"classifier-model\"\ntimeout = 30\n",
        search_config(&main.url, true).replace("xai-key", secret),
        classifier.url,
    );
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
            Some(2),
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
    classifier.finish();
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

fn main_fallback_config(
    xai_url: &str,
    openai_url: &str,
    stream: bool,
    fallback_models: &[&str],
    fallback: &str,
    journal_enabled: bool,
) -> String {
    let xai_url = format!("{xai_url}/v1");
    let openai_url = format!("{openai_url}/v1");
    format!(
        r#"
[search]
backends = ["xai", "openai_compatible"]
fallback = {fallback:?}

[providers.xai]
url = {xai_url:?}
keys = ["xai-key"]
model = "xai-model"
tools = ["web_search"]

[providers.openai_compatible]
url = {openai_url:?}
keys = ["openai-key"]
model = "primary-model"
fallback_models = {fallback_models:?}
stream = {stream}

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
