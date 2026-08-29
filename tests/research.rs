mod support;

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use support::{Fixture, Response, RunEnvironment};

#[test]
fn bare_research_requires_a_configured_classifier() {
    let environment = RunEnvironment::new("");

    let output = environment.run(&["research", "What changed?"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            payload["message"]
                .as_str()
                .is_some_and(|message| message.contains(
                    "research without --plan requires a configured classifier plan generator"
                )),
            &payload["journal_ref"],
            &payload["journal_status"],
            output.stderr.is_empty(),
        ),
        (
            Some(3),
            &Value::String("config".into()),
            true,
            &Value::Null,
            &Value::String("unavailable".into()),
            true,
        )
    );
}

#[test]
fn bare_research_executes_the_classifier_plan_through_the_research_pipeline() {
    let classifier = Fixture::start(
        200,
        "application/json",
        &classifier_response(valid_plan(json!(["web_search"]))),
    );
    let tavily = Fixture::start(
        200,
        "application/json",
        r#"{"results":[{"title":"Generated plan evidence","url":"https://example.test/generated"}]}"#,
    );
    let jina = Fixture::start(
        200,
        "application/json",
        &jina_response(&rich_content("Generated plan body")),
    );
    let environment = RunEnvironment::new(&bare_research_config(
        &classifier.url,
        &tavily.url,
        &jina.url,
        true,
    ));

    let output = environment.run(&["research", "What changed?", "--verbose"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let journal = read_only_journal(&environment);
    let summary: Value = serde_json::from_slice(
        &fs::read(
            Path::new(payload["evidence_dir"].as_str().expect("evidence dir")).join("summary.json"),
        )
        .expect("read summary"),
    )
    .expect("parse summary");
    let persisted_plan: Value = serde_json::from_slice(
        &fs::read(payload["plan_path"].as_str().expect("plan path")).expect("read plan"),
    )
    .expect("parse plan");

    assert_eq!(
        (
            output.status.code(),
            &persisted_plan,
            payload["provider_attempts"]
                .as_array()
                .expect("verbose attempts")
                .iter()
                .map(|attempt| attempt["seam"].as_str().expect("seam"))
                .collect::<Vec<_>>(),
            &journal["execution"]["plan_summary"]["source"],
            &journal["execution"]["plan_summary"]["classifier_degraded"],
            summary["provider_attempts"]
                .as_array()
                .expect("summary attempts")
                .iter()
                .map(|attempt| attempt["seam"].as_str().expect("summary seam"))
                .collect::<Vec<_>>(),
        ),
        (
            Some(0),
            &valid_plan(json!(["web_search"])),
            vec!["classifier", "web_search", "web_fetch"],
            &Value::String("classifier".into()),
            &Value::Bool(false),
            vec!["classifier", "web_search", "web_fetch"],
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let classifier_request = classifier.finish();
    assert!(classifier_request.contains("\"name\":\"classifier_research_plan\""));
    assert!(
        classifier_request.contains("Plan the user request as a complete Schema v1 investigation.")
    );
    let request_body = classifier_request
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .expect("classifier request body");
    let request: Value = serde_json::from_str(request_body).expect("parse classifier request body");
    let system_prompt = request["messages"][0]["content"]
        .as_str()
        .expect("classifier system prompt");
    let embedded_vocabulary = system_prompt
        .split_once("Capability vocabulary:\n")
        .map(|(_, vocabulary)| vocabulary)
        .expect("embedded capability vocabulary");
    let embedded_vocabulary: Value =
        serde_json::from_str(embedded_vocabulary).expect("parse embedded capability vocabulary");
    let shared_vocabulary: Value = serde_json::from_str(include_str!(
        "../skills/forager/references/capability-vocabulary.json"
    ))
    .expect("parse shared capability vocabulary");
    assert_eq!(embedded_vocabulary, shared_vocabulary);
    assert!(!classifier_request.contains("selection_boundary"));
    assert!(
        classifier_request
            .contains("\"enum\":[\"docs_search\",\"web_search\",\"vertical_search\"]")
    );
    assert!(classifier_request.contains("\"maxItems\":4"));
    assert!(system_prompt.contains("at most 4 subquestions"));
    assert!(system_prompt.contains("most important first"));
    tavily.finish();
    jina.finish();
}

#[test]
fn bare_research_truncates_an_over_limit_classifier_plan_by_importance() {
    let mut plan = valid_plan(json!(["web_search"]));
    for index in 2..=3 {
        plan["decomposition"]
            .as_array_mut()
            .expect("decomposition")
            .push(json!({
                "id": format!("sq{index}"),
                "question": format!("Question {index}?"),
                "reason": format!("Priority {index}"),
                "required_capabilities": ["web_search"]
            }));
    }
    let classifier = Fixture::start(200, "application/json", &classifier_response(plan));
    let tavily = Fixture::start_sequence(vec![
        Response::new(
            200,
            "application/json",
            r#"{"results":[{"title":"First","url":"https://example.test/first"}]}"#,
        ),
        Response::new(
            200,
            "application/json",
            r#"{"results":[{"title":"Second","url":"https://example.test/second"}]}"#,
        ),
    ]);
    let jina = Fixture::start_sequence(vec![
        Response::new(
            200,
            "application/json",
            &jina_response(&rich_content("First body")),
        ),
        Response::new(
            200,
            "application/json",
            &jina_response(&rich_content("Second body")),
        ),
    ]);
    let environment = RunEnvironment::new(&bare_research_config(
        &classifier.url,
        &tavily.url,
        &jina.url,
        false,
    ));

    let output = environment.run(&["research", "Prioritized plan", "--budget", "quick"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let persisted_plan: Value = serde_json::from_slice(
        &fs::read(payload["plan_path"].as_str().expect("plan path")).expect("read plan"),
    )
    .expect("parse plan");

    assert_eq!(
        (
            output.status.code(),
            persisted_plan["decomposition"].as_array().map(Vec::len),
            persisted_plan["decomposition"]
                .as_array()
                .expect("decomposition")
                .iter()
                .map(|item| item["id"].as_str().expect("id"))
                .collect::<Vec<_>>(),
            String::from_utf8_lossy(&output.stderr)
                .contains("classifier returned 3 subquestions; truncated to quick limit 2"),
        ),
        (Some(0), Some(2), vec!["sq1", "sq2"], true),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let classifier_request = classifier.finish();
    assert!(classifier_request.contains("\"maxItems\":2"));
    assert!(classifier_request.contains("at most 2 subquestions"));
    assert_eq!(tavily.finish_all().len(), 2);
    assert_eq!(jina.finish_all().len(), 2);
}

#[test]
fn bare_research_uses_the_fixed_web_search_plan_when_classifier_transport_fails() {
    let classifier = Fixture::start(500, "text/plain", "classifier unavailable");
    let tavily = Fixture::start(
        200,
        "application/json",
        r#"{"results":[{"title":"Fallback evidence","url":"https://example.test/fallback"}]}"#,
    );
    let jina = Fixture::start(
        200,
        "application/json",
        &jina_response(&rich_content("Fallback body")),
    );
    let environment = RunEnvironment::new(&bare_research_config(
        &classifier.url,
        &tavily.url,
        &jina.url,
        true,
    ));

    let output = environment.run(&["research", "What changed?", "--verbose"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let journal = read_only_journal(&environment);
    let persisted_plan: Value = serde_json::from_slice(
        &fs::read(payload["plan_path"].as_str().expect("plan path")).expect("read plan"),
    )
    .expect("parse plan");
    let summary: Value = serde_json::from_slice(
        &fs::read(
            Path::new(payload["evidence_dir"].as_str().expect("evidence dir")).join("summary.json"),
        )
        .expect("read summary"),
    )
    .expect("parse summary");

    assert_eq!(
        (
            output.status.code(),
            &journal["execution"]["plan_summary"]["source"],
            &persisted_plan["decomposition"][0]["required_capabilities"],
            String::from_utf8_lossy(&output.stderr).contains("Classifier warning"),
            &journal["execution"]["plan_summary"]["classifier_degraded"],
            &summary["provider_attempts"][0]["provider"],
            &summary["provider_attempts"][0]["disposition"],
        ),
        (
            Some(0),
            &Value::String("classifier_degraded".into()),
            &json!(["web_search"]),
            true,
            &Value::Bool(true),
            &Value::String("classifier".into()),
            &Value::String("failed".into()),
        )
    );
    classifier.finish();
    tavily.finish();
    jina.finish();
}

#[test]
fn bare_research_degrades_when_classifier_returns_an_invalid_plan() {
    let classifier = Fixture::start(
        200,
        "application/json",
        &classifier_response(json!({
            "plan_version": 1,
            "intent_signals": {
                "recency_requirement": "none",
                "docs_api_intent": false,
                "source_authority_need": "normal",
                "claim_risk": "medium",
                "cross_validation_need": "normal"
            },
            "decomposition": []
        })),
    );
    let tavily = Fixture::start(
        200,
        "application/json",
        r#"{"results":[{"title":"Fallback evidence","url":"https://example.test/invalid"}]}"#,
    );
    let jina = Fixture::start(
        200,
        "application/json",
        &jina_response(&rich_content("Fallback body")),
    );
    let environment = RunEnvironment::new(&bare_research_config(
        &classifier.url,
        &tavily.url,
        &jina.url,
        false,
    ));

    let output = environment.run(&["research", "What changed?"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let summary: Value = serde_json::from_slice(
        &fs::read(
            Path::new(payload["evidence_dir"].as_str().expect("evidence dir")).join("summary.json"),
        )
        .expect("read summary"),
    )
    .expect("parse summary");

    assert_eq!(
        (output.status.code(), &summary["plan_source"]),
        (Some(0), &Value::String("classifier_degraded".into())),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    classifier.finish();
    tavily.finish();
    jina.finish();
}

#[test]
fn bare_research_preserves_classifier_metadata_on_an_evidence_terminal() {
    let mut plan = valid_plan(json!([]));
    plan["intent_signals"]["cross_validation_need"] = json!("high");
    let classifier = Fixture::start(200, "application/json", &classifier_response(plan));
    let jina = Fixture::start(
        200,
        "application/json",
        &jina_response(&rich_content("Only evidence")),
    );
    let environment = RunEnvironment::new(&bare_fetch_research_config(
        &classifier.url,
        &jina.url,
        true,
    ));

    let output = environment.run(&["research", "Verify https://example.test/only"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let journal = read_only_journal(&environment);
    let manifest = read_recovery_manifest(&payload);

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            &journal["execution"]["plan_summary"]["source"],
            &journal["execution"]["plan_summary"]["classifier_degraded"],
            &manifest["provider_attempts"][0]["provider"],
        ),
        (
            Some(5),
            &Value::String("evidence".into()),
            &Value::String("classifier".into()),
            &Value::Bool(false),
            &Value::String("classifier".into()),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    classifier.finish();
    jina.finish();
}

#[test]
fn research_rejects_strict_schema_v1_negative_cases_before_loading_config() {
    let environment = RunEnvironment::new("");
    let valid = valid_plan(json!(["web_search"]));
    let mut cases = Vec::new();

    let mut unknown_version = valid.clone();
    unknown_version["plan_version"] = json!(2);
    cases.push(("unknown-version", unknown_version));

    let mut unknown_capability = valid.clone();
    unknown_capability["decomposition"][0]["required_capabilities"] = json!(["unknown"]);
    cases.push(("unknown-capability", unknown_capability));

    let mut web_fetch = valid.clone();
    web_fetch["decomposition"][0]["required_capabilities"] = json!(["web_fetch"]);
    cases.push(("web-fetch", web_fetch));

    let mut duplicate_id = valid.clone();
    duplicate_id["decomposition"]
        .as_array_mut()
        .expect("decomposition")
        .push(valid["decomposition"][0].clone());
    cases.push(("duplicate-id", duplicate_id));

    let mut empty_id = valid.clone();
    empty_id["decomposition"][0]["id"] = json!(" ");
    cases.push(("empty-id", empty_id));

    let mut empty_reason = valid.clone();
    empty_reason["decomposition"][0]["reason"] = json!("");
    cases.push(("empty-reason", empty_reason));

    let mut unknown_field = valid.clone();
    unknown_field["steps"] = json!([]);
    cases.push(("unknown-field", unknown_field));

    let mut missing_field = valid.clone();
    missing_field
        .as_object_mut()
        .expect("plan object")
        .remove("intent_signals");
    cases.push(("missing-field", missing_field));

    let mut empty_decomposition = valid;
    empty_decomposition["decomposition"] = json!([]);
    cases.push(("empty-decomposition", empty_decomposition));

    for (name, plan) in cases {
        let path = environment.config_dir.join(format!("{name}.json"));
        fs::write(&path, serde_json::to_vec(&plan).expect("encode plan")).expect("write plan");
        let output = environment.run(&[
            "research",
            "Strict plan",
            "--plan",
            path.to_str().expect("UTF-8 path"),
        ]);

        assert_eq!(
            output.status.code(),
            Some(2),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn research_rejects_caller_plans_over_each_budget_subquestion_limit() {
    for (budget, limit) in [("quick", 2), ("standard", 4), ("deep", 6)] {
        let environment = RunEnvironment::new("");
        let mut plan = valid_plan(json!([]));
        for index in 2..=(limit + 1) {
            plan["decomposition"]
                .as_array_mut()
                .expect("decomposition")
                .push(json!({
                    "id": format!("sq{index}"),
                    "question": format!("Question {index}?"),
                    "reason": format!("Need {index}"),
                    "required_capabilities": []
                }));
        }
        let plan_path = write_plan(&environment, plan);

        let output = environment.run(&[
            "research",
            "Too many subquestions",
            "--plan",
            plan_path.to_str().expect("UTF-8 path"),
            "--budget",
            budget,
        ]);
        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

        assert_eq!(
            (
                output.status.code(),
                &payload["error_kind"],
                payload["message"].as_str().is_some_and(|message| message.contains(&format!(
                    "caller research plan has {} subquestions; {budget} budget allows at most {limit}",
                    limit + 1
                ))),
                &payload["journal_ref"],
                &payload["journal_status"],
                output.stderr.is_empty(),
            ),
            (
                Some(2),
                &Value::String("parameter".into()),
                true,
                &Value::Null,
                &Value::String("unavailable".into()),
                true,
            ),
            "{budget}"
        );
    }
}

#[test]
fn research_reads_plan_from_stdin_and_normalizes_capabilities_in_declared_order() {
    let jina = Fixture::start(
        200,
        "application/json",
        &jina_response(&rich_content("Known evidence")),
    );
    let environment = RunEnvironment::new(&fetch_only_config(&jina.url, true));
    let plan = valid_plan(json!(["vertical_search", "docs_search", "vertical_search"]));

    let output = environment.run_with_stdin(
        &[
            "research",
            "Read https://example.test/known",
            "--plan",
            "-",
            "--budget",
            "quick",
        ],
        &serde_json::to_string(&plan).expect("encode plan"),
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let persisted_plan: Value = serde_json::from_slice(
        &fs::read(payload["plan_path"].as_str().expect("plan path")).expect("read plan"),
    )
    .expect("parse plan");
    let journal = read_only_journal(&environment);

    assert_eq!(
        (
            output.status.code(),
            &persisted_plan["decomposition"][0]["required_capabilities"],
            &journal["execution"]["plan_summary"]["capabilities"],
            &payload["evidence_items"][0]["url"],
        ),
        (
            Some(0),
            &json!(["vertical_search", "docs_search"]),
            &json!(["docs_search", "web_fetch", "vertical_search"]),
            &Value::String("https://example.test/known".into()),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    jina.finish();
}

#[test]
fn research_discovers_then_fetches_before_emitting_claims_and_journals_evidence() {
    let tavily = Fixture::start_sequence(vec![
        Response::new(
            200,
            "application/json",
            r#"{"results":[{"title":"Discovery only","url":"https://example.test/evidence"}]}"#,
        ),
        Response::new(
            200,
            "application/json",
            r#"{"results":[{"raw_content":"thin"}]}"#,
        ),
    ]);
    let firecrawl = Fixture::start(
        200,
        "application/json",
        r#"{"success":true,"data":{"markdown":"also thin"}}"#,
    );
    let jina = Fixture::start(
        200,
        "application/json",
        &jina_response(&rich_content("Fetched body")),
    );
    let evidence_dir = tempfile::tempdir().expect("evidence dir");
    let config = shared_fetch_research_config(&tavily.url, &firecrawl.url, &jina.url);
    let environment = RunEnvironment::new(&config);
    let plan_path = write_plan(&environment, valid_plan(json!(["web_search"])));

    let output = environment.run(&[
        "research",
        "What changed?",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
        "--budget",
        "standard",
        "--evidence-dir",
        evidence_dir.path().to_str().expect("UTF-8 evidence path"),
        "--verbose",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let journal = read_only_journal(&environment);

    assert_eq!(
        (
            output.status.code(),
            &payload["evidence_items"][0]["url"],
            fs::read_to_string(
                payload["evidence_items"][0]["path"]
                    .as_str()
                    .expect("evidence path"),
            )
            .expect("read evidence")
            .contains("Fetched body"),
            &journal["execution"]["plan_summary"]["source"],
            &journal["execution"]["plan_summary"]["capabilities"],
            &journal["result"]["evidence_items"][0]["url"],
        ),
        (
            Some(0),
            &Value::String("https://example.test/evidence".into()),
            true,
            &Value::String("caller".into()),
            &json!(["web_search", "web_fetch"]),
            &Value::String("https://example.test/evidence".into()),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(evidence_dir.path().join("00-plan.json").is_file());
    assert!(evidence_dir.path().join("summary.json").is_file());
    assert_eq!(
        payload["provider_attempts"]
            .as_array()
            .expect("verbose attempts")
            .iter()
            .map(|attempt| attempt["seam"].as_str().expect("seam"))
            .collect::<Vec<_>>(),
        ["web_search", "web_fetch", "web_fetch", "web_fetch"]
    );
    assert_eq!(tavily.finish_all().len(), 2);
    firecrawl.finish();
    jina.finish();
}

#[test]
fn research_returns_a_file_backed_evidence_index_without_repeating_evidence_content() {
    let tavily = Fixture::start(
        200,
        "application/json",
        r#"{"results":[
            {"title":"Selected","url":"https://example.test/selected"},
            {"title":"Candidate A","url":"https://example.test/candidate-a"},
            {"title":"Candidate B","url":"https://example.test/candidate-b"}
        ]}"#,
    );
    let evidence_body = rich_content("Unique persisted evidence body");
    let jina = Fixture::start(200, "application/json", &jina_response(&evidence_body));
    let evidence_dir = tempfile::tempdir().expect("evidence dir");
    let environment = RunEnvironment::new(&research_config(&tavily.url, &jina.url, true));
    let plan_path = write_plan(&environment, valid_plan(json!(["web_search"])));

    let output = environment.run(&[
        "research",
        "Build an evidence index",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
        "--budget",
        "quick",
        "--evidence-dir",
        evidence_dir.path().to_str().expect("UTF-8 evidence path"),
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let evidence = payload["evidence_items"][0]
        .as_object()
        .expect("evidence index item");
    let candidates_path = payload["unconsumed_candidates"]["path"]
        .as_str()
        .expect("candidate artifact path");
    let candidates: Value = serde_json::from_slice(
        &fs::read(candidates_path).expect("read candidate artifact from returned path"),
    )
    .expect("parse candidate artifact");
    let journal = read_only_journal(&environment);

    assert_eq!(
        (
            output.status.code(),
            payload
                .as_object()
                .expect("research index")
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            evidence.keys().map(String::as_str).collect::<Vec<_>>(),
            &payload["unconsumed_candidates"]["count"],
            &payload["synthesis_policy"],
            &candidates["is_evidence"],
            candidates["candidates"].as_array().map(Vec::len),
        ),
        (
            Some(0),
            vec![
                "capability_gaps",
                "evidence_dir",
                "evidence_items",
                "gap_check",
                "journal_ref",
                "journal_status",
                "plan_path",
                "synthesis_policy",
                "unconsumed_candidates",
            ],
            vec![
                "content_len",
                "id",
                "path",
                "provider",
                "source_type",
                "subquestion_ids",
                "title",
                "url",
                "verified",
            ],
            &json!(2),
            &Value::String("fetch_before_claim".into()),
            &Value::Bool(false),
            Some(2),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_file_backed_index_artifacts(
        &payload,
        evidence_dir.path(),
        &evidence_body,
        &output.stdout,
        &journal,
    );
    tavily.finish();
    jina.finish();
}

#[test]
fn research_markdown_and_content_render_the_same_index_semantics_and_output_remains_a_tee() {
    let body = rich_content("Persisted format evidence");
    let jina = Fixture::start_sequence(
        (0..2)
            .map(|_| Response::new(200, "application/json", &jina_response(&body)))
            .collect(),
    );
    let environment = RunEnvironment::new(&fetch_only_config(&jina.url, false));
    let plan_path = write_plan(&environment, valid_plan(json!([])));

    for format in ["markdown", "content"] {
        let evidence_dir = tempfile::tempdir().expect("evidence dir");
        let tee = evidence_dir.path().join("stdout.txt");
        let output = environment.run(&[
            "research",
            "Verify https://example.test/known",
            "--plan",
            plan_path.to_str().expect("UTF-8 plan path"),
            "--evidence-dir",
            evidence_dir.path().to_str().expect("UTF-8 evidence path"),
            "--format",
            format,
            "--output",
            tee.to_str().expect("UTF-8 tee path"),
        ]);
        let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");

        assert_eq!(
            (
                output.status.code(),
                stdout.contains("Research Evidence Index"),
                stdout.contains("Unresolved gaps"),
                stdout.contains("fetch_before_claim"),
                stdout.contains(&body),
                fs::read_to_string(&tee).expect("read tee output"),
            ),
            (Some(0), true, true, true, false, stdout.clone()),
            "format: {format}; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(jina.finish_all().len(), 2);
}

#[test]
fn research_intent_signals_do_not_add_or_reorder_capability_seams() {
    let jina = Fixture::start(
        200,
        "application/json",
        &jina_response(&rich_content("Known URL evidence")),
    );
    let environment = RunEnvironment::new(&fetch_only_config(&jina.url, false));
    let mut plan = valid_plan(json!([]));
    plan["intent_signals"] = json!({
        "recency_requirement": "current",
        "docs_api_intent": true,
        "source_authority_need": "normal",
        "claim_risk": "medium",
        "cross_validation_need": "normal"
    });
    let plan_path = write_plan(&environment, plan);

    let output = environment.run(&[
        "research",
        "Verify https://example.test/known",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
        "--verbose",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let summary: Value = serde_json::from_slice(
        &fs::read(
            Path::new(payload["evidence_dir"].as_str().expect("evidence dir")).join("summary.json"),
        )
        .expect("read summary"),
    )
    .expect("parse summary");

    assert_eq!(
        (
            output.status.code(),
            &summary["capabilities"],
            payload["provider_attempts"]
                .as_array()
                .expect("attempts")
                .iter()
                .map(|attempt| attempt["seam"].as_str().expect("seam"))
                .collect::<Vec<_>>(),
        ),
        (Some(0), &json!(["web_fetch"]), vec!["web_fetch"])
    );
    jina.finish();
}

#[test]
fn research_fetches_each_known_url_once_with_the_most_specific_ownership() {
    let jina = Fixture::start_sequence(vec![
        Response::new(
            200,
            "application/json",
            &jina_response(&rich_content("Subquestion evidence")),
        ),
        Response::new(
            200,
            "application/json",
            &jina_response(&rich_content("Plan evidence")),
        ),
    ]);
    let environment = RunEnvironment::new(&fetch_only_config(&jina.url, false));
    let mut plan = valid_plan(json!([]));
    plan["decomposition"][0]["question"] =
        json!("Inspect [the shared source](https://example.test/shared)。");
    plan["decomposition"]
        .as_array_mut()
        .expect("decomposition")
        .push(json!({
            "id": "sq2",
            "question": "Verify https://example.test/shared independently",
            "reason": "Cover the second question",
            "required_capabilities": []
        }));
    let plan_path = write_plan(&environment, plan);

    let output = environment.run(&[
        "research",
        "Compare <https://example.test/shared>,<https://example.test/plan-only>。",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
        "--verbose",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            payload["evidence_items"]
                .as_array()
                .expect("evidence items")
                .iter()
                .map(|item| (&item["url"], &item["subquestion_ids"]))
                .collect::<Vec<_>>(),
            payload["provider_attempts"].as_array().map(Vec::len),
        ),
        (
            Some(0),
            vec![
                (
                    &Value::String("https://example.test/shared".into()),
                    &json!(["sq1", "sq2"]),
                ),
                (
                    &Value::String("https://example.test/plan-only".into()),
                    &json!([]),
                ),
            ],
            Some(2),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(jina.finish_all().len(), 2);
}

#[test]
fn research_fetches_known_urls_outside_the_subquestion_evidence_cap() {
    let tavily = Fixture::start(
        200,
        "application/json",
        r#"{"results":[{"title":"Discovered","url":"https://example.test/discovered"}]}"#,
    );
    let jina = Fixture::start_sequence(
        ["Known A", "Known B", "Discovered"]
            .into_iter()
            .map(|label| {
                Response::new(
                    200,
                    "application/json",
                    &jina_response(&rich_content(label)),
                )
            })
            .collect(),
    );
    let environment = RunEnvironment::new(&research_config(&tavily.url, &jina.url, false));
    let mut plan = valid_plan(json!(["web_search"]));
    plan["decomposition"][0]["question"] =
        json!("Compare https://example.test/known-a and https://example.test/known-b");
    let plan_path = write_plan(&environment, plan);

    let output = environment.run(&[
        "research",
        "Compare the supplied sources",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
        "--budget",
        "quick",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            payload["evidence_items"].as_array().map(Vec::len),
            &payload["gap_check"]["status"],
        ),
        (Some(0), Some(3), &Value::String("closed".into())),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    tavily.finish();
    assert_eq!(jina.finish_all().len(), 3);
}

#[test]
fn research_fetches_known_urls_concurrently_and_preserves_plan_order() {
    let content = rich_content("Known evidence");
    let jina = Fixture::start_parallel_sequence(vec![
        Response::new(200, "application/json", &jina_response(&content))
            .with_delay(Duration::from_secs(2)),
        Response::new(200, "application/json", &jina_response(&content))
            .with_delay(Duration::from_secs(2)),
    ]);
    let environment = RunEnvironment::new(&fetch_only_config(&jina.url, false));
    let mut plan = valid_plan(json!([]));
    plan["decomposition"][0]["question"] =
        json!("Compare https://example.test/known-one and https://example.test/known-two");
    let plan_path = write_plan(&environment, plan);
    let started = Instant::now();

    let output = environment.run(&[
        "research",
        "Compare the supplied sources",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
        "--timeout",
        "3",
    ]);
    let elapsed = started.elapsed();
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            payload["evidence_items"]
                .as_array()
                .expect("evidence items")
                .iter()
                .map(|item| item["url"].as_str().expect("evidence URL"))
                .collect::<Vec<_>>(),
        ),
        (
            Some(0),
            vec![
                "https://example.test/known-one",
                "https://example.test/known-two"
            ],
        ),
        "elapsed: {elapsed:?}; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(jina.finish_all().len(), 2);
}

#[test]
fn research_keeps_a_failed_known_url_gap_after_replacement_evidence_succeeds() {
    let tavily = Fixture::start(
        200,
        "application/json",
        r#"{"results":[{"title":"Replacement","url":"https://example.test/replacement"}]}"#,
    );
    let jina = Fixture::start_sequence(vec![
        Response::new(500, "text/plain", "known URL unavailable"),
        Response::new(
            200,
            "application/json",
            &jina_response(&rich_content("Replacement evidence")),
        ),
    ]);
    let environment = RunEnvironment::new(&research_config(&tavily.url, &jina.url, false));
    let mut plan = valid_plan(json!(["web_search"]));
    plan["decomposition"][0]["question"] = json!("Verify https://example.test/known-unavailable");
    let plan_path = write_plan(&environment, plan);

    let output = environment.run(&[
        "research",
        "Verify the claim",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["gap_check"],
            payload["evidence_items"].as_array().map(Vec::len),
        ),
        (
            Some(0),
            &json!({
                "status": "degraded",
                "gaps": [{
                    "subquestion_id": "sq1",
                    "reason": "failed to fetch known URL",
                    "url": "https://example.test/known-unavailable"
                }],
                "stop_reason": "degraded_with_gaps"
            }),
            Some(1),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    tavily.finish();
    assert_eq!(jina.finish_all().len(), 2);
}

#[test]
fn research_reports_one_terminal_gap_with_the_candidate_url_attempt_count() {
    let tavily = Fixture::start_sequence(vec![
        Response::new(
            200,
            "application/json",
            r#"{"results":[{"title":"Covered","url":"https://example.test/covered"}]}"#,
        ),
        Response::new(
            200,
            "application/json",
            r#"{"results":[
                {"title":"Unavailable A","url":"https://example.test/unavailable-a"},
                {"title":"Unavailable B","url":"https://example.test/unavailable-b"}
            ]}"#,
        ),
    ]);
    let jina = Fixture::start_sequence(vec![
        Response::new(
            200,
            "application/json",
            &jina_response(&rich_content("Covered evidence")),
        ),
        Response::new(500, "text/plain", "candidate unavailable"),
        Response::new(500, "text/plain", "candidate unavailable"),
    ]);
    let environment = RunEnvironment::new(&research_config(&tavily.url, &jina.url, false));
    let mut plan = valid_plan(json!(["web_search"]));
    plan["decomposition"]
        .as_array_mut()
        .expect("decomposition")
        .push(json!({
            "id": "sq2",
            "question": "What unavailable evidence exists?",
            "reason": "Exercise candidate exhaustion",
            "required_capabilities": ["web_search"]
        }));
    let plan_path = write_plan(&environment, plan);

    let output = environment.run(&[
        "research",
        "Compare candidate outcomes",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
        "--budget",
        "standard",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["gap_check"]["status"],
            &payload["gap_check"]["gaps"],
        ),
        (
            Some(0),
            &Value::String("degraded".into()),
            &json!([{
                "subquestion_id": "sq2",
                "reason": "no verified evidence was collected after attempting 2 candidate URLs"
            }]),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(tavily.finish_all().len(), 2);
    assert_eq!(jina.finish_all().len(), 3);
}

#[test]
fn research_reports_provider_gaps_as_advisory_when_other_evidence_succeeds() {
    let jina = Fixture::start(
        200,
        "application/json",
        &jina_response(&rich_content("Known URL evidence")),
    );
    let environment = RunEnvironment::new(&fetch_only_config(&jina.url, false));
    let plan_path = write_plan(&environment, valid_plan(json!(["web_search"])));

    let output = environment.run(&[
        "research",
        "Verify https://example.test/known",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["capability_gaps"],
            &payload["gap_check"]["status"],
            String::from_utf8_lossy(&output.stderr).contains("capability gap"),
        ),
        (
            Some(0),
            &json!([{
                "capability": "web_search",
                "reason": "no_configured_provider",
                "providers_skipped": ["tavily", "firecrawl"]
            }]),
            &Value::String("degraded".into()),
            true,
        )
    );
    jina.finish();
}

#[test]
fn research_returns_evidence_exit_five_when_discovery_cannot_be_fetched() {
    let tavily = Fixture::start(
        200,
        "application/json",
        r#"{"results":[{"title":"Candidate","url":"https://example.test/unfetched"}]}"#,
    );
    let config = format!(
        r#"
[providers.tavily]
url = {:?}
keys = ["tavily-key"]
timeout = 30

[capabilities.web_search]
order = ["tavily"]

[retry]
max_attempts = 1
multiplier = 1
max_wait = 0

[journal]
enabled = false
"#,
        tavily.url
    );
    let environment = RunEnvironment::new(&config);
    let plan_path = write_plan(&environment, valid_plan(json!(["web_search"])));

    let output = environment.run(&[
        "research",
        "Unfetched claim",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (output.status.code(), &payload["error_kind"]),
        (Some(5), &Value::String("evidence".into())),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    tavily.finish();
}

#[test]
fn research_reports_error_summary_write_failures_without_replacing_the_terminal() {
    let environment = RunEnvironment::new("");
    let plan_path = write_plan(&environment, valid_plan(json!([])));
    let evidence_dir = tempfile::tempdir().expect("evidence dir");
    fs::create_dir(evidence_dir.path().join("summary.json"))
        .expect("block the summary artifact path");

    let output = environment.run(&[
        "research",
        "No available evidence",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
        "--evidence-dir",
        evidence_dir.path().to_str().expect("UTF-8 evidence path"),
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            &payload["evidence_dir"],
            &payload["summary_path"],
            String::from_utf8_lossy(&output.stderr)
                .contains("cannot write research summary artifact"),
        ),
        (
            Some(5),
            &Value::String("evidence".into()),
            &json!(evidence_dir.path()),
            &Value::Null,
            true,
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn research_failure_returns_a_stable_envelope_and_readable_recovery_manifest() {
    let environment = RunEnvironment::new("");
    let plan_path = write_plan(&environment, valid_plan(json!([])));
    let evidence_dir = tempfile::tempdir().expect("evidence dir");

    let output = environment.run(&[
        "research",
        "No available evidence",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
        "--evidence-dir",
        evidence_dir.path().to_str().expect("UTF-8 evidence path"),
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let manifest = read_recovery_manifest(&payload);

    assert_eq!(
        (
            output.status.code(),
            payload
                .as_object()
                .expect("failure envelope")
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        ),
        (
            Some(5),
            vec![
                "error_kind",
                "evidence_dir",
                "gap_check",
                "journal_ref",
                "journal_status",
                "message",
                "summary_path",
                "synthesis_policy",
            ],
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        (
            &manifest["status"],
            &manifest["error_kind"],
            &manifest["query"],
            &manifest["budget"],
            &manifest["plan_source"],
            &manifest["fallback"],
            &manifest["evidence_dir"],
            &manifest["plan_path"],
        ),
        (
            &json!("error"),
            &json!("evidence"),
            &json!("No available evidence"),
            &json!("standard"),
            &json!("caller"),
            &json!({"mode": "auto", "used": false}),
            &json!(evidence_dir.path()),
            &json!(evidence_dir.path().join("00-plan.json")),
        )
    );
    assert_eq!(
        (
            &manifest["unconsumed_candidates"],
            &manifest["coverage"]["gap_check"],
            &manifest["synthesis_policy"],
            manifest["provider_attempts"].as_array().map(Vec::len),
            manifest["evidence_items"].as_array().map(Vec::len),
        ),
        (
            &json!({
                "count": 0,
                "path": evidence_dir.path().join("candidates.json")
            }),
            &json!({
                "status": "degraded",
                "gaps": [{
                    "subquestion_id": "sq1",
                    "reason": "no verified evidence was collected after attempting 0 candidate URLs"
                }, {
                    "subquestion_id": "",
                    "reason": "research required 1 evidence items but obtained 0"
                }],
                "stop_reason": "degraded_with_gaps"
            }),
            &json!("fetch_before_claim"),
            Some(0),
            Some(0),
        )
    );
    assert!(
        manifest["message"]
            .as_str()
            .is_some_and(|message| message.len() <= 500)
    );
    assert!(manifest["capabilities"].is_array());
    assert!(manifest["capability_gaps"].is_array());
}

#[test]
fn research_early_artifact_failure_returns_no_speculative_summary_path() {
    let environment = RunEnvironment::new("");
    let plan_path = write_plan(&environment, valid_plan(json!([])));
    let blocked_evidence_dir = environment.config_dir.join("blocked-evidence-dir");
    fs::write(&blocked_evidence_dir, "not a directory").expect("block evidence directory");

    let output = environment.run(&[
        "research",
        "No artifact root",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
        "--evidence-dir",
        blocked_evidence_dir.to_str().expect("UTF-8 evidence path"),
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            &payload["evidence_dir"],
            &payload["summary_path"],
        ),
        (
            Some(4),
            &json!("runtime"),
            &json!(blocked_evidence_dir),
            &Value::Null,
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn research_large_failure_metadata_stays_in_the_recovery_manifest() {
    let title = "metadata".repeat(700);
    let tavily = Fixture::start(
        200,
        "application/json",
        &json!({
            "results": [{
                "title": title,
                "url": "https://example.test/large-metadata"
            }]
        })
        .to_string(),
    );
    let jina = Fixture::start(
        200,
        "application/json",
        &jina_response(&rich_content("Large metadata evidence")),
    );
    let environment = RunEnvironment::new(&research_config(&tavily.url, &jina.url, false));
    let mut plan = valid_plan(json!(["web_search"]));
    plan["intent_signals"]["cross_validation_need"] = json!("high");
    let plan_path = write_plan(&environment, plan);
    let evidence_dir = tempfile::tempdir().expect("evidence dir");

    let output = environment.run(&[
        "research",
        "Recover large metadata",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
        "--evidence-dir",
        evidence_dir.path().to_str().expect("UTF-8 evidence path"),
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let manifest = read_recovery_manifest(&payload);

    assert!(output.stdout.len() <= 4096);
    assert_eq!(
        (
            output.status.code(),
            &manifest["evidence_items"][0]["title"],
            Path::new(
                manifest["evidence_items"][0]["path"]
                    .as_str()
                    .expect("evidence path")
            )
            .is_file(),
            Path::new(manifest["plan_path"].as_str().expect("plan path")).is_file(),
            Path::new(
                manifest["unconsumed_candidates"]["path"]
                    .as_str()
                    .expect("candidate path")
            )
            .is_file(),
            &manifest["coverage"]["gap_check"]["status"],
        ),
        (Some(5), &json!(title), true, true, true, &json!("degraded"),),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    tavily.finish();
    jina.finish();
}

#[test]
fn research_summary_write_failure_preserves_the_already_persisted_evidence_index() {
    let body = rich_content("Evidence retained after summary failure");
    let jina = Fixture::start(200, "application/json", &jina_response(&body));
    let environment = RunEnvironment::new(&fetch_only_config(&jina.url, false));
    let plan_path = write_plan(&environment, valid_plan(json!([])));
    let evidence_dir = tempfile::tempdir().expect("evidence dir");
    fs::create_dir(evidence_dir.path().join("summary.json"))
        .expect("block the summary artifact path");

    let output = environment.run(&[
        "research",
        "Verify https://example.test/known",
        "--plan",
        plan_path.to_str().expect("UTF-8 plan path"),
        "--evidence-dir",
        evidence_dir.path().to_str().expect("UTF-8 evidence path"),
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let evidence_path = evidence_dir.path().join("01-evidence.md");

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            &payload["evidence_dir"],
            &payload["summary_path"],
            evidence_path.is_file(),
            evidence_dir.path().join("candidates.json").is_file(),
            evidence_dir.path().join("00-plan.json").is_file(),
        ),
        (
            Some(4),
            &Value::String("runtime".into()),
            &json!(evidence_dir.path()),
            &Value::Null,
            true,
            true,
            true,
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        fs::read_to_string(evidence_path)
            .expect("read retained evidence")
            .contains("Evidence retained after summary failure")
    );
    jina.finish();
}

#[test]
fn research_returns_evidence_when_high_strength_requires_more_than_one_item() {
    let jina = Fixture::start(
        200,
        "application/json",
        &jina_response(&rich_content("Only evidence")),
    );
    let environment = RunEnvironment::new(&fetch_only_config(&jina.url, true));
    let mut plan = valid_plan(json!([]));
    plan["intent_signals"]["cross_validation_need"] = json!("high");
    let plan_path = write_plan(&environment, plan);

    let output = environment.run(&[
        "research",
        "Verify https://example.test/only",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let manifest = read_recovery_manifest(&payload);
    let journal = read_only_journal(&environment);

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            &payload["synthesis_policy"],
            manifest["evidence_items"][0]["content"].is_null(),
            Path::new(manifest["plan_path"].as_str().expect("plan path")).is_file(),
            Path::new(
                manifest["evidence_items"][0]["path"]
                    .as_str()
                    .expect("evidence path"),
            )
            .is_file(),
            Path::new(
                manifest["unconsumed_candidates"]["path"]
                    .as_str()
                    .expect("candidate path")
            )
            .is_file(),
            journal["result"]["evidence_items"].as_array().map(Vec::len),
        ),
        (
            Some(5),
            &Value::String("evidence".into()),
            &Value::String("fetch_before_claim".into()),
            true,
            true,
            true,
            true,
            Some(1),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    jina.finish();
}

#[test]
fn research_quick_budget_clamps_high_strength_requirement_to_plan_capacity() {
    let jina = Fixture::start(
        200,
        "application/json",
        &jina_response(&rich_content("Only evidence")),
    );
    let environment = RunEnvironment::new(&fetch_only_config(&jina.url, true));
    let mut plan = valid_plan(json!([]));
    plan["intent_signals"]["source_authority_need"] = json!("high");
    let plan_path = write_plan(&environment, plan);

    let output = environment.run(&[
        "research",
        "Verify https://example.test/only",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
        "--budget",
        "quick",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    assert_eq!(
        (
            output.status.code(),
            payload["evidence_items"].as_array().map(Vec::len),
            String::from_utf8_lossy(&output.stderr)
                .contains("required evidence floor clamped from 2 to plan capacity 1"),
        ),
        (Some(0), Some(1), true),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    jina.finish();
}

#[test]
fn research_recovery_manifest_keeps_mixed_url_and_context7_evidence_identities() {
    let docs_content = rich_content("Context7 ownership docs");
    let context7 = context7_evidence_fixture(&docs_content);
    let jina = Fixture::start(
        200,
        "application/json",
        &jina_response(&rich_content("Known URL ownership docs")),
    );
    let config = context7_research_config(&context7.url, &jina.url);
    let environment = RunEnvironment::new(&config);
    let plan_path = write_plan(&environment, valid_plan(json!(["docs_search"])));
    let evidence_dir = tempfile::tempdir().expect("evidence dir");

    let output = environment.run(&[
        "research",
        "Rust ownership https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
        "--evidence-dir",
        evidence_dir.path().to_str().expect("UTF-8 evidence path"),
        "--format",
        "markdown",
        "--verbose",
    ]);
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    let summary: Value = serde_json::from_slice(
        &fs::read(evidence_dir.path().join("summary.json")).expect("read summary"),
    )
    .expect("parse summary");
    let url_evidence = &summary["evidence_items"][0];
    let docs_evidence = &summary["evidence_items"][1];

    assert_eq!(
        (
            output.status.code(),
            &url_evidence["url"],
            &url_evidence["library_id"],
            &docs_evidence["provider"],
            &docs_evidence["source_type"],
            &docs_evidence["url"],
            &docs_evidence["library_id"],
            summary["provider_attempts"]
                .as_array()
                .expect("attempts")
                .iter()
                .map(|attempt| attempt["provider"].as_str().expect("provider"))
                .collect::<Vec<_>>(),
        ),
        (
            Some(0),
            &Value::String(
                "https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html".into()
            ),
            &Value::Null,
            &Value::String("context7".into()),
            &Value::String("docs".into()),
            &Value::Null,
            &Value::String("/rust-lang/rust".into()),
            vec!["context7", "jina", "context7"],
        ),
        "summary: {summary}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout
            .contains("[e1](https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html)"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("- [e2] —"), "stdout: {stdout}");
    assert!(!stdout.contains("[e2]("), "stdout: {stdout}");
    assert!(
        fs::read_to_string(docs_evidence["path"].as_str().expect("evidence path"))
            .expect("read evidence")
            .contains("Context7 ownership docs")
    );
    let requests = context7.finish_all();
    assert!(requests[2].contains("resolve-library-id"));
    assert!(requests[5].contains("query-docs"));
    jina.finish();
}

#[test]
fn research_fetches_url_documentation_candidate_through_web_fetch() {
    let exa = Fixture::start(
        200,
        "application/json",
        &json!({
            "results": [{
                "title": "Ownership",
                "url": "https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html"
            }]
        })
        .to_string(),
    );
    let jina = Fixture::start(
        200,
        "application/json",
        &jina_response(&rich_content("Fetched ownership documentation")),
    );
    let config = format!(
        r#"
[providers.exa]
url = {:?}
keys = ["exa-key"]
timeout = 30

[providers.jina]
url = {:?}
keys = ["jina-key"]
timeout = 30

[capabilities.docs_search]
order = ["exa"]

[capabilities.web_fetch]
order = ["jina"]

[retry]
max_attempts = 1
multiplier = 1
max_wait = 0

[journal]
enabled = false
"#,
        exa.url, jina.url,
    );
    let environment = RunEnvironment::new(&config);
    let plan_path = write_plan(&environment, valid_plan(json!(["docs_search"])));

    let output = environment.run(&[
        "research",
        "Rust ownership",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
        "--verbose",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    assert_eq!(
        (
            output.status.code(),
            &payload["evidence_items"][0]["provider"],
            &payload["evidence_items"][0]["source_type"],
            payload["provider_attempts"]
                .as_array()
                .expect("provider attempts")
                .iter()
                .map(|attempt| attempt["seam"].as_str().expect("attempt seam"))
                .collect::<Vec<_>>(),
        ),
        (
            Some(0),
            &Value::String("jina".into()),
            &Value::String("fetched_page".into()),
            vec!["docs_search", "web_fetch"],
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(exa.finish().contains(r#""text":false"#));
    jina.finish();
}

#[test]
fn research_ignores_unsolicited_exa_text_and_fetches_the_url_candidate() {
    let exa = Fixture::start(
        200,
        "application/json",
        r#"{"results":[{"title":"Thin docs","url":"https://example.test/docs","text":"thin"}]}"#,
    );
    let jina = Fixture::start(
        200,
        "application/json",
        &jina_response(&rich_content("Must not be fetched")),
    );
    let config = format!(
        r#"
[providers.exa]
url = {:?}
keys = ["exa-key"]
timeout = 30

[providers.jina]
url = {:?}
keys = ["jina-key"]
timeout = 30

[capabilities.docs_search]
order = ["exa"]

[capabilities.web_fetch]
order = ["jina"]

[retry]
max_attempts = 1
multiplier = 1
max_wait = 0

[journal]
enabled = false
"#,
        exa.url, jina.url,
    );
    let environment = RunEnvironment::new(&config);
    let plan_path = write_plan(&environment, valid_plan(json!(["docs_search"])));

    let output = environment.run(&[
        "research",
        "Thin documentation",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
        "--verbose",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            payload["provider_attempts"]
                .as_array()
                .expect("provider attempts")
                .iter()
                .map(|attempt| attempt["seam"].as_str().expect("attempt seam"))
                .collect::<Vec<_>>(),
        ),
        (Some(0), &Value::Null, vec!["docs_search", "web_fetch"],),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(exa.finish().contains(r#""text":false"#));
    jina.finish();
}

#[test]
fn research_reports_one_terminal_gap_when_context7_is_the_only_failed_candidate() {
    let context7 = Fixture::start_sequence(vec![
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{}}}"#,
        )
        .with_session("library-session"),
        Response::json(202, ""),
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"results":[{"id":"/rust-lang/rust","title":"Rust"}]},"content":[]}}"#,
        ),
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{}}}"#,
        )
        .with_session("docs-session"),
        Response::json(202, ""),
        Response::new(500, "text/plain", "query-docs unavailable"),
    ]);
    let jina = Fixture::start_canary();
    let config = format!(
        r#"
[providers.context7]
url = {:?}
keys = ["context7-key"]
timeout = 30

[capabilities.docs_search]
order = ["context7"]

[providers.jina]
url = {:?}
keys = ["jina-key"]
timeout = 30

[capabilities.web_fetch]
order = ["jina"]

[retry]
max_attempts = 1
multiplier = 1
max_wait = 0

[journal]
enabled = false
"#,
        context7.url, jina.url,
    );
    let environment = RunEnvironment::new(&config);
    let plan_path = write_plan(&environment, valid_plan(json!(["docs_search"])));
    let evidence_dir = tempfile::tempdir().expect("evidence dir");

    let output = environment.run(&[
        "research",
        "Rust ownership",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
        "--evidence-dir",
        evidence_dir.path().to_str().expect("UTF-8 evidence path"),
        "--verbose",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let manifest = read_recovery_manifest(&payload);

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            &manifest["capability_gaps"],
            &manifest["coverage"]["gap_check"]["gaps"],
            payload["provider_attempts"].as_array().map(Vec::len),
        ),
        (
            Some(5),
            &Value::String("evidence".into()),
            &json!([]),
            &json!([{
                "subquestion_id": "sq1",
                "reason": "no verified evidence was collected after attempting 1 candidate URLs"
            }, {
                "subquestion_id": "",
                "reason": "research required 1 evidence items but obtained 0"
            }]),
            Some(2),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(evidence_dir.path().join("summary.json").is_file());
    assert!(Path::new(manifest["plan_path"].as_str().expect("plan path")).is_file());
    assert!(
        Path::new(
            manifest["unconsumed_candidates"]["path"]
                .as_str()
                .expect("candidates path")
        )
        .is_file()
    );
    context7.finish_all();
    assert!(jina.finish_all().is_empty());
}

#[test]
fn research_closes_a_context7_candidate_failure_with_web_evidence() {
    let context7 = Fixture::start_sequence(vec![
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{}}}"#,
        )
        .with_session("library-session"),
        Response::json(202, ""),
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"results":[{"id":"/rust-lang/rust","title":"Rust"}]},"content":[]}}"#,
        ),
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{}}}"#,
        )
        .with_session("docs-session"),
        Response::json(202, ""),
        Response::new(500, "text/plain", "query-docs unavailable"),
    ]);
    let tavily = Fixture::start(
        200,
        "application/json",
        r#"{"results":[{"title":"Web evidence","url":"https://example.test/rust"}]}"#,
    );
    let jina = Fixture::start(
        200,
        "application/json",
        &jina_response(&rich_content("Web fallback evidence")),
    );
    let config = format!(
        "{}\n[providers.context7]\nurl = {:?}\nkeys = [\"context7-key\"]\ntimeout = 30\n\n[capabilities.docs_search]\norder = [\"context7\"]\n",
        research_config(&tavily.url, &jina.url, false),
        context7.url,
    );
    let environment = RunEnvironment::new(&config);
    let plan_path = write_plan(
        &environment,
        valid_plan(json!(["docs_search", "web_search"])),
    );

    let output = environment.run(&[
        "research",
        "Rust ownership",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
        "--verbose",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let summary: Value = serde_json::from_slice(
        &fs::read(
            Path::new(payload["evidence_dir"].as_str().expect("evidence dir")).join("summary.json"),
        )
        .expect("read summary"),
    )
    .expect("parse summary");

    assert_eq!(
        (
            output.status.code(),
            &payload["gap_check"]["status"],
            &payload["capability_gaps"],
            summary["evidence_items"].as_array().map(Vec::len),
            payload["provider_attempts"].as_array().map(Vec::len),
        ),
        (
            Some(0),
            &Value::String("closed".into()),
            &json!([]),
            Some(1),
            Some(4),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    context7.finish_all();
    tavily.finish();
    jina.finish();
}

#[test]
fn research_budget_limits_fetched_evidence_to_quick_standard_and_deep_caps() {
    for (budget, expected, discovery_limit) in [("quick", 1, 3), ("standard", 2, 6), ("deep", 3, 9)]
    {
        let results = (1..=9)
            .map(|index| {
                json!({
                    "title": format!("Candidate {index}"),
                    "url": format!("https://example.test/{index}")
                })
            })
            .collect::<Vec<_>>();
        let tavily = Fixture::start(
            200,
            "application/json",
            &json!({"results": results}).to_string(),
        );
        let jina = Fixture::start_sequence(
            (1..=expected)
                .map(|index| {
                    Response::new(
                        200,
                        "application/json",
                        &jina_response(&rich_content(&format!("Fetched {index}"))),
                    )
                })
                .collect(),
        );
        let environment = RunEnvironment::new(&research_config(&tavily.url, &jina.url, false));
        let plan_path = write_plan(&environment, valid_plan(json!(["web_search"])));

        let output = environment.run(&[
            "research",
            "Budgeted evidence",
            "--plan",
            plan_path.to_str().expect("UTF-8 path"),
            "--budget",
            budget,
        ]);
        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

        assert_eq!(
            (
                output.status.code(),
                payload["evidence_items"].as_array().map(Vec::len),
            ),
            (Some(0), Some(expected)),
            "{budget}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let discovery_request = tavily.finish();
        assert!(
            discovery_request.contains(&format!("\"max_results\":{discovery_limit}")),
            "{budget}: {discovery_request}"
        );
        assert_eq!(jina.finish_all().len(), expected);
    }
}

#[test]
fn research_bounds_the_post_dedup_candidate_pool_to_the_subquestion_discovery_limit() {
    let results = (1..=9)
        .map(|index| {
            json!({
                "title": format!("Candidate {index}"),
                "url": format!("https://example.test/{index}")
            })
        })
        .collect::<Vec<_>>();
    let tavily = Fixture::start(
        200,
        "application/json",
        &json!({"results": results}).to_string(),
    );
    let jina = Fixture::start_parallel_sequence(
        (1..=6)
            .map(|_| Response::new(500, "text/plain", "fetch failed"))
            .collect(),
    );
    let environment = RunEnvironment::new(&research_config(&tavily.url, &jina.url, false));
    let plan_path = write_plan(&environment, valid_plan(json!(["web_search"])));

    let output = environment.run(&[
        "research",
        "Bounded candidates",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
        "--budget",
        "standard",
    ]);

    assert_eq!(output.status.code(), Some(5));
    let discovery_request = tavily.finish();
    assert!(discovery_request.contains("\"max_results\":6"));
    assert_eq!(jina.finish_all().len(), 6);
}

#[test]
fn research_standard_budget_covers_each_subquestion_before_extra_evidence() {
    let tavily = Fixture::start_sequence(vec![
        Response::new(
            200,
            "application/json",
            r#"{"results":[
                {"title":"First A","url":"https://example.test/first-a"},
                {"title":"First B","url":"https://example.test/first-b"},
                {"title":"First C","url":"https://example.test/first-c"}
            ]}"#,
        ),
        Response::new(
            200,
            "application/json",
            r#"{"results":[
                {"title":"Second A","url":"https://example.test/second-a"}
            ]}"#,
        ),
    ]);
    let jina = Fixture::start_sequence(
        (1..=3)
            .map(|index| {
                Response::new(
                    200,
                    "application/json",
                    &jina_response(&rich_content(&format!("Fetched {index}"))),
                )
            })
            .collect(),
    );
    let environment = RunEnvironment::new(&research_config(&tavily.url, &jina.url, false));
    let mut plan = valid_plan(json!(["web_search"]));
    plan["decomposition"]
        .as_array_mut()
        .expect("decomposition")
        .push(json!({
            "id": "sq2",
            "question": "What contrasting evidence is available?",
            "reason": "Cover the comparison target",
            "required_capabilities": ["web_search"]
        }));
    let plan_path = write_plan(&environment, plan);

    let output = environment.run(&[
        "research",
        "Compare two subjects",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
        "--budget",
        "standard",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            payload["evidence_items"]
                .as_array()
                .expect("evidence items")
                .iter()
                .map(|item| item["subquestion_ids"].clone())
                .collect::<Vec<_>>(),
            &payload["gap_check"]["status"],
        ),
        (
            Some(0),
            vec![json!(["sq1"]), json!(["sq2"]), json!(["sq1"])],
            &Value::String("closed".into()),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(tavily.finish_all().len(), 2);
    assert_eq!(jina.finish_all().len(), 3);
}

#[test]
fn research_defers_shared_candidates_until_prior_wave_reservations_resolve() {
    for first_succeeds in [false, true] {
        let tavily = Fixture::start_sequence(vec![
            Response::json(
                200,
                r#"{"results":[{"title":"First","url":"https://example.test/first"},{"title":"Shared","url":"https://example.test/shared"}]}"#,
            ),
            Response::json(
                200,
                r#"{"results":[{"title":"Shared again","url":"https://example.test/shared"}]}"#,
            ),
        ]);
        let first_response = if first_succeeds {
            Response::json(200, &jina_response(&rich_content("First evidence")))
        } else {
            Response::new(500, "text/plain", "first candidate failed")
        };
        let jina = Fixture::start_sequence(vec![
            first_response,
            Response::json(200, &jina_response(&rich_content("Shared evidence"))),
        ]);
        let environment = RunEnvironment::new(&research_config(&tavily.url, &jina.url, false));
        let mut plan = valid_plan(json!(["web_search"]));
        plan["decomposition"]
            .as_array_mut()
            .expect("decomposition")
            .push(json!({
                "id": "sq2",
                "question": "What corroborating evidence is available?",
                "reason": "Cover the second question",
                "required_capabilities": ["web_search"]
            }));
        let plan_path = write_plan(&environment, plan);

        let output = environment.run(&[
            "research",
            "Resolve shared evidence",
            "--plan",
            plan_path.to_str().expect("UTF-8 path"),
            "--budget",
            "quick",
        ]);
        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
        let evidence = payload["evidence_items"]
            .as_array()
            .expect("evidence items");
        let shared = evidence
            .iter()
            .find(|item| item["url"] == "https://example.test/shared")
            .expect("shared evidence");
        let mut shared_ids = shared["subquestion_ids"]
            .as_array()
            .expect("shared subquestion ids")
            .iter()
            .map(|id| id.as_str().expect("subquestion id"))
            .collect::<Vec<_>>();
        shared_ids.sort_unstable();
        let sq1_evidence = evidence
            .iter()
            .filter(|item| {
                item["subquestion_ids"]
                    .as_array()
                    .is_some_and(|ids| ids.iter().any(|id| id == "sq1"))
            })
            .count();

        assert_eq!(
            (
                output.status.code(),
                evidence.len(),
                shared_ids,
                sq1_evidence,
                payload["gap_check"]["status"].as_str(),
            ),
            if first_succeeds {
                (Some(0), 2, vec!["sq2"], 1, Some("closed"))
            } else {
                (Some(0), 1, vec!["sq1", "sq2"], 1, Some("closed"))
            },
            "first_succeeds={first_succeeds}, stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(tavily.finish_all().len(), 2);
        assert_eq!(jina.finish_all().len(), 2);
    }
}

#[test]
fn research_fetches_a_shared_discovery_url_once_and_covers_each_subquestion() {
    let tavily = Fixture::start_sequence(vec![
        Response::json(
            200,
            r#"{"results":[{"title":"Shared","url":"https://example.test/shared"}]}"#,
        ),
        Response::json(
            200,
            r#"{"results":[{"title":"Shared again","url":"https://example.test/shared"}]}"#,
        ),
    ]);
    let jina = Fixture::start(
        200,
        "application/json",
        &jina_response(&rich_content("Shared evidence")),
    );
    let environment = RunEnvironment::new(&research_config(&tavily.url, &jina.url, false));
    let mut plan = valid_plan(json!(["web_search"]));
    plan["decomposition"]
        .as_array_mut()
        .expect("decomposition")
        .push(json!({
            "id": "sq2",
            "question": "What corroborating evidence is available?",
            "reason": "Cover the second question",
            "required_capabilities": ["web_search"]
        }));
    let plan_path = write_plan(&environment, plan);

    let output = environment.run(&[
        "research",
        "Compare two questions",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            payload["evidence_items"].as_array().map(Vec::len),
            &payload["evidence_items"][0]["subquestion_ids"],
            &payload["gap_check"]["status"],
        ),
        (
            Some(0),
            Some(1),
            &json!(["sq1", "sq2"]),
            &Value::String("closed".into()),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(tavily.finish_all().len(), 2);
    assert_eq!(jina.finish_all().len(), 1);
}

#[test]
fn research_flattens_subquestions_into_one_ordered_concurrent_fanout() {
    let tavily = Fixture::start_parallel_sequence(vec![
        Response::new(
            200,
            "application/json",
            r#"{"results":[{"title":"First","url":"https://example.test/first"}]}"#,
        )
        .with_delay(Duration::from_secs(2)),
        Response::new(
            200,
            "application/json",
            r#"{"results":[{"title":"Second","url":"https://example.test/second"}]}"#,
        )
        .with_delay(Duration::from_secs(2)),
    ]);
    let jina = Fixture::start_parallel_sequence(vec![
        Response::new(
            200,
            "application/json",
            &jina_response(&rich_content("First fetched")),
        ),
        Response::new(
            200,
            "application/json",
            &jina_response(&rich_content("Second fetched")),
        ),
    ]);
    let environment = RunEnvironment::new(&research_config(&tavily.url, &jina.url, false));
    let mut plan = valid_plan(json!(["web_search"]));
    plan["decomposition"]
        .as_array_mut()
        .expect("decomposition")
        .push(json!({
            "id": "sq2",
            "question": "What contrasting evidence is available?",
            "reason": "Cover the comparison target",
            "required_capabilities": ["web_search"]
        }));
    let plan_path = write_plan(&environment, plan);

    let output = environment.run(&[
        "research",
        "Compare two subjects",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
        "--budget",
        "standard",
        "--timeout",
        "3",
        "--verbose",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            payload["evidence_items"]
                .as_array()
                .expect("evidence items")
                .iter()
                .map(|item| item["subquestion_ids"].clone())
                .collect::<Vec<_>>(),
            payload["provider_attempts"]
                .as_array()
                .expect("provider attempts")
                .iter()
                .map(|attempt| attempt["seam"].as_str().expect("attempt seam"))
                .collect::<Vec<_>>(),
        ),
        (
            Some(0),
            vec![json!(["sq1"]), json!(["sq2"])],
            vec!["web_search", "web_search", "web_fetch", "web_fetch"],
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(tavily.finish_all().len(), 2);
    assert_eq!(jina.finish_all().len(), 2);
}

#[test]
fn research_fetches_concurrently_without_reserving_over_the_subquestion_cap() {
    let tavily = Fixture::start(
        200,
        "application/json",
        r#"{"results":[
            {"title":"First","url":"https://example.test/first"},
            {"title":"Second","url":"https://example.test/second"},
            {"title":"Third","url":"https://example.test/third"}
        ]}"#,
    );
    let jina = Fixture::start_parallel_sequence(
        ["First fetched", "Second fetched"]
            .into_iter()
            .map(|label| {
                Response::new(
                    200,
                    "application/json",
                    &jina_response(&rich_content(label)),
                )
                .with_delay(Duration::from_secs(2))
            })
            .collect(),
    );
    let environment = RunEnvironment::new(&research_config(&tavily.url, &jina.url, false));
    let plan_path = write_plan(&environment, valid_plan(json!(["web_search"])));
    let started = Instant::now();

    let output = environment.run(&[
        "research",
        "Wave fetching",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
        "--budget",
        "standard",
        "--timeout",
        "3",
    ]);
    let elapsed = started.elapsed();
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            payload["evidence_items"]
                .as_array()
                .expect("evidence items")
                .iter()
                .map(|item| item["title"].as_str().expect("evidence title"))
                .collect::<Vec<_>>(),
        ),
        (Some(0), vec!["First", "Second"]),
        "elapsed: {elapsed:?}; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    tavily.finish();
    assert_eq!(jina.finish_all().len(), 2);
}

#[test]
fn research_discloses_a_subquestion_without_verified_evidence() {
    let tavily = Fixture::start_sequence(vec![
        Response::new(
            200,
            "application/json",
            r#"{"results":[
                {"title":"First A","url":"https://example.test/first-a"},
                {"title":"First B","url":"https://example.test/first-b"},
                {"title":"First C","url":"https://example.test/first-c"}
            ]}"#,
        ),
        Response::new(200, "application/json", r#"{"results":[]}"#),
    ]);
    let jina = Fixture::start_sequence(
        (1..=2)
            .map(|index| {
                Response::new(
                    200,
                    "application/json",
                    &jina_response(&rich_content(&format!("Fetched {index}"))),
                )
            })
            .collect(),
    );
    let environment = RunEnvironment::new(&research_config(&tavily.url, &jina.url, false));
    let mut plan = valid_plan(json!(["web_search"]));
    plan["decomposition"]
        .as_array_mut()
        .expect("decomposition")
        .push(json!({
            "id": "sq2",
            "question": "What contrasting evidence is available?",
            "reason": "Cover the comparison target",
            "required_capabilities": ["web_search"]
        }));
    let plan_path = write_plan(&environment, plan);

    let output = environment.run(&[
        "research",
        "Compare two subjects",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
        "--budget",
        "standard",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["gap_check"]["status"],
            &payload["gap_check"]["gaps"],
        ),
        (
            Some(0),
            &Value::String("degraded".into()),
            &json!([{
                "subquestion_id": "sq2",
                "reason": "no verified evidence was collected after attempting 0 candidate URLs"
            }]),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(tavily.finish_all().len(), 2);
    assert_eq!(jina.finish_all().len(), 2);
}

#[test]
fn research_fallback_off_executes_only_the_configured_chain_head() {
    let jina = Fixture::start(200, "application/json", &jina_response("thin"));
    let config = format!(
        r#"
[providers.jina]
url = {:?}
keys = ["jina-key"]
timeout = 30

[providers.tavily]
url = "http://127.0.0.1:9"
keys = ["tavily-key"]
timeout = 30

[capabilities.web_fetch]
order = ["jina", "tavily"]

[retry]
max_attempts = 1
multiplier = 1
max_wait = 0

[journal]
enabled = false
"#,
        jina.url
    );
    let environment = RunEnvironment::new(&config);
    let plan_path = write_plan(&environment, valid_plan(json!([])));

    let output = environment.run(&[
        "research",
        "Verify https://example.test/known",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
        "--fallback",
        "off",
        "--verbose",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            payload["provider_attempts"]
                .as_array()
                .expect("attempts")
                .iter()
                .map(|attempt| attempt["provider"].as_str().expect("provider"))
                .collect::<Vec<_>>(),
        ),
        (Some(5), &Value::String("quality".into()), vec!["jina"]),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    jina.finish();
}

#[test]
fn research_reports_quality_when_discovery_succeeds_but_all_fetches_are_thin() {
    let tavily = Fixture::start(
        200,
        "application/json",
        r#"{"results":[{"title":"Thin candidate","url":"https://example.test/thin"}]}"#,
    );
    let jina = Fixture::start(200, "application/json", &jina_response("thin"));
    let environment = RunEnvironment::new(&research_config(&tavily.url, &jina.url, false));
    let plan_path = write_plan(&environment, valid_plan(json!(["web_search"])));

    let output = environment.run(&[
        "research",
        "Discover thin evidence",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (output.status.code(), &payload["error_kind"]),
        (Some(5), &Value::String("quality".into())),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    tavily.finish();
    jina.finish();
}

#[test]
fn research_reports_evidence_when_discovery_succeeds_but_fetch_transport_fails() {
    let tavily = Fixture::start(
        200,
        "application/json",
        r#"{"results":[{"title":"Unavailable candidate","url":"https://example.test/down"}]}"#,
    );
    let jina = Fixture::start(500, "text/plain", "upstream unavailable");
    let environment = RunEnvironment::new(&research_config(&tavily.url, &jina.url, false));
    let plan_path = write_plan(&environment, valid_plan(json!(["web_search"])));

    let output = environment.run(&[
        "research",
        "Discover unavailable evidence",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (output.status.code(), &payload["error_kind"]),
        (Some(5), &Value::String("evidence".into())),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    tavily.finish();
    jina.finish();
}

#[test]
fn research_uses_one_hard_deadline_across_discovery_and_fetch() {
    let mut delayed = Response::new(
        200,
        "application/json",
        r#"{"results":[{"title":"Too late","url":"https://example.test/late"}]}"#,
    );
    delayed.delay = Duration::from_secs(3);
    let tavily = Fixture::start_sequence(vec![delayed]);
    let config = format!(
        r#"
[providers.tavily]
url = {:?}
keys = ["tavily-key"]
timeout = 30

[capabilities.web_search]
order = ["tavily"]

[retry]
max_attempts = 1
multiplier = 1
max_wait = 0

[journal]
enabled = true
"#,
        tavily.url
    );
    let environment = RunEnvironment::new(&config);
    let plan_path = write_plan(&environment, valid_plan(json!(["web_search"])));
    let started = Instant::now();

    let output = environment.run(&[
        "research",
        "Deadline",
        "--plan",
        plan_path.to_str().expect("UTF-8 path"),
        "--timeout",
        "1",
    ]);
    let elapsed = started.elapsed();
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let journal = read_only_journal(&environment);

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            &journal["execution"]["deadline_budget"]["exhausted"],
        ),
        (
            Some(4),
            &Value::String("timeout".into()),
            &Value::Bool(true)
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        elapsed < Duration::from_millis(2500),
        "hard deadline took {elapsed:?}"
    );
    tavily.finish();
}

// Test builders take ownership so nested call sites can pass JSON temporaries directly.
#[expect(clippy::needless_pass_by_value)]
fn valid_plan(capabilities: Value) -> Value {
    json!({
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
            "required_capabilities": capabilities
        }]
    })
}

// Test builders take ownership so nested call sites can pass JSON temporaries directly.
#[expect(clippy::needless_pass_by_value)]
fn write_plan(environment: &RunEnvironment, plan: Value) -> std::path::PathBuf {
    let path = environment.config_dir.join("plan.json");
    fs::write(&path, serde_json::to_vec(&plan).expect("encode plan")).expect("write plan");
    path
}

fn read_recovery_manifest(payload: &Value) -> Value {
    serde_json::from_slice(
        &fs::read(payload["summary_path"].as_str().expect("summary path"))
            .expect("read Recovery Manifest"),
    )
    .expect("parse Recovery Manifest")
}

fn context7_evidence_fixture(docs_content: &str) -> Fixture {
    Fixture::start_sequence(vec![
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{}}}"#,
        )
        .with_session("library-session"),
        Response::json(202, ""),
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"results":[{"id":"/rust-lang/rust","title":"Rust","description":"Rust docs"}]},"content":[]}}"#,
        ),
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{}}}"#,
        )
        .with_session("docs-session"),
        Response::json(202, ""),
        Response::json(
            200,
            &json!({
                "jsonrpc": "2.0",
                "id": 2,
                "result": {"content": [{"type": "text", "text": docs_content}]}
            })
            .to_string(),
        ),
    ])
}

fn context7_research_config(context7_url: &str, jina_url: &str) -> String {
    format!(
        r#"
[providers.context7]
url = {context7_url:?}
keys = ["context7-key"]
timeout = 30

[capabilities.docs_search]
order = ["context7"]

[providers.jina]
url = {jina_url:?}
keys = ["jina-key"]
timeout = 30

[capabilities.web_fetch]
order = ["jina"]

[retry]
max_attempts = 1
multiplier = 1
max_wait = 0

[journal]
enabled = false
"#
    )
}

fn rich_content(label: &str) -> String {
    (0..12)
        .map(|index| format!("{label} line {index} with independently fetched details."))
        .collect::<Vec<_>>()
        .join("\n")
}

fn jina_response(content: &str) -> String {
    json!({"data": {"content": content}}).to_string()
}

fn fetch_only_config(jina_url: &str, journal_enabled: bool) -> String {
    format!(
        r#"
[providers.jina]
url = {jina_url:?}
keys = ["jina-key"]
timeout = 30

[capabilities.web_fetch]
order = ["jina"]

[retry]
max_attempts = 1
multiplier = 1
max_wait = 0

[journal]
enabled = {journal_enabled}
"#
    )
}

fn research_config(tavily_url: &str, jina_url: &str, journal_enabled: bool) -> String {
    format!(
        r#"
[providers.tavily]
url = {tavily_url:?}
keys = ["tavily-key"]
timeout = 30

[providers.jina]
url = {jina_url:?}
keys = ["jina-key"]
timeout = 30

[capabilities.web_search]
order = ["tavily"]

[capabilities.web_fetch]
order = ["jina"]

[retry]
max_attempts = 1
multiplier = 1
max_wait = 0

[journal]
enabled = {journal_enabled}
"#
    )
}

fn shared_fetch_research_config(tavily_url: &str, firecrawl_url: &str, jina_url: &str) -> String {
    format!(
        r#"
[providers.tavily]
url = {tavily_url:?}
keys = ["tavily-key"]
timeout = 30

[providers.firecrawl]
url = {firecrawl_url:?}
keys = ["firecrawl-key"]
timeout = 30

[providers.jina]
url = {jina_url:?}
keys = ["jina-key"]
timeout = 30

[capabilities.web_search]
order = ["tavily"]

[capabilities.web_fetch]
order = ["tavily", "firecrawl", "jina"]

[retry]
max_attempts = 1
multiplier = 1
max_wait = 0

[journal]
enabled = true
"#
    )
}

fn bare_research_config(
    classifier_url: &str,
    tavily_url: &str,
    jina_url: &str,
    journal_enabled: bool,
) -> String {
    format!(
        r#"
[classifier]
url = {classifier_url:?}
keys = ["classifier-key"]
model = "classifier-model"
timeout = 30

{}
"#,
        research_config(tavily_url, jina_url, journal_enabled)
    )
}

fn bare_fetch_research_config(
    classifier_url: &str,
    jina_url: &str,
    journal_enabled: bool,
) -> String {
    format!(
        r#"
[classifier]
url = {classifier_url:?}
keys = ["classifier-key"]
model = "classifier-model"
timeout = 30

{}
"#,
        fetch_only_config(jina_url, journal_enabled)
    )
}

// Test builders take ownership so nested call sites can pass JSON temporaries directly.
#[expect(clippy::needless_pass_by_value)]
fn classifier_response(plan: Value) -> String {
    json!({
        "choices": [{
            "message": {
                "content": serde_json::to_string(&plan).expect("encode classifier plan")
            }
        }]
    })
    .to_string()
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

fn assert_file_backed_index_artifacts(
    payload: &Value,
    evidence_dir: &Path,
    evidence_body: &str,
    stdout: &[u8],
    journal: &Value,
) {
    let candidates_path = payload["unconsumed_candidates"]["path"]
        .as_str()
        .expect("candidate path");
    let evidence_path = payload["evidence_items"][0]["path"]
        .as_str()
        .expect("evidence path");
    for path in [
        payload["plan_path"].as_str().expect("plan path"),
        evidence_path,
        candidates_path,
        evidence_dir
            .join("summary.json")
            .to_str()
            .expect("summary path"),
    ] {
        assert!(Path::new(path).is_file(), "unreadable artifact: {path}");
    }
    assert!(
        fs::read_to_string(evidence_path)
            .expect("read evidence body")
            .contains(evidence_body)
    );
    for (surface, content) in [
        ("stdout", String::from_utf8_lossy(stdout).into_owned()),
        (
            "candidates",
            fs::read_to_string(candidates_path).expect("read candidates"),
        ),
        (
            "summary",
            fs::read_to_string(evidence_dir.join("summary.json")).expect("read summary"),
        ),
        ("journal", journal.to_string()),
    ] {
        assert!(
            !content.contains(evidence_body),
            "{surface} repeated persisted evidence content"
        );
    }
}
