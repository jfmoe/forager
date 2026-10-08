mod support;

use serde_json::Value;

use std::time::Duration;

use support::doctor::{assert_deep_success, reachable_responses, shallow_config};
use support::opencli::FakeOpenCli;
use support::{Fixture, RunEnvironment};

const EMPTY_ARXIV_FEED: &str = r#"<feed xmlns:opensearch="http://a9.com/-/spec/opensearch/1.1/" xmlns="http://www.w3.org/2005/Atom"><opensearch:totalResults>0</opensearch:totalResults></feed>"#;

#[test]
fn arxiv_api_deep_doctor_executes_the_registry_search_probe_without_credentials() {
    let fixture = Fixture::start(200, "application/atom+xml", EMPTY_ARXIV_FEED);
    let environment = RunEnvironment::new(&format!(
        "[providers.arxiv_api]\nurl = \"{}/api/query\"\n",
        fixture.url
    ));

    let output = environment.run(&["doctor", "--provider", "arxiv_api"]);
    assert_deep_success(&output, "arxiv_api", &[("search", "http")]);
    let request = fixture.finish();
    assert!(request.starts_with("GET /api/query?"), "{request}");
}

#[test]
fn serpapi_deep_doctor_runs_one_keyed_scholar_search() {
    let body = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/scholar/search.json"
    ))
    .expect("read scholar fixture");
    let fixture = Fixture::start_json(200, &body);
    let environment = RunEnvironment::new(&format!(
        "[providers.serpapi]\nurl = \"{}/search.json\"\nkeys = [\"test-serpapi-key\"]\n",
        fixture.url
    ));

    let output = environment.run(&["doctor", "--provider", "serpapi"]);
    assert_deep_success(&output, "serpapi", &[("search", "http")]);
    let request = fixture.finish();
    assert!(
        request.starts_with("GET /search.json?engine=google_scholar&")
            && request.contains("api_key=test-serpapi-key"),
        "{request}"
    );
}

#[test]
fn serpapi_deep_doctor_without_keys_fails_before_any_request() {
    let fixture = Fixture::start_canary();
    let environment = RunEnvironment::new(&format!(
        "[providers.serpapi]\nurl = \"{}/search.json\"\n",
        fixture.url
    ));

    let output = environment.run(&["doctor", "--provider", "serpapi"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");

    assert_eq!(
        (
            output.status.code(),
            &payload["configured"],
            &payload["error_kind"],
            fixture.finish_all().len(),
        ),
        (
            Some(3),
            &Value::Bool(false),
            &Value::String("config".into()),
            0,
        )
    );
}

#[test]
fn shallow_doctor_reaches_serpapi_with_a_keyless_get() {
    let fixture = Fixture::start_sequence(reachable_responses(3));
    let environment = RunEnvironment::new(&format!(
        "[providers.arxiv_api]\nurl = {url:?}\n[providers.ssrn_crossref]\nurl = {url:?}\n\
         [providers.serpapi]\nurl = \"{url}/search.json\"\nkeys = [\"test-serpapi-key\"]\n",
        url = fixture.url
    ));

    let output = environment.run(&["doctor"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");
    let serpapi = payload["providers"]
        .as_array()
        .expect("providers")
        .iter()
        .find(|provider| provider["provider"] == "serpapi")
        .expect("serpapi status");
    let requests = fixture.finish_all();

    assert_eq!(
        (
            output.status.code(),
            &serpapi["configured"],
            &serpapi["reachable"],
            requests
                .iter()
                .filter_map(|request| request.lines().next())
                .filter(|line| line.contains("/search.json"))
                .collect::<Vec<_>>(),
        ),
        (
            Some(0),
            &Value::Bool(true),
            &Value::Bool(true),
            vec!["GET /search.json HTTP/1.1"],
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Records a reservation a minute ahead, as if another process had just reserved the window.
fn reserve_arxiv_window(environment: &RunEnvironment) {
    let reserved_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("wall clock")
        .as_millis()
        + Duration::from_mins(1).as_millis();
    let directory = environment.state_dir.join("forager");
    std::fs::create_dir_all(&directory).expect("create state directory");
    std::fs::write(
        directory.join("rate_limit_state.json"),
        serde_json::json!({
            "schema_version": 1,
            "routes": {"arxiv_api": {"reserved_at_ms": reserved_at_ms}}
        })
        .to_string(),
    )
    .expect("write rate limit state");
}

#[test]
fn arxiv_api_deep_doctor_probe_waits_for_the_request_window() {
    let fixture = Fixture::start_canary();
    let environment = RunEnvironment::new(&format!(
        "[providers.arxiv_api]\nurl = \"{}/api/query\"\n",
        fixture.url
    ));
    reserve_arxiv_window(&environment);

    let output = environment.run(&["doctor", "--provider", "arxiv_api", "--timeout", "1"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            fixture.finish_all().len()
        ),
        (Some(4), &Value::String("timeout".into()), 0)
    );
}

#[test]
fn shallow_doctor_reachability_probe_waits_for_the_request_window() {
    let fixture = Fixture::start_sequence(reachable_responses(9));
    let environment = RunEnvironment::new(&shallow_config(&fixture.url));
    reserve_arxiv_window(&environment);

    let output = environment.run(&["doctor", "--timeout", "1"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");
    let arxiv_api = payload["providers"]
        .as_array()
        .expect("providers")
        .iter()
        .find(|provider| provider["provider"] == "arxiv_api")
        .expect("arxiv_api status");

    assert_eq!(
        (
            output.status.code(),
            &arxiv_api["configured"],
            &arxiv_api["reachable"],
            fixture.finish_all().len()
        ),
        (Some(4), &Value::Bool(true), &Value::Bool(false), 9)
    );
}

fn ssrn_browser_status(payload: &Value) -> &Value {
    payload["providers"]
        .as_array()
        .expect("providers")
        .iter()
        .find(|provider| provider["provider"] == "ssrn_browser")
        .expect("ssrn_browser status")
}

/// Runs a shallow doctor where every HTTP provider is reachable and `fake` answers for
/// `ssrn_browser`, which the SSRN order lists when `enabled`.
fn shallow_with_browser(fake: &FakeOpenCli, enabled: bool) -> std::process::Output {
    let fixture = Fixture::start_sequence(reachable_responses(10));
    let order = if enabled {
        "[\"ssrn_crossref\", \"ssrn_browser\"]"
    } else {
        "[\"ssrn_crossref\"]"
    };
    let environment = RunEnvironment::new(&format!(
        "{}\n{}",
        shallow_config(&fixture.url),
        fake.config(order)
    ));
    let output = environment.run(&["doctor"]);
    assert_eq!(fixture.finish_all().len(), 10);
    output
}

#[test]
fn shallow_doctor_checks_an_enabled_process_route_by_its_contract_command() {
    let fake = FakeOpenCli::envelope("ok", &serde_json::json!({}));

    let output = shallow_with_browser(&fake, true);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");
    let calls = fake.calls();

    assert_eq!(
        (
            output.status.code(),
            &payload["ok"],
            ssrn_browser_status(&payload)["configured"].clone(),
            ssrn_browser_status(&payload)["reachable"].clone(),
            calls.len(),
            calls[0][..2].to_vec(),
        ),
        (
            Some(0),
            &Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            1,
            vec!["ssrn".to_owned(), "contract".to_owned()],
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn shallow_doctor_reports_an_outdated_adapter_with_install_steps() {
    let fake = FakeOpenCli::answering(
        r#"{"contract":"forager-ssrn/0","status":"ok","data":{}}"#,
        "",
        0,
    );

    let output = shallow_with_browser(&fake, true);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");
    let status = ssrn_browser_status(&payload);

    assert_eq!(
        (
            output.status.code(),
            &payload["ok"],
            &status["reachable"],
            status["message"]
                .as_str()
                .is_some_and(|message| message.contains("install or update")),
        ),
        (Some(4), &Value::Bool(false), &Value::Bool(false), true)
    );
}

#[test]
fn shallow_doctor_skips_a_process_route_that_no_order_enables() {
    let fake = FakeOpenCli::answering("", "", 1);

    let output = shallow_with_browser(&fake, false);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");

    assert_eq!(
        (
            output.status.code(),
            &payload["ok"],
            ssrn_browser_status(&payload)["configured"].clone(),
            fake.calls().len(),
        ),
        (Some(0), &Value::Bool(true), Value::Bool(false), 0)
    );
}

#[test]
fn ssrn_browser_deep_doctor_runs_one_platform_search() {
    let fake = FakeOpenCli::envelope(
        "ok",
        &serde_json::json!({
            "url": "https://papers.ssrn.com/searchresults.cfm?term=forager+doctor",
            "term": "forager doctor",
            "search_state": {"scope":"title-abstract-keywords", "mode":"fuzzy", "author":"", "date":"All Time", "sort":"Relevancy", "request_url":"https://api.ssrn.com/papers/v1/papers/search/advanced?text=forager+doctor&text_fields=title-abstract-keywords&search_mode=fuzzy&authors=&date=all_time&sort_by=&page=1"},
            "current_page": "1",
            "range": "Displaying results 1 to 1 of 1",
            "results": [{
                "url": "https://papers.ssrn.com/sol3/papers.cfm?abstract_id=1",
                "title": "Paper"
            }]
        }),
    );
    let environment = RunEnvironment::new(&fake.config("[\"ssrn_browser\"]"));

    let output = environment.run(&["doctor", "--provider", "ssrn_browser"]);

    assert_deep_success(&output, "ssrn_browser", &[("search", "process")]);
    assert_eq!(fake.calls()[0][..2], ["ssrn", "search"]);
}

#[test]
fn ssrn_browser_deep_doctor_refuses_a_route_that_no_order_enables() {
    let fake = FakeOpenCli::envelope("ok", &serde_json::json!({}));
    let environment = RunEnvironment::new(&fake.config("[\"ssrn_crossref\"]"));

    let output = environment.run(&["doctor", "--provider", "ssrn_browser"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            fake.calls().len()
        ),
        (Some(3), &Value::String("config".into()), 0)
    );
}
