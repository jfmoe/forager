mod support;

use serde_json::Value;

use std::time::Duration;

use support::doctor::{assert_deep_success, reachable_responses, shallow_config};
use support::opencli::FakeOpenCli;
use support::{Fixture, RunEnvironment, youtube_web};

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
fn shallow_doctor_reaches_serpapi_with_a_keyless_get() {
    let fixture = Fixture::start_sequence(reachable_responses(4));
    let environment = RunEnvironment::new(&format!(
        "[providers.arxiv_api]\nurl = {url:?}\n[providers.ssrn_crossref]\nurl = {url:?}\n\
         [providers.hn_api]\nurl = {url:?}\n\
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

fn route_status<'a>(payload: &'a Value, route: &str) -> &'a Value {
    payload["providers"]
        .as_array()
        .expect("providers")
        .iter()
        .find(|provider| provider["provider"] == route)
        .unwrap_or_else(|| panic!("{route} status"))
}

fn ssrn_browser_status(payload: &Value) -> &Value {
    route_status(payload, "ssrn_browser")
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
    let fake = FakeOpenCli::contract_envelope("forager-ssrn/0", "ok", &Value::Null);

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
            "request_url": "https://api.ssrn.com/papers/v1/papers/search/advanced?text=forager+doctor&text_fields=title-abstract-keywords&search_mode=fuzzy&authors=&date=all_time&sort_by=&page=1",
            "http_status": 200,
            "content_type": "application/json",
            "body": {"total": 1, "papers": [{"id": 1, "title": "Paper"}]}
        }),
    );
    let environment = RunEnvironment::new(&fake.config("[\"ssrn_browser\"]"));

    let output = environment.run(&["doctor", "--provider", "ssrn_browser"]);

    assert_deep_success(&output, "ssrn_browser", &[("search", "process")]);
    assert_eq!(fake.calls()[0][..2], ["ssrn", "search"]);
}

#[test]
fn youtube_api_deep_doctor_runs_one_search_with_the_key_in_a_header() {
    let fixture = Fixture::start_sequence(vec![support::youtube::search_page(&[], None)]);
    let environment = RunEnvironment::new(&support::youtube::config(&fixture.url));

    let output = environment.run(&["doctor", "--provider", "youtube_api"]);
    assert_deep_success(&output, "youtube_api", &[("search", "http")]);
    let request = fixture.finish();

    assert_eq!(
        (
            support::youtube::path(&request),
            support::youtube::header(&request, "x-goog-api-key").as_deref(),
            request
                .lines()
                .next()
                .unwrap_or_default()
                .contains(support::youtube::KEY),
            String::from_utf8_lossy(&output.stdout).contains(support::youtube::KEY),
        ),
        (
            "/youtube/v3/search".to_owned(),
            Some(support::youtube::KEY),
            false,
            false
        )
    );
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

#[test]
fn xiaohongshu_browser_deep_doctor_runs_one_first_page_search() {
    let fake = FakeOpenCli::contract_envelope(
        support::opencli::XHS_CONTRACT,
        "ok",
        &serde_json::json!({
            "page": {
                "url": "https://www.xiaohongshu.com/search_result?keyword=forager%20doctor&source=web_explore_feed",
                "title": "forager doctor - 小红书搜索",
                "guest": false
            },
            "filter_clicks": 0,
            "responses": [{
                "click": 0,
                "request": {"keyword": "forager doctor", "page": 1, "search_id": "2fhpxs5cks5vx6nvw1ar4"},
                "body": {"code": 0, "success": true, "data": {"has_more": true, "items": [{
                    "id": "66f0a1b2c3d4e5f607100001",
                    "model_type": "note",
                    "xsec_token": "ABtoken1=",
                    "note_card": {"type": "normal", "display_title": "Note"}
                }]}}
            }]
        }),
    );
    let environment = RunEnvironment::new(&fake.route_config(
        "xiaohongshu_browser",
        "xiaohongshu",
        "[\"xiaohongshu_browser\"]",
    ));

    let output = environment.run(&["doctor", "--provider", "xiaohongshu_browser"]);

    assert_deep_success(&output, "xiaohongshu_browser", &[("search", "process")]);
    let calls = fake.calls();
    assert_eq!(
        calls[0][..12],
        [
            "forager-xhs",
            "search",
            "--query",
            "forager doctor",
            "--sort",
            "comprehensive",
            "--note-type",
            "all",
            "--publish-time",
            "any",
            "--pages",
            "1"
        ]
    );
}

#[test]
fn shallow_doctor_checks_the_enabled_xiaohongshu_adapter_contract() {
    let fixture = Fixture::start_sequence(reachable_responses(10));
    let fake = FakeOpenCli::contract_envelope(
        support::opencli::XHS_CONTRACT,
        "ok",
        &serde_json::json!({"commands": ["contract", "search"]}),
    );
    let environment = RunEnvironment::new(&format!(
        "{}\n{}",
        shallow_config(&fixture.url),
        fake.route_config(
            "xiaohongshu_browser",
            "xiaohongshu",
            "[\"xiaohongshu_browser\"]"
        )
    ));

    let output = environment.run(&["doctor"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");
    let status = route_status(&payload, "xiaohongshu_browser").clone();

    assert_eq!(
        (
            output.status.code(),
            &status["configured"],
            &status["reachable"],
            fake.calls()
                .iter()
                .map(|call| call[..2].to_vec())
                .collect::<Vec<_>>(),
        ),
        (
            Some(0),
            &Value::Bool(true),
            &Value::Bool(true),
            vec![vec!["forager-xhs".to_owned(), "contract".to_owned()]],
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fixture.finish_all().len(), 10);
}

#[test]
fn shallow_doctor_skips_the_xiaohongshu_route_while_its_order_is_empty() {
    let fixture = Fixture::start_sequence(reachable_responses(10));
    let fake = FakeOpenCli::answering("", "", 1);
    let environment = RunEnvironment::new(&format!(
        "{}\n{}",
        shallow_config(&fixture.url),
        fake.route_config("xiaohongshu_browser", "xiaohongshu", "[]")
    ));

    let output = environment.run(&["doctor"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");

    assert_eq!(
        (
            output.status.code(),
            &payload["ok"],
            route_status(&payload, "xiaohongshu_browser")["configured"].clone(),
            fake.calls().len(),
        ),
        (Some(0), &Value::Bool(true), Value::Bool(false), 0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fixture.finish_all().len(), 10);
}

#[test]
fn xiaohongshu_browser_deep_doctor_refuses_the_route_while_its_order_is_empty() {
    let fake = FakeOpenCli::answering("", "", 1);
    let environment =
        RunEnvironment::new(&fake.route_config("xiaohongshu_browser", "xiaohongshu", "[]"));

    let output = environment.run(&["doctor", "--provider", "xiaohongshu_browser"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            &payload["message"],
            fake.calls().len()
        ),
        (
            Some(3),
            &Value::String("config".into()),
            &Value::String(
                "no platform order lists `xiaohongshu_browser`; add it to the order to enable it"
                    .into()
            ),
            0
        ),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Runs a shallow doctor where every HTTP provider is reachable, `youtube_web` among them when
/// the YouTube order lists it.
fn shallow_with_youtube_web(enabled: bool) -> (Value, usize) {
    let requests = if enabled { 11 } else { 10 };
    let fixture = Fixture::start_sequence(reachable_responses(requests));
    let order = if enabled {
        "[\"youtube_api\", \"youtube_web\"]"
    } else {
        "[\"youtube_api\"]"
    };
    let environment = RunEnvironment::new(&format!(
        "{}\n[providers.youtube_web]\nurl = {:?}\n\n[platforms.youtube]\norder = {order}\n",
        shallow_config(&fixture.url),
        fixture.url
    ));
    let output = environment.run(&["doctor"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");
    (
        route_status(&payload, "youtube_web").clone(),
        fixture.finish_all().len(),
    )
}

#[test]
fn shallow_doctor_reaches_youtube_web_only_while_an_order_enables_it() {
    let (disabled, disabled_requests) = shallow_with_youtube_web(false);
    let (enabled, enabled_requests) = shallow_with_youtube_web(true);

    assert_eq!(
        (
            &disabled["configured"],
            &disabled["reachable"],
            disabled_requests,
            &enabled["configured"],
            &enabled["reachable"],
            enabled_requests,
        ),
        (
            &Value::Bool(false),
            &Value::Bool(false),
            10,
            &Value::Bool(true),
            &Value::Bool(true),
            11,
        )
    );
}

#[test]
fn youtube_web_deep_doctor_refuses_the_route_while_no_order_enables_it() {
    let fixture = Fixture::start_canary();
    let environment = RunEnvironment::new(&format!(
        "[providers.youtube_web]\nurl = {:?}\n",
        fixture.url
    ));

    let output = environment.run(&["doctor", "--provider", "youtube_web"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            &payload["message"],
            fixture.finish_all().len()
        ),
        (
            Some(3),
            &Value::String("config".into()),
            &Value::String(
                "no platform order lists `youtube_web`; add it to the order to enable it".into()
            ),
            0
        )
    );
}

#[test]
fn youtube_web_deep_doctor_fetches_the_canary_video_metadata() {
    let fixture = Fixture::start_sequence_for(|origin| {
        vec![
            youtube_web::watch_page(&youtube_web::zoo_embedded(youtube_web::ZOO_DESCRIPTION)),
            youtube_web::json(&youtube_web::zoo_player(
                &[youtube_web::track(origin, "en", false)],
                0,
            )),
        ]
    });
    let environment = RunEnvironment::new(&youtube_web::config(&fixture.url));

    let output = environment.run(&["doctor", "--provider", "youtube_web"]);
    assert_deep_success(&output, "youtube_web", &[("fetch", "http")]);
    let requests = fixture.finish_all();

    assert_eq!(
        requests
            .iter()
            .map(|request| support::youtube::path(request))
            .collect::<Vec<_>>(),
        ["/watch", "/youtubei/v1/player"]
    );
}
