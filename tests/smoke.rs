mod support;

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use support::hn::{search_page, story_hit, story_item, story_tree};
use support::scholar::{fixture, ok};
use support::smoke::{DISABLED_PLATFORMS, SmokeEnvironment, case};
use support::{Fixture, Response, run_command};

#[test]
fn offline_smoke_reports_local_readiness_without_contacting_provider_endpoints() {
    let fixture = Fixture::start_canary();
    let environment =
        SmokeEnvironment::new(|journal_dir| complete_config(&fixture.url, journal_dir));

    let output = environment.run(&["smoke"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse smoke JSON");
    let network_was_contacted = !fixture.finish_all().is_empty();
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
            &json!({"ok": true, "provider_count": 17}),
            Some(17),
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
fn live_smoke_lists_exactly_the_specification_case_registry_without_l0_doctor_gates() {
    let endpoint = "http://127.0.0.1:9";
    let environment = SmokeEnvironment::new(|journal_dir| minimal_config(endpoint, journal_dir));

    let output = environment.run(&["smoke", "--live", "--list"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse live registry JSON");
    let expected = json!([
        "P1", "P2", "C01", "C04", "C05", "C06", "C07", "C08", "C09", "C10", "C11", "C12", "C13",
        "C17", "C18", "C19", "C20", "C21", "C24", "C25", "C26", "C27", "C28", "C29", "C30", "C31",
        "C32", "C33", "C34", "C35", "C36", "C37", "C38", "C39", "C40", "C41", "C42", "C43", "C44",
        "C45", "C46", "C47", "C48", "C49"
    ]);

    assert_eq!(
        (
            output.status.code(),
            &payload["mode"],
            &payload["registered_case_ids"],
            &payload["specification_case_ids"],
        ),
        (
            Some(0),
            &Value::String("live_registry".into()),
            &expected,
            &expected,
        ),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(payload["cases"].as_array().is_some_and(|cases| {
        cases
            .iter()
            .all(|case| case["id"].as_str().is_some_and(|id| !id.starts_with("L0")))
    }));
}

#[test]
fn live_smoke_retries_configured_cases_and_distinguishes_failure_deferral_and_unconfigured_cases() {
    let endpoint = "http://127.0.0.1:9";
    let environment = SmokeEnvironment::new(|journal_dir| minimal_config(endpoint, journal_dir));

    let failed = environment.run(&["smoke", "--live", "--timeout", "5"]);
    let failed_payload: Value =
        serde_json::from_slice(&failed.stdout).expect("parse failed live smoke JSON");
    let failed_c01 = case(&failed_payload, "C01");

    assert_eq!(
        (
            failed.status.code(),
            &failed_payload["mode"],
            &failed_payload["ok"],
            &failed_payload["summary"],
            &failed_c01["status"],
            &failed_c01["attempts"],
            &case(&failed_payload, "C04")["status"],
            &case(&failed_payload, "P1")["status"],
        ),
        (
            Some(4),
            &Value::String("live".into()),
            &Value::Bool(false),
            &json!({"passed": 0, "failed": 1, "deferred": 0, "unconfigured": 43, "skipped": 0}),
            &Value::String("failed".into()),
            &Value::Number(3.into()),
            &Value::String("unconfigured".into()),
            &Value::String("unconfigured".into()),
        ),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&failed.stdout),
        String::from_utf8_lossy(&failed.stderr)
    );

    let outage_evidence = format!("C01={endpoint}?token=outage-secret");
    let deferred = environment.run(&[
        "smoke",
        "--live",
        "--timeout",
        "15",
        "--outage-evidence",
        &outage_evidence,
    ]);
    let deferred_payload: Value =
        serde_json::from_slice(&deferred.stdout).expect("parse deferred live smoke JSON");
    let deferred_c01 = case(&deferred_payload, "C01");

    assert_eq!(
        (
            deferred.status.code(),
            &deferred_payload["ok"],
            &deferred_payload["summary"],
            &deferred_c01["status"],
            &deferred_c01["attempts"],
            &deferred_c01["outage_evidence"],
            deferred_c01["checked_at_unix_seconds"].as_u64().is_some(),
        ),
        (
            Some(4),
            &Value::Bool(false),
            &json!({"passed": 0, "failed": 0, "deferred": 1, "unconfigured": 43, "skipped": 0}),
            &Value::String("deferred".into()),
            &Value::Number(3.into()),
            &Value::String("http://127.0.0.1:9?token=********".into()),
            true,
        ),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&deferred.stdout),
        String::from_utf8_lossy(&deferred.stderr)
    );
    assert!(!String::from_utf8_lossy(&deferred.stdout).contains("outage-secret"));
}

#[test]
fn live_smoke_runs_a_case_that_fails_on_auth_once() {
    let fixture = Fixture::start_repeating(Response::json(401, r#"{"error":"invalid key"}"#));
    let endpoint = format!("{}/v1", fixture.url);
    let environment = SmokeEnvironment::new(|journal_dir| minimal_config(&endpoint, journal_dir));

    let output = environment.run(&["smoke", "--live", "--timeout", "10"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse live smoke JSON");

    assert_eq!(
        (
            &case(&payload, "C01")["status"],
            &case(&payload, "C01")["attempts"],
            fixture.finish_all().len(),
        ),
        (&Value::String("failed".into()), &Value::Number(1.into()), 1),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn independent_outage_probe_reports_a_failed_same_endpoint_request() {
    let endpoint = "http://127.0.0.1:9";
    let environment = SmokeEnvironment::new(|journal_dir| minimal_config(endpoint, journal_dir));

    let output = environment.run(&[
        "smoke",
        "--probe",
        "OUTAGE",
        "--probe-url",
        endpoint,
        "--probe-timeout",
        "1",
    ]);

    let payload = serde_json::from_slice::<Value>(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "parse outage probe JSON: {error}; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert_eq!(
        (output.status.code(), payload,),
        (Some(4), json!({"outage": true})),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn internal_smoke_probe_does_not_add_a_top_level_command() {
    let mut command = Command::new(env!("CARGO_BIN_EXE_forager"));
    command.arg("__smoke-probe");
    let output = run_command(&mut command, None);

    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn live_smoke_passes_a_configured_case_only_after_a_zero_parseable_nonempty_terminal() {
    let response_body = format!(
        "data: {}\n\n",
        json!({
            "type": "response.completed",
            "response": {
                "output": [{
                    "content": [{
                        "type": "output_text",
                        "text": "Rust stable release",
                        "annotations": [{
                            "type": "url_citation",
                            "url": "https://example.test/rust",
                            "title": "Rust"
                        }]
                    }]
                }]
            }
        })
    );
    let fixture = Fixture::start(200, "text/event-stream", &response_body);
    let endpoint = format!("{}/v1", fixture.url);
    let environment = SmokeEnvironment::new(|journal_dir| minimal_config(&endpoint, journal_dir));

    let output = environment.run(&["smoke", "--live", "--timeout", "2"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse live smoke JSON");

    assert_eq!(
        (
            output.status.code(),
            &payload["summary"],
            &case(&payload, "C01")["status"],
            &case(&payload, "C01")["attempts"],
        ),
        (
            Some(4),
            &json!({"passed": 1, "failed": 0, "deferred": 0, "unconfigured": 43, "skipped": 0}),
            &Value::String("passed".into()),
            &Value::Number(1.into()),
        ),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.finish();
}

#[test]
fn live_smoke_drains_large_child_output_without_false_timeout() {
    let response_body = format!(
        "data: {}\n\n",
        json!({
            "type": "response.completed",
            "response": {
                "output": [{
                    "content": [{
                        "type": "output_text",
                        "text": "x".repeat(256 * 1024),
                        "annotations": [{
                            "type": "url_citation",
                            "url": "https://example.test/rust",
                            "title": "Rust"
                        }]
                    }]
                }]
            }
        })
    );
    let fixture = Fixture::start(200, "text/event-stream", &response_body);
    let endpoint = format!("{}/v1", fixture.url);
    let environment = SmokeEnvironment::new(|journal_dir| minimal_config(&endpoint, journal_dir));

    let output = environment.run(&["smoke", "--live", "--timeout", "3"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse live smoke JSON");

    assert_eq!(
        (
            output.status.code(),
            &case(&payload, "C01")["status"],
            &case(&payload, "C01")["attempts"],
        ),
        (
            Some(4),
            &Value::String("passed".into()),
            &Value::Number(1.into()),
        ),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.finish();
}

#[test]
fn live_smoke_enforces_one_hard_deadline_and_reports_the_cases_it_left_no_time_for() {
    let fixture = Fixture::start_sequence(vec![
        Response::json(200, "").with_delay(Duration::from_secs(6)),
    ]);
    let endpoint = format!("{}/v1", fixture.url);
    let environment = SmokeEnvironment::new(|journal_dir| {
        format!(
            "{}[providers.tavily]\nurl = \"http://127.0.0.1:9\"\nkeys = [\"tavily-secret\"]\n",
            minimal_config(&endpoint, journal_dir)
        )
    });

    let started = Instant::now();
    let output = environment.run(&["smoke", "--live", "--timeout", "1"]);
    let elapsed = started.elapsed();
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse live smoke JSON");

    assert_eq!(
        (
            output.status.code(),
            &case(&payload, "C01")["status"],
            &case(&payload, "C01")["attempts"],
            &case(&payload, "C05")["status"],
            &case(&payload, "C05")["attempts"],
            &case(&payload, "C05")["message"],
        ),
        (
            Some(4),
            &Value::String("failed".into()),
            &Value::Number(1.into()),
            &Value::String("skipped".into()),
            &Value::Number(0.into()),
            &Value::String(
                "not started: the live smoke deadline elapsed before this case began".into()
            ),
        ),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    // The provider answers after 6 s; finishing well before that proves the 1 s deadline.
    assert!(
        elapsed < Duration::from_secs(4),
        "live smoke exceeded its hard deadline: {elapsed:?}"
    );
    fixture.finish();
}

#[test]
fn live_smoke_p2_does_not_write_evidence_into_the_configured_journal() {
    let classifier_response = json!({
        "choices": [{
            "message": {
                "content": json!({
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
                        "required_capabilities": []
                    }]
                })
                .to_string()
            }
        }]
    })
    .to_string();
    let classifier = Fixture::start_repeating(Response::json(200, &classifier_response));
    let environment = SmokeEnvironment::new(|journal_dir| p2_config(&classifier.url, journal_dir));

    let output = environment.run(&["smoke", "--live", "--timeout", "3"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse live smoke JSON");

    assert_eq!(
        (
            &case(&payload, "P2")["status"],
            &case(&payload, "P2")["attempts"]
        ),
        (&Value::String("failed".into()), &Value::Number(3.into())),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !environment
            .journal_dir
            .join("live-smoke-p2-evidence")
            .exists(),
        "P2 smoke evidence polluted the configured journal"
    );
    let classifier_requests = classifier.finish_all();
    assert!(
        classifier_requests.len() >= 3,
        "P2 smoke did not exercise all three classifier attempts"
    );
}

fn single_entry_feed(id: &str) -> String {
    format!(
        r#"<feed xmlns:opensearch="http://a9.com/-/spec/opensearch/1.1/" xmlns="http://www.w3.org/2005/Atom">
  <opensearch:totalResults>1</opensearch:totalResults>
  <entry><id>http://arxiv.org/abs/{id}</id><title>Paper</title><summary>Abstract.</summary></entry>
</feed>"#
    )
}

fn crossref_work(id: &str) -> Value {
    json!({"DOI": format!("10.2139/ssrn.{id}"), "title": ["Paper"]})
}

fn crossref_response(message_type: &str, message: &Value) -> Response {
    Response::json(
        200,
        &json!({"status": "ok", "message-type": message_type, "message": message}).to_string(),
    )
}

#[test]
fn live_smoke_runs_the_platform_cases_through_their_configured_route() {
    let atom = |body: &str| Response::new(200, "application/atom+xml", body);
    let arxiv = Fixture::start_sequence(vec![
        atom(&single_entry_feed("2005.11401v4")),
        atom(&single_entry_feed("1706.03762v7")),
    ]);
    let crossref = Fixture::start_sequence(vec![
        crossref_response(
            "work-list",
            &json!({"total-results": 1, "items": [crossref_work("1")]}),
        ),
        crossref_response("work", &crossref_work("2042750")),
    ]);
    let algolia = Fixture::start_sequence(vec![
        search_page(&[story_hit()], 0, 1),
        support::hn::json(&story_tree(&[])),
        search_page(&[story_hit()], 0, 1),
    ]);
    let firebase = Fixture::start_sequence(vec![
        support::hn::json(&story_item(&[])),
        Response::json(200, "[8863]"),
    ]);
    let environment = SmokeEnvironment::new(|journal_dir| {
        format!(
            "[providers.arxiv_api]\nurl = \"{}/api/query\"\n[providers.ssrn_crossref]\nurl = \"{}\"\n[providers.hn_api]\nurl = \"{}\"\nfirebase_url = \"{}\"\n[platforms.polymarket]\norder = []\n[journal]\ndir = {journal_dir:?}\n",
            arxiv.url, crossref.url, algolia.url, firebase.url
        )
    });

    let output = environment.run(&["smoke", "--live", "--timeout", "20"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse live smoke JSON");
    let arxiv_requests = arxiv.finish_all();
    let crossref_requests = crossref.finish_all();
    let algolia_requests = algolia.finish_all();
    firebase.finish_all();

    assert_eq!(
        (
            &payload["summary"],
            ["C18", "C19", "C20", "C21", "C33", "C34", "C35"]
                .map(|id| case(&payload, id)["status"].clone()),
            [
                &case(&payload, "C18")["platform"],
                &case(&payload, "C20")["platform"]
            ],
            arxiv_requests[0].contains("max_results=3"),
            arxiv_requests[1].contains("id_list=1706.03762"),
            crossref_requests[0].contains("rows=3"),
            crossref_requests[1].contains("/works/10.2139/ssrn.2042750"),
            algolia_requests[0].starts_with("GET /search?query=rust&hitsPerPage=5 "),
            algolia_requests[1].starts_with("GET /items/8863 "),
        ),
        (
            &json!({"passed": 7, "failed": 0, "deferred": 0, "unconfigured": 37, "skipped": 0}),
            [(); 7].map(|()| Value::String("passed".into())),
            [
                &Value::String("arxiv".into()),
                &Value::String("ssrn".into())
            ],
            true,
            true,
            true,
            true,
            true,
            true,
        ),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn live_smoke_runs_the_scholar_cases_only_with_a_serpapi_key() {
    let serpapi = Fixture::start_sequence(vec![
        ok(&fixture("search.json")),
        ok(&fixture("cluster.json")),
        ok(&fixture("cited_by.json")),
    ]);
    let run = |keys: &str| {
        let environment = SmokeEnvironment::new(|journal_dir| {
            format!(
                "[providers.serpapi]\nurl = \"{}/search.json\"\nkeys = {keys}\n[journal]\ndir = {journal_dir:?}\n[platforms.arxiv]\norder = []\n[platforms.hn]\norder = []\n[platforms.polymarket]\norder = []\n[platforms.ssrn]\norder = []\n",
                serpapi.url
            )
        });
        let output = environment.run(&["smoke", "--live", "--timeout", "10"]);
        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse live smoke JSON");
        ["C24", "C25", "C26"].map(|id| case(&payload, id)["status"].clone())
    };

    let without_key = run("[]");
    let with_key = run("[\"test-serpapi-key\"]");
    let requests = serpapi.finish_all();

    assert_eq!(
        (
            without_key,
            with_key,
            requests.len(),
            requests[0].contains("engine=google_scholar") && requests[0].contains("num=3"),
            requests[1].contains("cluster=18208131694456651388"),
            requests[2].contains("cites=18208131694456651388") && requests[2].contains("num=3"),
        ),
        (
            [(); 3].map(|()| Value::String("unconfigured".into())),
            [(); 3].map(|()| Value::String("passed".into())),
            3,
            true,
            true,
            true,
        )
    );
}

#[test]
fn live_smoke_runs_the_x_cases_through_the_xai_route_only_when_the_order_lists_it() {
    let response = |name: &str| support::x::completed(&support::x::fixture(name));
    let xai = Fixture::start_sequence(vec![
        response("ask.json"),
        response("search.json"),
        response("reply_thread.json"),
        response("ask.json"),
        response("ask.json"),
    ]);
    let run = |order: &str| {
        let environment = SmokeEnvironment::new(|journal_dir| {
            format!(
                "[providers.xai]\nurl = \"{}\"\nkeys = [\"xai-secret\"]\n[journal]\ndir = {journal_dir:?}\n[platforms.arxiv]\norder = []\n[platforms.hn]\norder = []\n[platforms.polymarket]\norder = []\n[platforms.ssrn]\norder = []\n[platforms.x]\norder = {order}\n",
                xai.url
            )
        });
        let output = environment.run(&["smoke", "--live", "--timeout", "10"]);
        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse live smoke JSON");
        ["C30", "C31", "C32"].map(|id| case(&payload, id)["status"].clone())
    };

    let enabled = run("[\"xai\"]");
    let disabled = run("[]");
    let requests = xai
        .finish_all()
        .iter()
        .map(|request| support::request_json(request))
        .collect::<Vec<_>>();

    assert_eq!(
        (
            enabled,
            disabled,
            requests.len(),
            requests[1]["text"]["format"]["name"].clone(),
            requests[2]["input"][0]["content"]
                .as_str()
                .is_some_and(|input| input.contains("2098634733248720992")),
            requests[3]["tools"].clone(),
        ),
        (
            [(); 3].map(|()| Value::String("passed".into())),
            [(); 3].map(|()| Value::String("unconfigured".into())),
            5,
            json!("x_posts"),
            true,
            json!([{"type": "x_search"}]),
        )
    );
}

#[cfg(unix)]
#[test]
fn live_smoke_runs_the_xiaohongshu_search_once_and_only_when_the_order_lists_the_route() {
    use support::browser::{StandIn, mark_logged_in};

    // A failing command would be retried for any other case.
    let stand_in = StandIn::failing("no CDP here");
    let run = |order: &str| {
        let environment = SmokeEnvironment::new(|journal_dir| {
            format!(
                "{}[journal]\ndir = {journal_dir:?}\n[platforms.arxiv]\norder = []\n[platforms.hn]\norder = []\n[platforms.polymarket]\norder = []\n[platforms.ssrn]\norder = []\n",
                stand_in.xiaohongshu_config(order)
            )
        });
        mark_logged_in(&environment.state_dir, "xiaohongshu");
        let output = environment.run(&["smoke", "--live", "--timeout", "60"]);
        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse live smoke JSON");
        ["C27", "C28", "C29"].map(|id| {
            let case = case(&payload, id);
            (case["status"].clone(), case["attempts"].clone())
        })
    };

    let disabled = run("[]");
    let launches_while_disabled = stand_in.launches().len();
    let enabled = run("[\"xiaohongshu_browser\"]");

    let unconfigured = (json!("unconfigured"), json!(0));
    let not_started = (json!("skipped"), json!(0));
    assert_eq!(
        (
            disabled,
            launches_while_disabled,
            enabled,
            stand_in.launches().len()
        ),
        (
            [unconfigured.clone(), unconfigured.clone(), unconfigured],
            0,
            [
                (json!("failed"), json!(1)),
                not_started.clone(),
                not_started
            ],
            1
        )
    );
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

// Debug path formatting supplies the quoted and escaped TOML string literal required by fixtures.
#[expect(clippy::unnecessary_debug_formatting)]
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

[journal]
dir = {journal_dir:?}
"#
    )
}

// Debug path formatting supplies the quoted and escaped TOML string literal required by fixtures.
#[expect(clippy::unnecessary_debug_formatting)]
fn minimal_config(endpoint: &str, journal_dir: &Path) -> String {
    format!(
        "[providers.xai]\nurl = {endpoint:?}\nkeys = [\"xai-secret\"]\n{DISABLED_PLATFORMS}[journal]\ndir = {journal_dir:?}\n"
    )
}

// Debug path formatting supplies the quoted and escaped TOML string literal required by fixtures.
#[expect(clippy::unnecessary_debug_formatting)]
fn p2_config(classifier_url: &str, journal_dir: &Path) -> String {
    format!(
        r#"
[classifier]
url = {classifier_url:?}
keys = ["classifier-secret"]
model = "classifier-model"

[providers.exa]
url = "http://127.0.0.1:9"
keys = ["exa-secret"]

[providers.tavily]
url = "http://127.0.0.1:9"
keys = ["tavily-secret"]

[providers.jina]
url = "http://127.0.0.1:9"
keys = ["jina-secret"]

[capabilities.docs_search]
order = ["exa"]

[capabilities.web_search]
order = ["tavily"]

[capabilities.web_fetch]
order = ["jina"]

{DISABLED_PLATFORMS}
[journal]
dir = {journal_dir:?}
"#
    )
}

fn provider_secrets() -> [&'static str; 7] {
    [
        "classifier-secret",
        "xai-secret",
        "exa-secret",
        "tavily-secret",
        "firecrawl-secret",
        "jina-secret",
        "context7-secret",
    ]
}
