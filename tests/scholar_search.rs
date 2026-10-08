mod support;

use std::collections::BTreeMap;
use std::process::Output;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Value, json};

use support::{Fixture, Response, RunEnvironment};

const KEY: &str = "test-serpapi-key-a";
const SECOND_KEY: &str = "test-serpapi-key-b";

/// Reads a trimmed real SerpApi response captured on 2026-10-07.
fn fixture(name: &str) -> Value {
    let path = format!(
        "{}/tests/fixtures/scholar/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).expect("read fixture")).expect("fixture")
}

fn ok(body: &Value) -> Response {
    Response::json(200, &body.to_string())
}

fn config(url: &str, keys: &[&str]) -> String {
    let keys = keys
        .iter()
        .map(|key| format!("\"{key}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "[providers.serpapi]\nurl = \"{url}/search.json\"\nkeys = [{keys}]\n[retry]\nmax_wait = 0\n"
    )
}

fn search(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "scholar", "search"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

/// Runs one search against a fixture that answers in order and returns every request.
fn run(keys: &[&str], arguments: &[&str], responses: Vec<Response>) -> (Output, Vec<String>) {
    let fixture = Fixture::start_sequence(responses);
    let environment = RunEnvironment::new(&config(&fixture.url, keys));
    let output = search(&environment, arguments);
    (output, fixture.finish_all())
}

fn payload(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "parse JSON stdout: {error}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn request_target(request: &str) -> reqwest::Url {
    let target = request.split_whitespace().nth(1).expect("request target");
    reqwest::Url::parse(&format!("http://fixture.test{target}")).expect("request URL")
}

fn query_pairs(request: &str) -> BTreeMap<String, String> {
    request_target(request)
        .query_pairs()
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}

fn refs(page: &Value) -> Vec<String> {
    page["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|item| item["ref"].as_str().expect("ref").to_owned())
        .collect()
}

/// A cursor as the route issues it, for a page the caller names.
fn cursor(limit: u64, page: &str, options: Value) -> String {
    let mut options = options;
    options["platform"] = json!("scholar");
    let request = json!({"query": "momentum", "limit": limit, "options": options, "page": page});
    format!("v1.serpapi.{}", URL_SAFE_NO_PAD.encode(request.to_string()))
}

/// A search page that holds only the given results of the search fixture, by index.
fn page_of(results: Vec<Value>) -> Value {
    let mut page = fixture("search.json");
    page["organic_results"] = Value::Array(results);
    page
}

fn result(index: usize) -> Value {
    fixture("search.json")["organic_results"][index].clone()
}

#[test]
fn search_sends_the_engine_language_query_and_page_size_with_the_key() {
    let (output, requests) = run(
        &[KEY],
        &[
            "\"time series momentum\" author:\"Pedersen\"",
            "--limit",
            "5",
        ],
        vec![ok(&fixture("search.json"))],
    );

    assert_eq!(
        (
            output.status.code(),
            request_target(&requests[0]).path().to_owned(),
            query_pairs(&requests[0]),
        ),
        (
            Some(0),
            "/search.json".to_owned(),
            BTreeMap::from([
                ("engine".to_owned(), "google_scholar".to_owned()),
                ("hl".to_owned(), "en".to_owned()),
                (
                    "q".to_owned(),
                    "\"time series momentum\" author:\"Pedersen\"".to_owned()
                ),
                ("num".to_owned(), "5".to_owned()),
                ("api_key".to_owned(), KEY.to_owned()),
            ])
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn year_bounds_and_review_only_join_the_query_parameters() {
    let (output, requests) = run(
        &[KEY],
        &[
            "momentum",
            "--year-from",
            "2020",
            "--year-to",
            "2024",
            "--review-only",
        ],
        vec![ok(&fixture("search.json"))],
    );
    let query = query_pairs(&requests[0]);

    assert_eq!(
        (
            output.status.code(),
            ["as_ylo", "as_yhi", "as_rr"].map(|name| query.get(name).cloned())
        ),
        (
            Some(0),
            [Some("2020"), Some("2024"), Some("1")].map(|value| value.map(str::to_owned))
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_search_without_years_or_review_only_sends_no_filter_parameters() {
    let (output, requests) = run(&[KEY], &["momentum"], vec![ok(&fixture("search.json"))]);
    let query = query_pairs(&requests[0]);

    assert_eq!(
        (
            output.status.code(),
            ["as_ylo", "as_yhi", "as_rr", "start"].map(|name| query.contains_key(name))
        ),
        (Some(0), [false; 4])
    );
}

#[test]
fn the_page_size_defaults_to_twenty() {
    let (output, requests) = run(&[KEY], &["momentum"], vec![ok(&fixture("search.json"))]);

    assert_eq!(
        (
            output.status.code(),
            query_pairs(&requests[0])["num"].as_str()
        ),
        (Some(0), "20")
    );
}

#[test]
fn a_page_size_outside_one_to_twenty_or_a_blank_query_is_rejected_before_any_request() {
    for arguments in [
        &["momentum", "--limit", "0"][..],
        &["momentum", "--limit", "21"],
        &["  "],
    ] {
        let fixture = Fixture::start_canary();
        let environment = RunEnvironment::new(&config(&fixture.url, &[KEY]));

        let output = search(&environment, arguments);

        assert_eq!(
            (output.status.code(), fixture.finish_all().len()),
            (Some(2), 0),
            "arguments: {arguments:?}\nstderr: {}",
            stderr(&output)
        );
    }
}

#[test]
fn a_reversed_or_out_of_range_year_is_rejected_before_any_request() {
    for (arguments, message) in [
        (
            &["momentum", "--year-from", "2025", "--year-to", "2024"][..],
            "--year-from must not be later than --year-to",
        ),
        (&["momentum", "--year-from", "999"], "--year-from"),
        (&["momentum", "--year-to", "10000"], "--year-to"),
    ] {
        let fixture = Fixture::start_canary();
        let environment = RunEnvironment::new(&config(&fixture.url, &[KEY]));

        let output = search(&environment, arguments);

        assert_eq!(
            (
                output.status.code(),
                stderr(&output).contains(message),
                fixture.finish_all().len()
            ),
            (Some(2), true, 0),
            "arguments: {arguments:?}\nstderr: {}",
            stderr(&output)
        );
    }
}

#[test]
fn a_cursor_resumes_the_next_page_with_every_original_condition() {
    let fixture = Fixture::start_sequence(vec![
        ok(&fixture("search.json")),
        ok(&fixture("search.json")),
    ]);
    let environment = RunEnvironment::new(&config(&fixture.url, &[KEY]));
    let first = search(
        &environment,
        &[
            "\"time series momentum\" author:\"Pedersen\"",
            "--limit",
            "10",
            "--year-from",
            "2020",
            "--year-to",
            "2024",
            "--review-only",
        ],
    );
    let cursor = payload(&first)["next_cursor"]
        .as_str()
        .expect("first page cursor")
        .to_owned();
    let second = search(&environment, &["--cursor", &cursor]);
    let requests = fixture.finish_all();
    let mut resumed = query_pairs(&requests[1]);
    let start = resumed.remove("start");

    assert_eq!(
        (
            second.status.code(),
            cursor.starts_with("v1.serpapi."),
            start.as_deref(),
            resumed,
        ),
        (Some(0), true, Some("10"), query_pairs(&requests[0])),
        "stderr: {}",
        stderr(&second)
    );
}

#[test]
fn the_last_page_google_scholar_offers_has_no_next_cursor() {
    let (output, requests) = run(
        &[KEY],
        &["--cursor", &cursor(20, "980", json!({}))],
        vec![ok(&fixture("deep980.json"))],
    );
    let page = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            query_pairs(&requests[0])["start"].as_str(),
            page["items"].as_array().map(Vec::len),
            &page["next_cursor"],
        ),
        (Some(0), "980", Some(2), &Value::Null),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn no_cursor_is_issued_for_a_next_page_that_would_pass_result_1000() {
    let next_cursor = |start: &str| {
        let (output, _) = run(
            &[KEY],
            &["--cursor", &cursor(7, start, json!({}))],
            vec![ok(&fixture("search.json"))],
        );
        payload(&output)["next_cursor"].clone()
    };

    assert_eq!(
        [next_cursor("980").is_string(), next_cursor("987").is_null()],
        [true, true]
    );
}

#[test]
fn twenty_result_pages_reach_result_1000_exactly() {
    let (output, _) = run(
        &[KEY],
        &["--cursor", &cursor(20, "960", json!({}))],
        vec![ok(&fixture("search.json"))],
    );
    let next = payload(&output)["next_cursor"]
        .as_str()
        .expect("cursor for the page ending at 1000")
        .to_owned();
    let (output, requests) = run(
        &[KEY],
        &["--cursor", &next],
        vec![ok(&fixture("search.json"))],
    );

    assert_eq!(
        (
            query_pairs(&requests[0])["start"].as_str(),
            &payload(&output)["next_cursor"]
        ),
        ("980", &Value::Null)
    );
}

/// Runs a search that must fail before any request and returns its exit code and stderr.
fn preflight(configuration: &str, arguments: &[&str]) -> (Option<i32>, String, usize) {
    let fixture = Fixture::start_canary();
    let environment = RunEnvironment::new(&configuration.replace("{url}", &fixture.url));
    let output = search(&environment, arguments);
    (
        output.status.code(),
        stderr(&output),
        fixture.finish_all().len(),
    )
}

#[test]
fn a_cursor_conflicts_with_an_explicit_query_option_or_limit() {
    let cursor = cursor(20, "20", json!({}));
    for extra in [
        &["momentum"][..],
        &["--limit", "20"],
        &["--year-from", "2020"],
        &["--year-to", "2024"],
        &["--review-only"],
    ] {
        let mut arguments = vec!["--cursor", cursor.as_str()];
        arguments.extend_from_slice(extra);

        let (code, message, requests) = preflight(&config("{url}", &[KEY]), &arguments);

        assert_eq!(
            (code, message.contains("cannot be used with"), requests),
            (Some(2), true, 0),
            "arguments: {arguments:?}\nstderr: {message}"
        );
    }
}

#[test]
fn a_tampered_cursor_is_rejected_before_any_request() {
    for (cursor, expected) in [
        (
            cursor(0, "20", json!({})),
            "--limit must be between 1 and 20",
        ),
        (
            cursor(21, "20", json!({})),
            "--limit must be between 1 and 20",
        ),
        (
            cursor(20, "20", json!({"year_from": 999})),
            "--year-from must be between 1000 and 9999",
        ),
        (
            cursor(20, "20", json!({"year_from": 2025, "year_to": 2024})),
            "--year-from must not be later than --year-to",
        ),
        (
            cursor(20, "1000", json!({})),
            "serpapi cannot page past Google Scholar result 1000",
        ),
        (
            cursor(20, "-20", json!({})),
            "invalid Google Scholar page position `-20`",
        ),
    ] {
        let (code, message, requests) = preflight(&config("{url}", &[KEY]), &["--cursor", &cursor]);

        assert_eq!(
            (code, message.contains(expected), requests),
            (Some(2), true, 0),
            "expected: {expected}\nstderr: {message}"
        );
    }
}

#[test]
fn a_cursor_whose_route_left_the_order_or_lost_its_keys_is_rejected() {
    let cursor = cursor(20, "20", json!({}));
    for configuration in [
        format!(
            "{}[platforms.scholar]\norder = []\n",
            config("{url}", &[KEY])
        ),
        config("{url}", &[]),
    ] {
        let (code, message, requests) = preflight(&configuration, &["--cursor", &cursor]);

        assert_eq!(
            (code, message.contains("no longer available"), requests),
            (Some(2), true, 0),
            "config: {configuration}\nstderr: {message}"
        );
    }
}

#[test]
fn search_decodes_results_with_cluster_refs_and_metadata() {
    let source = fixture("search.json");
    let snippet = |index: usize| source["organic_results"][index]["snippet"].clone();
    let (output, _) = run(&[KEY], &["momentum"], vec![ok(&source)]);
    let mut page = payload(&output);
    let items = page["items"].as_array_mut().expect("items").split_off(4);
    let next_cursor = page
        .as_object_mut()
        .expect("page")
        .remove("next_cursor")
        .expect("next_cursor");

    assert_eq!(
        (page, items.len(), next_cursor.is_string()),
        (
            json!({
                "platform": "scholar",
                "provider": "serpapi",
                "items": [
                    {
                        "ref": "scholar:18208131694456651388",
                        "url": "https://scholar.google.com/scholar?cluster=18208131694456651388",
                        "depth": "snippet",
                        "title": "Time series momentum",
                        "authors": ["TJ Moskowitz", "YH Ooi", "LH Pedersen"],
                        "published": "2012",
                        "snippet": snippet(0),
                        "link": "https://www.sciencedirect.com/science/article/pii/S0304405X11002613",
                        "source": "TJ Moskowitz, YH Ooi, LH Pedersen - Journal of financial economics, 2012 - Elsevier",
                        "cited_by": 2307,
                        "version_count": 45,
                        "resources": [{
                            "title": "sciencedirect.com",
                            "file_format": "HTML",
                            "url": "https://www.sciencedirect.com/science/article/pii/S0304405X11002613"
                        }],
                        "result_type": "Html"
                    },
                    {
                        "ref": "scholar:10743434366743644145",
                        "url": "https://scholar.google.com/scholar?cluster=10743434366743644145",
                        "depth": "snippet",
                        "title": "Enhancing time series momentum strategies using deep neural networks",
                        "authors": ["B Lim", "S Zohren", "S Roberts"],
                        "published": "2019",
                        "snippet": snippet(1),
                        "link": "https://arxiv.org/abs/1904.04912",
                        "source": "B Lim, S Zohren, S Roberts - arXiv preprint arXiv:1904.04912, 2019 - arxiv.org",
                        "cited_by": 151,
                        "version_count": 18,
                        "resources": [{
                            "title": "arxiv.org",
                            "file_format": "PDF",
                            "url": "https://arxiv.org/pdf/1904.04912"
                        }],
                        "result_type": null
                    },
                    {
                        // No explicit ID: the ref comes from `result_id`, and SerpApi answered a
                        // cluster lookup for this ID with this paper.
                        "ref": "scholar:2175834747627124651",
                        "url": "https://scholar.google.com/scholar?cluster=2175834747627124651",
                        "depth": "snippet",
                        "title": "When Does Risk-Managed Momentum Add Value? An Out-of-Sample, Regime-Conditional, and Statistically Tested Study of a Volatility-Targeted Dual-Momentum …",
                        "authors": ["A Goyal"],
                        "published": "2026",
                        "snippet": snippet(2),
                        "link": "https://papers.ssrn.com/sol3/papers.cfm?abstract_id=7143882",
                        "source": "A Goyal - … Volatility-Targeted Dual-Momentum Strategy (June 15 …, 2026 - papers.ssrn.com",
                        "cited_by": null,
                        "version_count": null,
                        "resources": [{
                            "title": "ssrn.com",
                            "file_format": "PDF",
                            "url": "https://papers.ssrn.com/sol3/Delivery.cfm?abstractid=7143882"
                        }],
                        "result_type": null
                    },
                    {
                        "ref": "scholar:15167737520660287132",
                        "url": "https://scholar.google.com/scholar?cluster=15167737520660287132",
                        "depth": "snippet",
                        "title": "Resilience Anomaly and Asset Pricing in the Chinese Stock Market",
                        "authors": ["J Hu", "X Li", "T Wang"],
                        "published": null,
                        "snippet": snippet(3),
                        "link": "https://papers.ssrn.com/sol3/papers.cfm?abstract_id=7506942",
                        "source": "J Hu, X Li, T Wang - papers.ssrn.com",
                        "cited_by": null,
                        "version_count": null,
                        "resources": [{
                            "title": "ssrn.com",
                            "file_format": "PDF",
                            "url": "https://papers.ssrn.com/sol3/Delivery.cfm?abstractid=7506942"
                        }],
                        "result_type": null
                    }
                ]
            }),
            4,
            true
        )
    );
}

#[test]
fn a_result_without_a_verifiable_identity_is_skipped_with_a_diagnostic() {
    let mut unidentified = result(3);
    unidentified["result_id"] = Value::Null;
    let (output, _) = run(
        &[KEY],
        &["momentum"],
        vec![ok(&page_of(vec![result(1), unidentified]))],
    );

    assert_eq!(
        (
            output.status.code(),
            refs(&payload(&output)),
            stderr(&output).contains("Resilience Anomaly and Asset Pricing"),
        ),
        (
            Some(0),
            vec!["scholar:10743434366743644145".to_owned()],
            true
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_page_whose_every_result_is_skipped_is_a_runtime_failure() {
    let mut unidentified = result(3);
    unidentified["result_id"] = Value::Null;
    let (output, requests) = run(
        &[KEY],
        &["momentum"],
        vec![ok(&page_of(vec![unidentified]))],
    );

    assert_eq!(
        (
            output.status.code(),
            &payload(&output)["error_kind"],
            requests.len()
        ),
        (Some(4), &json!("runtime"), 1),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_fully_empty_result_is_an_empty_page() {
    let (output, requests) = run(
        &[KEY],
        &["\"zxqvbn qqqzzz momentum kkkwww\""],
        vec![ok(&fixture("empty.json"))],
    );
    let page = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &page["items"],
            &page["next_cursor"],
            requests.len()
        ),
        (Some(0), &json!([]), &Value::Null, 1)
    );
}

#[test]
fn a_success_without_results_is_a_runtime_failure() {
    let mut empty_array = fixture("search.json");
    empty_array["organic_results"] = json!([]);
    let (output, requests) = run(&[KEY], &["momentum"], vec![ok(&empty_array)]);

    assert_eq!(
        (
            output.status.code(),
            &payload(&output)["error_kind"],
            requests.len()
        ),
        (Some(4), &json!("runtime"), 1),
        "stderr: {}",
        stderr(&output)
    );
}

fn status_error() -> Value {
    json!({
        "search_metadata": {"id": "6ac6613da20802ffe5e7c8be", "status": "Error"},
        "error": "Google hasn't returned any results for this query."
    })
}

#[test]
fn a_status_error_is_a_network_failure_that_the_shared_policy_retries() {
    let (output, requests) = run(
        &[KEY],
        &["momentum", "--verbose"],
        vec![ok(&status_error()), ok(&fixture("search.json"))],
    );
    let page = payload(&output);
    let attempts = page["provider_attempts"]
        .as_array()
        .expect("attempts")
        .iter()
        .map(|attempt| {
            (
                attempt["error_kind"].clone(),
                attempt["http_status"].clone(),
                attempt["retry_count"].clone(),
                attempt["message"].clone(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        (output.status.code(), requests.len(), attempts),
        (
            Some(0),
            2,
            vec![
                (
                    json!("network"),
                    json!(200),
                    json!(0),
                    json!(
                        "SerpApi reported an error: Google hasn't returned any results for this query."
                    )
                ),
                (Value::Null, json!(200), json!(1), json!("")),
            ]
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn an_exhausted_key_rotates_to_the_next_key_in_the_same_request() {
    let (output, requests) = run(
        &[KEY, SECOND_KEY],
        &["momentum", "--verbose"],
        vec![
            Response::json(429, r#"{"error": "Your account has run out of searches."}"#),
            ok(&fixture("search.json")),
        ],
    );
    let page = payload(&output);
    let attempts = page["provider_attempts"]
        .as_array()
        .expect("attempts")
        .iter()
        .map(|attempt| {
            (
                attempt["error_kind"].clone(),
                attempt["credential_index"].clone(),
                attempt["rotation_count"].clone(),
            )
        })
        .collect::<Vec<_>>();
    let keys = requests
        .iter()
        .map(|request| query_pairs(request)["api_key"].clone())
        .collect::<Vec<_>>();

    assert_eq!(
        (output.status.code(), attempts, keys),
        (
            Some(0),
            vec![
                (json!("quota_exhausted"), json!(0), json!(0)),
                (Value::Null, json!(1), json!(1)),
            ],
            vec![KEY.to_owned(), SECOND_KEY.to_owned()]
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn an_invalid_key_is_sent_once_without_rotation_and_fails_after_flight() {
    let (output, requests) = run(
        &[KEY, SECOND_KEY],
        &["momentum"],
        vec![Response::json(401, &fixture("badkey.json").to_string())],
    );

    assert_eq!(
        (
            output.status.code(),
            &payload(&output)["error_kind"],
            requests.len()
        ),
        (Some(4), &json!("auth"), 1),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn an_empty_key_pool_fails_before_any_request_and_names_the_keys_setting() {
    let fixture = Fixture::start_canary();
    let environment = RunEnvironment::new(&config(&fixture.url, &[]));

    let output = search(&environment, &["momentum"]);

    assert_eq!(
        (
            output.status.code(),
            stderr(&output).contains("providers.serpapi.keys"),
            fixture.finish_all().len()
        ),
        (Some(3), true, 0),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn an_empty_order_names_only_the_order() {
    let fixture = Fixture::start_canary();
    let environment = RunEnvironment::new(&config(&fixture.url, &[KEY]));

    let output = environment.run_with_env(
        &["platform", "scholar", "search", "momentum"],
        &[("FORAGER_PLATFORMS__SCHOLAR__ORDER", "[]")],
    );
    let message = stderr(&output);

    assert_eq!(
        (
            output.status.code(),
            message.contains("platforms.scholar.order"),
            message.contains("keys"),
            fixture.finish_all().len()
        ),
        (Some(3), true, false, 0),
        "stderr: {message}"
    );
}

#[test]
fn the_key_never_reaches_output_or_trace_logs_on_any_failure() {
    const CANARY: &str = "serpapi-canary-7f3c9e1d";
    let echo = format!("{{\"error\":\"Invalid API key {CANARY}, see ?api_key={CANARY}\"}}");
    let exhausted = format!("{{\"error\":\"Your account has run out of searches for {CANARY}.\"}}");
    let mut reported = status_error();
    reported["error"] = json!(format!("Upstream failed for api_key={CANARY}"));
    for (case, response) in [
        ("401", Some(Response::json(401, &echo))),
        ("429", Some(Response::json(429, &exhausted))),
        ("200 Error", Some(ok(&reported))),
        ("network", None),
    ] {
        let fixture = response.map(Fixture::start_repeating);
        let url = fixture.as_ref().map_or_else(
            || "http://127.0.0.1:9".to_owned(),
            |fixture| fixture.url.clone(),
        );
        let environment = RunEnvironment::new(&config(&url, &[CANARY]));

        let output = environment.run_with_env(
            &["platform", "scholar", "search", "momentum", "--verbose"],
            &[("FORAGER_LOG__LEVEL", "trace")],
        );
        let requests = fixture.map(|fixture| fixture.finish_all().len());

        assert_ne!(output.status.code(), Some(0), "{case} should fail");
        assert_ne!(requests, Some(0), "{case} should reach the fixture");
        for (sink, contents) in [
            (
                "stdout",
                String::from_utf8_lossy(&output.stdout).into_owned(),
            ),
            ("stderr", stderr(&output)),
        ] {
            assert!(!contents.contains(CANARY), "{case}: {sink} leaked the key");
        }
    }
}
