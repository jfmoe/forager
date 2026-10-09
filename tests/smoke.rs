mod support;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use support::scholar::{fixture, ok};
use support::{Fixture, Response, run_command};

struct SmokeEnvironment {
    _root: tempfile::TempDir,
    config_dir: PathBuf,
    state_dir: PathBuf,
    home_dir: PathBuf,
    journal_dir: PathBuf,
}

impl SmokeEnvironment {
    fn new(config: impl FnOnce(&Path) -> String) -> Self {
        let root = tempfile::tempdir().expect("create isolated root");
        let config_home = root.path().join("xdg-config");
        let config_dir = config_home.join("forager");
        let state_dir = root.path().join("xdg-state");
        let home_dir = root.path().join("home");
        let journal_dir = root.path().join("journal");
        fs::create_dir_all(&config_dir).expect("create config directory");
        fs::create_dir_all(&home_dir).expect("create home directory");
        forager::config::ensure_private_directory(&config_dir)
            .expect("restrict config directory permissions");
        let mut config_file = forager::config::create_private_file(&config_dir.join("config.toml"))
            .expect("create private config file");
        config_file
            .write_all(config(&journal_dir).as_bytes())
            .expect("write config");
        Self {
            _root: root,
            config_dir,
            state_dir,
            home_dir,
            journal_dir,
        }
    }

    fn run(&self, arguments: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_forager"));
        command
            .args(arguments)
            .env_clear()
            .env(
                "XDG_CONFIG_HOME",
                self.config_dir.parent().expect("XDG config home"),
            )
            .env("XDG_STATE_HOME", &self.state_dir)
            .env("HOME", &self.home_dir)
            .env("NO_PROXY", "127.0.0.1,localhost");
        #[cfg(windows)]
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", system_root);
        }
        run_command(&mut command, None)
    }
}

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
            &json!({"ok": true, "provider_count": 13}),
            Some(13),
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
        "P1", "P2", "C01", "C02", "C03", "C04", "C05", "C06", "C07", "C08", "C09", "C10", "C11",
        "C12", "C13", "C14", "C15", "C16", "C17", "C18", "C19", "C20", "C21", "C22", "C23", "C24",
        "C25", "C26", "C27", "C28", "C29"
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
            &case(&failed_payload, "C02")["status"],
            &case(&failed_payload, "P1")["status"],
        ),
        (
            Some(4),
            &Value::String("live".into()),
            &Value::Bool(false),
            &json!({"passed": 0, "failed": 1, "deferred": 0, "unconfigured": 30, "skipped": 0}),
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
            &json!({"passed": 0, "failed": 0, "deferred": 1, "unconfigured": 30, "skipped": 0}),
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
            &json!({"passed": 1, "failed": 0, "deferred": 0, "unconfigured": 30, "skipped": 0}),
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
fn live_smoke_enforces_one_hard_deadline_across_retries() {
    let fixture = Fixture::start_sequence(vec![
        Response::json(200, "").with_delay(Duration::from_secs(3)),
    ]);
    let endpoint = format!("{}/v1", fixture.url);
    let environment = SmokeEnvironment::new(|journal_dir| minimal_config(&endpoint, journal_dir));

    let started = Instant::now();
    let output = environment.run(&["smoke", "--live", "--timeout", "1"]);
    let elapsed = started.elapsed();
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse live smoke JSON");

    assert_eq!(
        (
            output.status.code(),
            &case(&payload, "C01")["status"],
            &case(&payload, "C01")["attempts"],
        ),
        (
            Some(4),
            &Value::String("failed".into()),
            &Value::Number(1.into()),
        ),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        elapsed < Duration::from_millis(2500),
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
    let environment = SmokeEnvironment::new(|journal_dir| {
        format!(
            "[providers.arxiv_api]\nurl = \"{}/api/query\"\n[providers.ssrn_crossref]\nurl = \"{}\"\n[journal]\ndir = {journal_dir:?}\n",
            arxiv.url, crossref.url
        )
    });

    let output = environment.run(&["smoke", "--live", "--timeout", "10"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse live smoke JSON");
    let arxiv_requests = arxiv.finish_all();
    let crossref_requests = crossref.finish_all();

    assert_eq!(
        (
            &payload["summary"],
            ["C18", "C19", "C20", "C21"].map(|id| case(&payload, id)["status"].clone()),
            [
                &case(&payload, "C18")["platform"],
                &case(&payload, "C20")["platform"]
            ],
            arxiv_requests[0].contains("max_results=3"),
            arxiv_requests[1].contains("id_list=1706.03762"),
            crossref_requests[0].contains("rows=3"),
            crossref_requests[1].contains("/works/10.2139/ssrn.2042750"),
        ),
        (
            &json!({"passed": 4, "failed": 0, "deferred": 0, "unconfigured": 27, "skipped": 0}),
            [(); 4].map(|()| Value::String("passed".into())),
            [
                &Value::String("arxiv".into()),
                &Value::String("ssrn".into())
            ],
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
                "[providers.serpapi]\nurl = \"{}/search.json\"\nkeys = {keys}\n[journal]\ndir = {journal_dir:?}\n[platforms.arxiv]\norder = []\n[platforms.ssrn]\norder = []\n",
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

#[cfg(unix)]
#[test]
fn live_smoke_runs_the_browser_cases_only_when_the_order_lists_the_route() {
    use support::opencli::FakeOpenCli;

    let fake = FakeOpenCli::by_command(&[
        (
            "search",
            json!({
                "url": "https://papers.ssrn.com/searchresults.cfm?term=retrieval+augmented+generation",
                "term": "retrieval augmented generation",
                "search_state": {"scope":"title-abstract-keywords", "mode":"fuzzy", "author":"", "date":"All Time", "sort":"Relevancy", "request_url":"https://api.ssrn.com/papers/v1/papers/search/advanced?text=retrieval+augmented+generation&text_fields=title-abstract-keywords&search_mode=fuzzy&authors=&date=all_time&sort_by=&page=1"},
                "current_page": "1",
                "range": "Displaying results 1 to 1 of 1",
                "results": [{
                    "url": "https://papers.ssrn.com/sol3/papers.cfm?abstract_id=1",
                    "title": "Paper"
                }]
            }),
        ),
        (
            "paper",
            json!({
                "url": "https://papers.ssrn.com/sol3/papers.cfm?abstract_id=2042750",
                "canonical_url": "https://papers.ssrn.com/sol3/papers.cfm?abstract_id=2042750",
                "title": "Risk Premia Harvesting Through Dual Momentum"
            }),
        ),
    ]);
    let run = |order: &str| {
        let environment = SmokeEnvironment::new(|journal_dir| {
            format!(
                "{}[journal]\ndir = {journal_dir:?}\n[platforms.arxiv]\norder = []\n",
                fake.config(order)
            )
        });
        let output = environment.run(&["smoke", "--live", "--timeout", "60"]);
        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse live smoke JSON");
        ["C22", "C23"].map(|id| case(&payload, id)["status"].clone())
    };

    let disabled = run("[]");
    let enabled = run("[\"ssrn_browser\"]");

    assert_eq!(
        (disabled, enabled, fake.calls().len()),
        (
            [(); 2].map(|()| Value::String("unconfigured".into())),
            [(); 2].map(|()| Value::String("passed".into())),
            2
        )
    );
}

#[test]
fn live_smoke_runs_the_xiaohongshu_search_once_and_only_when_the_order_lists_the_route() {
    use support::opencli::FakeOpenCli;

    // A failing command would be retried for any other case.
    let fake = FakeOpenCli::answering("", "ok: false\nerror:\n  code: COMMAND_EXEC\n", 1);
    let run = |order: &str| {
        let environment = SmokeEnvironment::new(|journal_dir| {
            format!(
                "{}[journal]\ndir = {journal_dir:?}\n[platforms.arxiv]\norder = []\n[platforms.ssrn]\norder = []\n",
                fake.route_config("xiaohongshu_browser", "xiaohongshu", order)
            )
        });
        let output = environment.run(&["smoke", "--live", "--timeout", "60"]);
        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse live smoke JSON");
        let c27 = case(&payload, "C27");
        (c27["status"].clone(), c27["attempts"].clone())
    };

    let disabled = run("[]");
    let calls_while_disabled = fake.calls().len();
    let enabled = run("[\"xiaohongshu_browser\"]");
    let calls = fake.calls();

    assert_eq!(
        (disabled, calls_while_disabled, enabled, calls.len()),
        (
            (json!("unconfigured"), json!(0)),
            0,
            (json!("failed"), json!(1)),
            1
        )
    );
    assert_eq!(
        calls[0][..12],
        [
            "forager-xhs",
            "search",
            "--query",
            "咖啡",
            "--sort",
            "latest",
            "--note-type",
            "all",
            "--publish-time",
            "week",
            "--pages",
            "2"
        ]
    );
}

#[cfg(unix)]
const XHS_NOTE_ID: &str = "66f0a1b2c3d4e5f607100001";
#[cfg(unix)]
const XHS_TOKEN: &str = "ABsmokeToken1=";
#[cfg(unix)]
const XHS_COMMENTED_NOTE_ID: &str = "66f0a1b2c3d4e5f607100002";
#[cfg(unix)]
const XHS_COMMENTED_TOKEN: &str = "ABsmokeToken2=";

#[cfg(unix)]
fn xhs_page(url: &str) -> Value {
    json!({"url": url, "title": "小红书", "guest": false, "error_code": null, "notice": null, "blocked_status": null})
}

#[cfg(unix)]
fn xhs_note(id: &str, token: &str, comments: &str) -> Value {
    json!({
        "id": id,
        "model_type": "note",
        "xsec_token": token,
        "note_card": {"type": "normal", "display_title": "", "user": {"user_id": "5ff0e6410000000001008400", "nickname": "豆子"}, "interact_info": {"comment_count": comments}, "corner_tag_info": [{"type": "publish_time", "text": "3天前"}]}
    })
}

/// The C27 search as the adapter reports it after both filter clicks: on one page, a note with
/// 3 comments, then one with 1.2万, then one with 999.
#[cfg(unix)]
fn xhs_search(page: &Value) -> Value {
    json!({
        "page": page,
        "filter_clicks": 2,
        "timed_out": false,
        "body_missing": false,
        "responses": [{
            "click": 2,
            "request": {"keyword": "咖啡", "page": 1, "search_id": "2fhpxs5cks5vx6nvw1ar4@2fhpxsfv1qadn0lsg9txa", "filters": [
                {"tags": ["time_descending"], "type": "sort_type"},
                {"tags": ["不限"], "type": "filter_note_type"},
                {"tags": ["一周内"], "type": "filter_note_time"},
                {"tags": ["不限"], "type": "filter_note_range"},
                {"tags": ["不限"], "type": "filter_pos_distance"}
            ]},
            "body": {"code": 0, "success": true, "data": {"has_more": false, "items": [
                xhs_note(XHS_NOTE_ID, XHS_TOKEN, "3"),
                xhs_note(XHS_COMMENTED_NOTE_ID, XHS_COMMENTED_TOKEN, "1.2万"),
                xhs_note("66f0a1b2c3d4e5f607100003", "ABsmokeToken3=", "999")
            ]}}
        }]
    })
}

/// A top-level comment of the most-commented note; with `inline`, it carries that reply and has
/// more.
#[cfg(unix)]
fn xhs_comment(serial: u32, inline: Option<u32>) -> Value {
    let id = |serial: u32| format!("6a9eda8f00000000140{serial:05}");
    let replies = inline.map_or_else(Vec::new, |reply| {
        vec![json!({"id": id(reply), "content": "回复", "target_comment": {"id": id(serial)}})]
    });
    json!({
        "id": id(serial),
        "content": "评论",
        "sub_comment_cursor": inline.map(id),
        "sub_comment_has_more": inline.is_some(),
        "sub_comments": replies
    })
}

/// The C29 comments as the adapter reports them: `count` comments on one or two pages, and the
/// first page of replies of the first comment when it has more.
#[cfg(unix)]
fn xhs_comments(count: u32, expandable: bool) -> Value {
    let params = |cursor: &str| json!({"note_id": XHS_COMMENTED_NOTE_ID, "cursor": cursor});
    let comments = (1..=count)
        .map(|serial| xhs_comment(serial, (expandable && serial == 1).then_some(101)))
        .collect::<Vec<_>>();
    let mut responses = comments
        .chunks(10)
        .enumerate()
        .map(|(index, page)| {
            json!({
                "kind": "page",
                "params": params(if index == 0 { "" } else { "c1" }),
                "body": {"data": {"comments": page, "cursor": "c1", "has_more": index == 0 && count > 10}}
            })
        })
        .collect::<Vec<_>>();
    if expandable {
        responses.push(json!({
            "kind": "sub",
            "params": {"note_id": XHS_COMMENTED_NOTE_ID, "cursor": "6a9eda8f0000000014000101", "root_comment_id": "6a9eda8f0000000014000001"},
            "body": {"data": {"comments": [xhs_comment(102, None)], "cursor": "", "has_more": false}}
        }));
    }
    json!({
        "page": xhs_page(&format!("https://www.xiaohongshu.com/explore/{XHS_COMMENTED_NOTE_ID}?xsec_source=pc_search")),
        "timed_out": false,
        "body_missing": false,
        "responses": responses
    })
}

#[cfg(unix)]
fn xhs_smoke(fake: &support::opencli::FakeOpenCli) -> Value {
    let environment = SmokeEnvironment::new(|journal_dir| {
        format!(
            "{}[journal]\ndir = {:?}\n[platforms.arxiv]\norder = []\n[platforms.ssrn]\norder = []\n",
            fake.route_config(
                "xiaohongshu_browser",
                "xiaohongshu",
                "[\"xiaohongshu_browser\"]"
            ),
            journal_dir.display().to_string()
        )
    });
    let output = environment.run(&["smoke", "--live", "--timeout", "60"]);
    serde_json::from_slice(&output.stdout).expect("parse live smoke JSON")
}

#[cfg(unix)]
fn status_and_attempts(payload: &Value, id: &str) -> (Value, Value) {
    let case = case(payload, id);
    (case["status"].clone(), case["attempts"].clone())
}

#[cfg(unix)]
#[test]
fn live_smoke_fetches_the_first_xiaohongshu_search_result_and_reads_the_comments_of_the_most_commented()
 {
    use support::opencli::{FakeOpenCli, XHS_CONTRACT};

    let fake = FakeOpenCli::contract_by_command(
        XHS_CONTRACT,
        &[
            (
                "search",
                xhs_search(&xhs_page(
                    "https://www.xiaohongshu.com/search_result?keyword=%E5%92%96%E5%95%A1",
                )),
            ),
            (
                "note",
                json!({
                    "page": xhs_page(&format!("https://www.xiaohongshu.com/explore/{XHS_NOTE_ID}?xsec_source=pc_search")),
                    "note": {"noteId": XHS_NOTE_ID, "type": "normal", "title": "", "desc": "第一次手冲", "imageList": []},
                    "timed_out": false
                }),
            ),
            ("comments", xhs_comments(11, true)),
        ],
    );

    let payload = xhs_smoke(&fake);
    let calls = fake.calls();

    assert_eq!(
        (
            status_and_attempts(&payload, "C27"),
            status_and_attempts(&payload, "C28"),
            status_and_attempts(&payload, "C29"),
            calls.len(),
        ),
        (
            (json!("passed"), json!(1)),
            (json!("passed"), json!(1)),
            (json!("passed"), json!(1)),
            3
        ),
        "{payload}"
    );
    assert_eq!(
        calls[2][..10],
        [
            "forager-xhs",
            "comments",
            "--id",
            XHS_COMMENTED_NOTE_ID,
            "--xsec-token",
            XHS_COMMENTED_TOKEN,
            "--limit",
            "15",
            "--expand",
            "1"
        ]
    );
    assert_eq!(
        calls[1][..6],
        [
            "forager-xhs",
            "note",
            "--id",
            XHS_NOTE_ID,
            "--xsec-token",
            XHS_TOKEN
        ]
    );
}

#[cfg(unix)]
#[test]
fn a_xiaohongshu_login_wall_or_block_stops_every_later_xiaohongshu_case() {
    use support::opencli::{FakeOpenCli, XHS_CONTRACT};

    let mut logged_out =
        xhs_page("https://www.xiaohongshu.com/search_result?keyword=%E5%92%96%E5%95%A1");
    logged_out["guest"] = json!(true);
    let mut blocked = xhs_page("https://www.xiaohongshu.com/website-login/error?error_code=300017");
    blocked["error_code"] = json!("300017");
    blocked["notice"] = json!("安全限制");

    let outcomes = [logged_out, blocked].map(|page| {
        let fake = FakeOpenCli::contract_envelope(XHS_CONTRACT, "ok", &xhs_search(&page));
        let payload = xhs_smoke(&fake);
        (
            status_and_attempts(&payload, "C27"),
            ["C28", "C29"].map(|id| {
                let (status, attempts) = status_and_attempts(&payload, id);
                (status, attempts, case(&payload, id)["message"].clone())
            }),
            fake.calls().len(),
            payload["ok"].clone(),
        )
    });

    let not_started = (
        json!("skipped"),
        json!(0),
        json!(
            "not started: an earlier Xiaohongshu case met a login wall or a block, so this run stops all Xiaohongshu access"
        ),
    );
    let stopped = (
        (json!("failed"), json!(1)),
        [not_started.clone(), not_started],
        1,
        json!(false),
    );
    assert_eq!(outcomes, [stopped.clone(), stopped]);
}

#[cfg(unix)]
#[test]
fn live_smoke_leaves_the_comments_case_unverified_without_a_second_page_or_an_expansion() {
    use support::opencli::{FakeOpenCli, XHS_CONTRACT};

    let outcomes = [xhs_comments(10, true), xhs_comments(11, false)].map(|comments| {
        let fake = FakeOpenCli::contract_by_command(
            XHS_CONTRACT,
            &[
                (
                    "search",
                    xhs_search(&xhs_page(
                        "https://www.xiaohongshu.com/search_result?keyword=%E5%92%96%E5%95%A1",
                    )),
                ),
                ("comments", comments),
            ],
        );
        let payload = xhs_smoke(&fake);
        (
            status_and_attempts(&payload, "C29"),
            case(&payload, "C29")["message"].clone(),
            payload["ok"].clone(),
        )
    });

    assert_eq!(
        outcomes,
        [
            (
                (json!("skipped"), json!(1)),
                json!(
                    "not verified: the most-commented note of the Xiaohongshu search has 10 or fewer comments, so the second comment page was not read"
                ),
                json!(false)
            ),
            (
                (json!("skipped"), json!(1)),
                json!(
                    "not verified: no returned comment of the most-commented Xiaohongshu note had more replies to expand"
                ),
                json!(false)
            ),
        ]
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

fn case<'a>(payload: &'a Value, id: &str) -> &'a Value {
    payload["cases"]
        .as_array()
        .expect("live cases")
        .iter()
        .find(|case| case["id"] == id)
        .expect("registered live case")
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

[providers.openai_compatible]
url = {endpoint:?}
keys = ["openai-secret"]

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

[providers.anysearch]
url = {endpoint:?}
keys = ["anysearch-secret"]

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

// Platform routes need no credentials, so live smoke would reach the real endpoints unless
// the platform orders disable them.
const DISABLED_PLATFORMS: &str = "[platforms.arxiv]\norder = []\n[platforms.ssrn]\norder = []\n";

// Debug path formatting supplies the quoted and escaped TOML string literal required by fixtures.
#[expect(clippy::unnecessary_debug_formatting)]
fn p2_config(classifier_url: &str, journal_dir: &Path) -> String {
    format!(
        r#"
[classifier]
url = {classifier_url:?}
keys = ["classifier-secret"]
model = "classifier-model"

[providers.xai]
url = "http://127.0.0.1:9"
keys = ["xai-secret"]

[providers.exa]
url = "http://127.0.0.1:9"
keys = ["exa-secret"]

[providers.anysearch]
url = "http://127.0.0.1:9"
keys = ["anysearch-secret"]

[providers.jina]
url = "http://127.0.0.1:9"
keys = ["jina-secret"]

[capabilities.docs_search]
order = ["exa"]

[capabilities.vertical_search]
order = ["anysearch"]

[capabilities.web_fetch]
order = ["jina"]

{DISABLED_PLATFORMS}
[journal]
dir = {journal_dir:?}
"#
    )
}

fn provider_secrets() -> [&'static str; 9] {
    [
        "classifier-secret",
        "xai-secret",
        "openai-secret",
        "exa-secret",
        "tavily-secret",
        "firecrawl-secret",
        "jina-secret",
        "context7-secret",
        "anysearch-secret",
    ]
}
