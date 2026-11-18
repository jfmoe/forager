mod support;

use serde_json::Value;

use std::time::Duration;

use support::doctor::{assert_deep_success, reachable_responses, shallow_config};
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

#[test]
fn polymarket_api_deep_doctor_checks_the_clob_and_the_data_api_after_its_search() {
    let gamma = Fixture::start_json(200, r#"{"pagination":{"hasMore":false,"totalResults":0}}"#);
    let clob = Fixture::start_json(200, "1792800000");
    let data = Fixture::start_json(
        200,
        r#"{"data":{"computed_at":"2026-10-24T07:35:04Z","age_seconds":4}}"#,
    );
    let environment = RunEnvironment::new(&format!(
        "[providers.polymarket_api]\nurl = {:?}\nclob_url = {:?}\ndata_url = {:?}\n",
        gamma.url, clob.url, data.url
    ));

    let output = environment.run(&["doctor", "--provider", "polymarket_api"]);
    assert_deep_success(
        &output,
        "polymarket_api",
        &[("search", "http"), ("clob", "http"), ("data", "http")],
    );
    let paths =
        [gamma.finish(), clob.finish(), data.finish()].map(|request| support::hn::path(&request));

    assert_eq!(paths, ["/public-search", "/time", "/v2/status"]);
}

/// The browser route against a shell stand-in for the browser, which speaks no CDP.
#[cfg(unix)]
mod xiaohongshu_browser {
    use serde_json::{Value, json};

    use super::route_status;
    use crate::support::browser::{StandIn, mark_logged_in};
    use crate::support::doctor::{reachable_responses, shallow_config};
    use crate::support::{Fixture, RunEnvironment};

    fn shallow_with_xiaohongshu(order: &str, logged_in: bool) -> (Option<i32>, Value, usize) {
        let fixture = Fixture::start_sequence(reachable_responses(10));
        let stand_in = StandIn::failing("must not start");
        let environment = RunEnvironment::new(&format!(
            "{}\n{}",
            shallow_config(&fixture.url),
            stand_in.xiaohongshu_config(order)
        ));
        if logged_in {
            mark_logged_in(&environment.state_dir, "xiaohongshu");
        }

        let output = environment.run(&["doctor"]);
        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");
        assert_eq!(fixture.finish_all().len(), 10);
        (
            output.status.code(),
            route_status(&payload, "xiaohongshu_browser").clone(),
            stand_in.launches().len(),
        )
    }

    #[test]
    fn shallow_doctor_checks_the_xiaohongshu_route_with_its_preflight_alone() {
        let enabled = "[\"xiaohongshu_browser\"]";

        let [ready, not_logged_in, disabled] = [
            shallow_with_xiaohongshu(enabled, true),
            shallow_with_xiaohongshu(enabled, false),
            shallow_with_xiaohongshu("[]", true),
        ]
        .map(|(code, status, launches)| {
            (
                code,
                status["configured"].clone(),
                status["reachable"].clone(),
                status["message"].clone(),
                launches,
            )
        });

        assert_eq!(
            [ready, not_logged_in, disabled],
            [
                (Some(0), json!(true), json!(true), Value::Null, 0),
                (
                    Some(4),
                    json!(true),
                    json!(false),
                    json!(
                        "the xiaohongshu Browser Profile has never been logged in; run `forager browser login xiaohongshu`"
                    ),
                    0
                ),
                (Some(0), json!(false), json!(false), Value::Null, 0),
            ]
        );
    }

    #[test]
    fn xiaohongshu_browser_deep_doctor_runs_one_browser_search() {
        let stand_in = StandIn::failing("no CDP here");
        let environment =
            RunEnvironment::new(&stand_in.xiaohongshu_config("[\"xiaohongshu_browser\"]"));
        mark_logged_in(&environment.state_dir, "xiaohongshu");

        let output = environment.run(&["doctor", "--provider", "xiaohongshu_browser"]);
        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");
        let launches = stand_in.launches();

        assert_eq!(
            (
                output.status.code(),
                payload["checks"].as_array().map(|checks| checks
                    .iter()
                    .map(|check| (check["name"].clone(), check["transport"].clone()))
                    .collect::<Vec<_>>()),
                launches.len(),
                launches.concat().contains(&"--headless=new".to_owned()),
            ),
            (
                Some(4),
                Some(vec![(json!("search"), json!("browser"))]),
                1,
                true
            ),
            "{payload}"
        );
    }

    #[test]
    fn xiaohongshu_browser_deep_doctor_refuses_a_route_that_cannot_run() {
        let stand_in = StandIn::failing("must not start");
        let disabled = RunEnvironment::new(&stand_in.xiaohongshu_config("[]"));
        let not_logged_in =
            RunEnvironment::new(&stand_in.xiaohongshu_config("[\"xiaohongshu_browser\"]"));

        let outcomes = [disabled, not_logged_in].map(|environment| {
            let output = environment.run(&["doctor", "--provider", "xiaohongshu_browser"]);
            let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");
            (
                output.status.code(),
                payload["error_kind"].clone(),
                payload["message"].clone(),
            )
        });

        assert_eq!(
            (outcomes, stand_in.launches().len()),
            (
                [
                    (
                        Some(3),
                        json!("config"),
                        json!(
                            "no platform order lists `xiaohongshu_browser`; add it to the order to enable it"
                        )
                    ),
                    (
                        Some(3),
                        json!("config"),
                        json!(
                            "the xiaohongshu Browser Profile has never been logged in; run `forager browser login xiaohongshu`"
                        )
                    ),
                ],
                0
            )
        );
    }
}
