use std::time::Duration;

use serde_json::{Value, json};

use super::{
    SearchResults, Serpapi, cited_by_support, fetch_support, search_support, success_protocol,
};
use crate::config::KeyedHttpRouteRuntimeConfig;
use crate::fixture_server::{Fixture, Response, block_on, ok_json, pairs, path, query};
use crate::net::{RetryPolicy, build_client};
use crate::providers::RouteContext;
use crate::providers::test_projection::{failure, to_json};
use crate::samples::sample_json;
use crate::types::{
    AttemptErrorKind, ContentDepth, Deadline, PlatformFetchOutcome, PlatformFetchRequest,
    PlatformRef, PlatformSearchOptions, PlatformSearchOutcome, PlatformSearchRequest,
    ProviderError, ScholarCitedByRequest, ScholarCitedBySort, ScholarRef, ScholarSearchOptions,
};

const KEY: &str = "test-serpapi-key-a";
const SECOND_KEY: &str = "test-serpapi-key-b";
const CITED: u64 = 18_208_131_694_456_651_388;

/// The route against the fixture at `url`, built as its factory does, with `keys` in pool order.
/// Each request has `attempts` attempts with no wait between them.
fn route(url: &str, keys: &[&str], attempts: usize) -> Serpapi {
    Serpapi::build(
        KeyedHttpRouteRuntimeConfig {
            url: format!("{url}/search.json"),
            keys: keys.iter().map(|key| (*key).into()).collect(),
            timeout_seconds: 5,
        },
        RouteContext {
            client: build_client(true).expect("build client"),
            retry_policy: RetryPolicy::new(attempts, 1.0, Duration::ZERO),
            deadline: Deadline::new(Duration::from_secs(10)),
        },
    )
}

/// The stored search page holding only `results`, which SerpApi says no page follows.
fn last_page(results: &[Value]) -> Value {
    let mut page = sample_json("serpapi/search.json");
    page["organic_results"] = Value::Array(results.to_vec());
    page["serpapi_pagination"]
        .as_object_mut()
        .expect("pagination")
        .remove("next");
    page
}

/// The stored error body, saying `message`.
fn error_body(message: &str) -> String {
    let mut body = sample_json("serpapi/invalid-key.json");
    body["error"] = json!(message);
    body.to_string()
}

/// The stored search page's organic result at `index`.
pub(super) fn result(index: usize) -> Value {
    sample_json("serpapi/search.json")["organic_results"][index].clone()
}

fn search_request(
    query: &str,
    limit: u16,
    options: ScholarSearchOptions,
    page: Option<&str>,
) -> PlatformSearchRequest {
    PlatformSearchRequest {
        query: query.to_owned(),
        limit,
        options: PlatformSearchOptions::Scholar(options),
        page: page.map(str::to_owned),
    }
}

fn cited_by_request(limit: u16, page: Option<&str>) -> ScholarCitedByRequest {
    ScholarCitedByRequest {
        cited: ScholarRef::from(CITED),
        query: None,
        limit,
        year_from: None,
        year_to: None,
        sort: ScholarCitedBySort::Relevance,
        page: page.map(str::to_owned),
    }
}

/// Searches with `request` through a route with `keys` and `attempts` attempts against a fixture
/// that answers `answers` in order, and returns the outcome and the requests the fixture received.
fn search_with(
    keys: &[&str],
    attempts: usize,
    request: &PlatformSearchRequest,
    answers: Vec<Response>,
) -> (Result<PlatformSearchOutcome, ProviderError>, Vec<String>) {
    let fixture = Fixture::start_sequence(answers);
    let outcome = block_on(route(&fixture.url, keys, attempts).search(request));
    (outcome, fixture.finish_all())
}

fn search(
    request: &PlatformSearchRequest,
    answers: Vec<Response>,
) -> (Result<PlatformSearchOutcome, ProviderError>, Vec<String>) {
    search_with(&[KEY], 1, request, answers)
}

fn cited_by(
    attempts: usize,
    request: &ScholarCitedByRequest,
    answers: Vec<Response>,
) -> (Result<PlatformSearchOutcome, ProviderError>, Vec<String>) {
    let fixture = Fixture::start_sequence(answers);
    let outcome = block_on(route(&fixture.url, &[KEY], attempts).cited_by(request));
    (outcome, fixture.finish_all())
}

fn fetch(
    reference: &str,
    depth: ContentDepth,
    answers: Vec<Response>,
) -> (Result<PlatformFetchOutcome, ProviderError>, Vec<String>) {
    let fixture = Fixture::start_sequence(answers);
    let reference = PlatformRef::Scholar(ScholarRef::parse(reference).expect("valid ref"));
    let request = PlatformFetchRequest::new(reference, depth);
    let outcome = block_on(route(&fixture.url, &[KEY], 3).fetch(&request));
    (outcome, fixture.finish_all())
}

fn next_page(outcome: Result<PlatformSearchOutcome, ProviderError>) -> Option<String> {
    outcome.expect("a page").next_page
}

// The HTTP 200 success protocol.

fn results_or_failure(body: &Value) -> Result<SearchResults, (AttemptErrorKind, String)> {
    success_protocol(&body.to_string()).map_err(|failure| (failure.kind, failure.message))
}

#[test]
fn results_with_success_are_found_with_the_next_page_signal() {
    let page = |pagination: Option<Value>| {
        let mut body = sample_json("serpapi/search.json");
        let fields = body.as_object_mut().expect("search body");
        match pagination {
            Some(pagination) => fields.insert("serpapi_pagination".to_owned(), pagination),
            None => fields.remove("serpapi_pagination"),
        };
        body
    };
    let results = [
        Some(json!({"next": "https://serpapi.com/search.json?start=20"})),
        Some(json!({"current": 1})),
        None,
    ]
    .map(|pagination| results_or_failure(&page(pagination)));

    let found = |has_next_page| {
        Ok(SearchResults::Found {
            results: sample_json("serpapi/search.json")["organic_results"]
                .as_array()
                .expect("stored results")
                .clone(),
            has_next_page,
        })
    };
    assert_eq!(results, [found(true), found(false), found(false)]);
}

#[test]
fn a_fully_empty_success_without_results_is_empty() {
    assert_eq!(
        results_or_failure(&sample_json("serpapi/fully-empty.json")),
        Ok(SearchResults::Empty)
    );
}

#[test]
fn an_error_status_is_a_network_failure_with_serpapi_message() {
    assert_eq!(
        results_or_failure(&sample_json("serpapi/status-error.json")),
        Err((
            AttemptErrorKind::Network,
            "SerpApi reported an error: Google hasn't returned any results for this query."
                .to_owned()
        ))
    );
}

#[test]
fn any_other_status_or_results_shape_is_a_runtime_failure() {
    let search_page = |change: &dyn Fn(&mut serde_json::Map<String, Value>)| {
        let mut page = sample_json("serpapi/search.json");
        change(page.as_object_mut().expect("search page"));
        page
    };
    let mut fully_empty = sample_json("serpapi/fully-empty.json");
    fully_empty["organic_results"] = json!([]);
    let results = [
        search_page(&|page| {
            page.remove("organic_results");
        }),
        search_page(&|page| {
            page.remove("search_information");
            page.insert("organic_results".into(), json!([]));
        }),
        fully_empty,
        search_page(&|page| {
            page["search_metadata"]
                .as_object_mut()
                .expect("metadata")
                .remove("status");
        }),
        search_page(&|page| page["search_metadata"]["status"] = json!("Processing")),
    ]
    .map(|body| results_or_failure(&body));

    let runtime = |message: &str| Err((AttemptErrorKind::Runtime, message.to_owned()));
    assert_eq!(
        results,
        [
            runtime(
                "SerpApi reported success without results (results state `Results for exact spelling`)"
            ),
            runtime("SerpApi reported success without results (results state `none`)"),
            runtime("SerpApi reported success without results (results state `Fully empty`)"),
            runtime("SerpApi response has no search status"),
            runtime("SerpApi returned search status `Processing`"),
        ]
    );
}

#[test]
fn a_body_that_is_no_serpapi_envelope_is_a_runtime_failure() {
    let mut page = sample_json("serpapi/search.json");
    page["organic_results"] = json!({"position": 1});

    let failure = results_or_failure(&page).unwrap_err();

    assert_eq!(
        (
            failure.0,
            failure.1.starts_with("invalid SerpApi response: ")
        ),
        (AttemptErrorKind::Runtime, true),
        "message: {}",
        failure.1
    );
}

// Search requests and page positions.

#[test]
fn a_search_sends_the_engine_language_query_and_page_size_with_the_key() {
    let request = search_request(
        "\"time series momentum\" author:\"Pedersen\"",
        5,
        ScholarSearchOptions::default(),
        None,
    );

    let (outcome, requests) = search(&request, vec![ok_json(&sample_json("serpapi/search.json"))]);

    assert_eq!(
        (outcome.is_ok(), path(&requests[0]), query(&requests[0])),
        (
            true,
            "/search.json".to_owned(),
            pairs(&[
                ("engine", "google_scholar"),
                ("hl", "en"),
                ("q", "\"time series momentum\" author:\"Pedersen\""),
                ("num", "5"),
                ("api_key", KEY),
            ])
        )
    );
}

#[test]
fn year_bounds_and_review_only_join_the_query_parameters() {
    let options = ScholarSearchOptions {
        year_from: Some(2020),
        year_to: Some(2024),
        review_only: true,
    };
    let request = search_request("momentum", 20, options, None);

    let (outcome, requests) = search(&request, vec![ok_json(&sample_json("serpapi/search.json"))]);
    let query = query(&requests[0]);

    assert_eq!(
        (
            outcome.is_ok(),
            ["as_ylo", "as_yhi", "as_rr"].map(|name| query.get(name).cloned())
        ),
        (
            true,
            [Some("2020"), Some("2024"), Some("1")].map(|value| value.map(str::to_owned))
        )
    );
}

#[test]
fn a_first_page_without_years_or_review_only_sends_no_filter_or_start_parameters() {
    let request = search_request("momentum", 20, ScholarSearchOptions::default(), None);

    let (outcome, requests) = search(&request, vec![ok_json(&sample_json("serpapi/search.json"))]);
    let query = query(&requests[0]);

    assert_eq!(
        (
            outcome.is_ok(),
            ["as_ylo", "as_yhi", "as_rr", "start"].map(|name| query.contains_key(name))
        ),
        (true, [false; 4])
    );
}

#[test]
fn the_next_page_starts_one_page_size_later_and_only_adds_its_start() {
    let options = ScholarSearchOptions {
        year_from: Some(2020),
        year_to: Some(2024),
        review_only: true,
    };
    let first = search_request("\"time series momentum\"", 10, options, None);
    let (first_outcome, first_requests) =
        search(&first, vec![ok_json(&sample_json("serpapi/search.json"))]);
    let next = next_page(first_outcome);
    let second = PlatformSearchRequest {
        page: next.clone(),
        ..first
    };

    let (outcome, requests) = search(&second, vec![ok_json(&sample_json("serpapi/search.json"))]);
    let mut resumed = query(&requests[0]);
    let start = resumed.remove("start");

    assert_eq!(
        (next.as_deref(), outcome.is_ok(), start.as_deref(), resumed),
        (Some("10"), true, Some("10"), query(&first_requests[0]))
    );
}

#[test]
fn a_page_without_a_next_link_ends_the_search() {
    for page in [None, Some("980")] {
        let request = search_request("momentum", 20, ScholarSearchOptions::default(), page);

        let (outcome, requests) =
            search(&request, vec![ok_json(&last_page(&[result(0), result(1)]))]);
        let outcome = outcome.expect("a page");

        assert_eq!(
            (
                query(&requests[0]).get("start").cloned(),
                outcome.items.len(),
                outcome.next_page
            ),
            (page.map(str::to_owned), 2, None),
            "page: {page:?}"
        );
    }
}

#[test]
fn no_next_page_is_named_when_it_would_pass_result_1000() {
    let next = |start: &str| {
        let request = search_request("momentum", 7, ScholarSearchOptions::default(), Some(start));
        next_page(search(&request, vec![ok_json(&sample_json("serpapi/search.json"))]).0)
    };

    assert_eq!([next("980"), next("987")], [Some("987".to_owned()), None]);
}

#[test]
fn twenty_result_pages_reach_result_1000_exactly() {
    let page = |start: &str| {
        let request = search_request("momentum", 20, ScholarSearchOptions::default(), Some(start));
        let (outcome, requests) =
            search(&request, vec![ok_json(&sample_json("serpapi/search.json"))]);
        (query(&requests[0])["start"].clone(), next_page(outcome))
    };

    assert_eq!(
        [page("960"), page("980")],
        [
            ("960".to_owned(), Some("980".to_owned())),
            ("980".to_owned(), None)
        ]
    );
}

#[test]
fn a_page_position_past_result_1000_or_not_issued_by_the_route_is_refused_before_any_request() {
    for (page, message) in [
        (
            "1000",
            "serpapi cannot page past Google Scholar result 1000",
        ),
        ("-20", "invalid Google Scholar page position `-20`"),
    ] {
        let request = search_request("momentum", 20, ScholarSearchOptions::default(), Some(page));

        let support = search_support(&request);
        let (outcome, requests) = search(&request, Vec::new());

        assert_eq!(
            (support, failure(outcome), requests.len()),
            (
                Err(message.to_owned()),
                (AttemptErrorKind::Parameter, message.to_owned()),
                0
            ),
            "page: {page}"
        );
    }
}

// Search outcomes.

#[test]
fn a_fully_empty_answer_is_an_empty_page() {
    let request = search_request(
        "\"zxqvbn qqqzzz momentum kkkwww\"",
        20,
        ScholarSearchOptions::default(),
        None,
    );

    let (outcome, requests) = search(
        &request,
        vec![ok_json(&sample_json("serpapi/fully-empty.json"))],
    );
    let outcome = outcome.expect("an empty page");

    assert_eq!(
        (outcome.items.len(), outcome.next_page, requests.len()),
        (0, None, 1)
    );
}

#[test]
fn a_page_without_a_usable_result_is_a_runtime_failure_that_is_not_retried() {
    let mut unidentified = result(3);
    unidentified["result_id"] = Value::Null;
    for (case, results) in [
        ("every result skipped", vec![unidentified]),
        ("no results", vec![]),
    ] {
        let mut page = sample_json("serpapi/search.json");
        page["organic_results"] = Value::Array(results);
        let request = search_request("momentum", 20, ScholarSearchOptions::default(), None);

        let (outcome, requests) = search_with(&[KEY], 3, &request, vec![ok_json(&page)]);

        assert_eq!(
            (failure(outcome).0, requests.len()),
            (AttemptErrorKind::Runtime, 1),
            "{case}"
        );
    }
}

// Retries, keys and attempts.

#[test]
fn a_status_error_is_a_network_failure_that_the_shared_policy_retries() {
    let request = search_request("momentum", 20, ScholarSearchOptions::default(), None);

    let (outcome, requests) = search_with(
        &[KEY],
        3,
        &request,
        vec![
            ok_json(&sample_json("serpapi/status-error.json")),
            ok_json(&sample_json("serpapi/search.json")),
        ],
    );
    let attempts = outcome
        .expect("the retry succeeds")
        .attempts
        .iter()
        .map(|attempt| {
            (
                attempt.error_kind,
                attempt.http_status,
                attempt.retry_count,
                attempt.message.clone(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        (requests.len(), attempts),
        (
            2,
            vec![
                (
                    Some(AttemptErrorKind::Network),
                    Some(200),
                    0,
                    "SerpApi reported an error: Google hasn't returned any results for this query."
                        .to_owned()
                ),
                (None, Some(200), 1, String::new()),
            ]
        )
    );
}

#[test]
fn an_exhausted_key_rotates_to_the_next_key_in_the_same_request() {
    let keys = [KEY, SECOND_KEY];
    let request = search_request("momentum", 20, ScholarSearchOptions::default(), None);

    let (outcome, requests) = search_with(
        &keys,
        1,
        &request,
        vec![
            Response::json(429, &error_body("Your account has run out of searches.")),
            ok_json(&sample_json("serpapi/search.json")),
        ],
    );
    let attempts = outcome.expect("the next key succeeds").attempts;
    // The credential cursor persists across searches, so the first key may be either.
    let first = attempts[0].credential_index;
    let second = (first + 1) % keys.len();
    let sent = requests
        .iter()
        .map(|request| query(request)["api_key"].clone())
        .collect::<Vec<_>>();

    assert_eq!(
        (
            attempts
                .iter()
                .map(|attempt| (
                    attempt.error_kind,
                    attempt.credential_index,
                    attempt.rotation_count
                ))
                .collect::<Vec<_>>(),
            sent
        ),
        (
            vec![
                (Some(AttemptErrorKind::QuotaExhausted), first, 0),
                (None, second, 1),
            ],
            vec![keys[first].to_owned(), keys[second].to_owned()]
        )
    );
}

#[test]
fn an_invalid_key_is_sent_once_without_rotation_or_retry() {
    let request = search_request("momentum", 20, ScholarSearchOptions::default(), None);

    let (outcome, requests) = search_with(
        &[KEY, SECOND_KEY],
        3,
        &request,
        vec![Response::json(
            401,
            &sample_json("serpapi/invalid-key.json").to_string(),
        )],
    );

    assert_eq!(
        (failure(outcome).0, requests.len()),
        (AttemptErrorKind::Auth, 1)
    );
}

#[test]
fn the_key_never_reaches_a_failure_or_its_attempts() {
    const CANARY: &str = "serpapi-canary-7f3c9e1d";
    let echo = error_body(&format!("Invalid API key {CANARY}, see ?api_key={CANARY}"));
    let exhausted = error_body(&format!(
        "Your account has run out of searches for {CANARY}."
    ));
    let mut reported = sample_json("serpapi/status-error.json");
    reported["error"] = json!(format!("Upstream failed for api_key={CANARY}"));
    for (case, response) in [
        ("401", Some(Response::json(401, &echo))),
        ("429", Some(Response::json(429, &exhausted))),
        ("200 Error", Some(ok_json(&reported))),
        ("network", None),
    ] {
        let fixture = response.map(Fixture::start_repeating);
        let url = fixture.as_ref().map_or_else(
            || "http://127.0.0.1:9".to_owned(),
            |fixture| fixture.url.clone(),
        );
        let request = search_request("momentum", 20, ScholarSearchOptions::default(), None);

        let outcome = block_on(route(&url, &[CANARY], 3).search(&request));
        let requests = fixture.map(|fixture| fixture.finish_all().len());

        let Err(error) = outcome else {
            panic!("{case} should fail");
        };
        assert_ne!(requests, Some(0), "{case} should reach the fixture");
        assert!(
            !format!("{error:?}").contains(CANARY),
            "{case} leaked the key: {error:?}"
        );
    }
}

// Cited-by.

#[test]
fn a_cited_by_page_sends_the_cited_cluster_and_page_size_with_the_key() {
    let (outcome, requests) = cited_by(
        1,
        &cited_by_request(20, None),
        vec![ok_json(&sample_json("serpapi/cited_by.json"))],
    );

    assert_eq!(
        (outcome.is_ok(), path(&requests[0]), query(&requests[0])),
        (
            true,
            "/search.json".to_owned(),
            pairs(&[
                ("engine", "google_scholar"),
                ("hl", "en"),
                ("cites", "18208131694456651388"),
                ("num", "20"),
                ("api_key", KEY),
            ])
        )
    );
}

#[test]
fn the_query_years_and_date_order_join_the_cited_by_parameters() {
    let filtered = ScholarCitedByRequest {
        query: Some("crash".to_owned()),
        year_from: Some(2020),
        year_to: Some(2024),
        ..cited_by_request(20, None)
    };
    let by_date = ScholarCitedByRequest {
        sort: ScholarCitedBySort::Date,
        ..cited_by_request(20, None)
    };
    for (request, expected) in [
        (filtered, [Some("crash"), Some("2020"), Some("2024"), None]),
        (by_date, [None, None, None, Some("2")]),
    ] {
        let (outcome, requests) = cited_by(
            1,
            &request,
            vec![ok_json(&sample_json("serpapi/cited_by.json"))],
        );
        let query = query(&requests[0]);

        assert_eq!(
            (
                outcome.is_ok(),
                ["q", "as_ylo", "as_yhi", "scisbd"].map(|name| query.get(name).cloned())
            ),
            (true, expected.map(|value| value.map(str::to_owned))),
            "request: {request:?}"
        );
    }
}

#[test]
fn the_next_cited_by_page_starts_one_page_size_later_and_only_adds_its_start() {
    let first = ScholarCitedByRequest {
        query: Some("crash".to_owned()),
        year_from: Some(2020),
        year_to: Some(2024),
        ..cited_by_request(10, None)
    };
    let (first_outcome, first_requests) = cited_by(
        1,
        &first,
        vec![ok_json(&sample_json("serpapi/cited_by.json"))],
    );
    let next = next_page(first_outcome);
    let second = ScholarCitedByRequest {
        page: next.clone(),
        ..first
    };

    let (outcome, requests) = cited_by(
        1,
        &second,
        vec![ok_json(&sample_json("serpapi/cited_by.json"))],
    );
    let mut resumed = query(&requests[0]);
    let start = resumed.remove("start");

    assert_eq!(
        (next.as_deref(), outcome.is_ok(), start.as_deref(), resumed),
        (Some("10"), true, Some("10"), query(&first_requests[0]))
    );
}

#[test]
fn a_date_ordered_page_keeps_the_order_at_its_position() {
    let request = ScholarCitedByRequest {
        sort: ScholarCitedBySort::Date,
        ..cited_by_request(20, Some("20"))
    };

    let (outcome, requests) = cited_by(
        1,
        &request,
        vec![ok_json(&sample_json("serpapi/cited_by.json"))],
    );
    let query = query(&requests[0]);

    assert_eq!(
        (
            outcome.is_ok(),
            query.get("scisbd").map(String::as_str),
            query.get("start").map(String::as_str)
        ),
        (true, Some("2"), Some("20"))
    );
}

#[test]
fn a_paper_without_citing_works_lists_nothing_and_succeeds() {
    let request = ScholarCitedByRequest {
        cited: ScholarRef::from(1),
        ..cited_by_request(20, None)
    };

    let (outcome, requests) = cited_by(
        1,
        &request,
        vec![ok_json(&sample_json("serpapi/fully-empty.json"))],
    );
    let outcome = outcome.expect("an empty page");

    assert_eq!(
        (outcome.items.len(), outcome.next_page, requests.len()),
        (0, None, 1)
    );
}

#[test]
fn a_cited_by_page_past_result_1000_is_refused_before_any_request() {
    let message = "serpapi cannot page past Google Scholar result 1000";
    let request = cited_by_request(20, Some("1000"));

    let support = cited_by_support(&request);
    let (outcome, requests) = cited_by(1, &request, Vec::new());

    assert_eq!(
        (support, failure(outcome), requests.len()),
        (
            Err(message.to_owned()),
            (AttemptErrorKind::Parameter, message.to_owned()),
            0
        )
    );
}

#[test]
fn a_status_error_is_retried_under_the_cited_by_target() {
    let (outcome, requests) = cited_by(
        3,
        &cited_by_request(20, None),
        vec![
            ok_json(&sample_json("serpapi/status-error.json")),
            ok_json(&sample_json("serpapi/cited_by.json")),
        ],
    );
    let attempts = outcome
        .expect("the retry succeeds")
        .attempts
        .iter()
        .map(|attempt| {
            let attempt = to_json(attempt);
            (
                attempt["error_kind"].clone(),
                attempt["platform"].clone(),
                attempt["operation"].clone(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        (requests.len(), attempts),
        (
            2,
            vec![
                (json!("network"), json!("scholar"), json!("cited_by")),
                (Value::Null, json!("scholar"), json!("cited_by")),
            ]
        )
    );
}

// Fetch.

#[test]
fn a_fetch_requests_one_full_page_of_the_cluster() {
    let (outcome, requests) = fetch(
        "scholar:18208131694456651388",
        ContentDepth::Metadata,
        vec![ok_json(&sample_json("serpapi/cluster.json"))],
    );

    assert_eq!(
        (outcome.is_ok(), path(&requests[0]), query(&requests[0])),
        (
            true,
            "/search.json".to_owned(),
            pairs(&[
                ("engine", "google_scholar"),
                ("hl", "en"),
                ("cluster", "18208131694456651388"),
                ("num", "20"),
                ("api_key", KEY),
            ])
        )
    );
}

#[test]
fn a_cluster_with_more_versions_than_one_page_says_the_list_is_incomplete() {
    let mut cluster = sample_json("serpapi/cluster.json");
    cluster["serpapi_pagination"] = json!({
        "current": 1,
        "next": "https://serpapi.com/search.json?cluster=18208131694456651388&engine=google_scholar&hl=en&num=20&start=20"
    });

    let (outcome, _) = fetch(
        "scholar:18208131694456651388",
        ContentDepth::Metadata,
        vec![ok_json(&cluster)],
    );
    let outcome = outcome.expect("a cluster");
    let diagnostic = outcome.diagnostic.unwrap_or_default();

    assert_eq!(
        (
            to_json(&outcome.item)["versions"].as_array().map(Vec::len),
            diagnostic.contains("more versions of scholar:18208131694456651388"),
            diagnostic.contains("https://scholar.google.com/scholar?cluster=18208131694456651388"),
        ),
        (Some(3), true, true),
        "diagnostic: {diagnostic}"
    );
}

#[test]
fn a_cluster_that_does_not_exist_is_a_parameter_failure() {
    let (outcome, requests) = fetch(
        "scholar:1",
        ContentDepth::Metadata,
        vec![ok_json(&sample_json("serpapi/fully-empty.json"))],
    );

    assert_eq!(
        (failure(outcome), requests.len()),
        (
            (
                AttemptErrorKind::Parameter,
                "Google Scholar has no cluster scholar:1".to_owned()
            ),
            1
        )
    );
}

#[test]
fn a_fetch_at_any_depth_but_metadata_is_refused_before_any_request() {
    for (depth, name) in [
        (ContentDepth::Snippet, "snippet"),
        (ContentDepth::Abstract, "abstract"),
        (ContentDepth::FullText, "full_text"),
    ] {
        let message = format!(
            "serpapi cannot fetch at depth `{name}`; Google Scholar provides metadata only"
        );
        let reference = PlatformRef::Scholar(ScholarRef::from(CITED));

        let support = fetch_support(&PlatformFetchRequest::new(reference, depth));
        let (outcome, requests) = fetch("scholar:18208131694456651388", depth, Vec::new());

        assert_eq!(
            (support, failure(outcome), requests.len()),
            (
                Err(message.clone()),
                (AttemptErrorKind::Parameter, message),
                0
            ),
            "depth: {name}"
        );
    }
}
