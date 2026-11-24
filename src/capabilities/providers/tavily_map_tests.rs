use std::time::Duration;

use serde_json::{Value, json};

use super::{MapRequest, TavilyMap, TavilyMapResponse, decode_site};
use crate::config::WebFetchProviderConfig;
use crate::credentials::CredentialPool;
use crate::fixture_server::{
    Fixture, Response, advance_after_request, block_on, header, path, request_json,
};
use crate::net::{RetryPolicy, build_client};
use crate::providers::constructors::build_tavily_map;
use crate::samples::{sample, sample_json};
use crate::types::{AttemptErrorKind, Deadline, MapOutcome, ProviderError};

const KEY: &str = "tavily-key";
const SECOND_KEY: &str = "second-key";

/// The stored map body with `field` replaced, or removed when `value` is null.
fn map_with(field: &str, value: Value) -> String {
    let mut body = sample_json("tavily/map.json");
    let object = body.as_object_mut().expect("map object");
    if value.is_null() {
        object.remove(field);
    } else {
        object.insert(field.to_owned(), value);
    }
    body.to_string()
}

fn decode(text: &str) -> Result<TavilyMapResponse, String> {
    decode_site(text, &CredentialPool::new("tavily", Vec::new()))
}

/// The route as the provider factory builds it, on the fixture at `url`, with `attempts` attempts
/// per request and a per-attempt timeout of `timeout_seconds`.
fn route(url: &str, keys: &[&str], attempts: usize, timeout_seconds: u64) -> TavilyMap {
    build_tavily_map(
        WebFetchProviderConfig {
            url: url.to_owned(),
            keys: keys.iter().map(|key| (*key).into()).collect(),
            timeout_seconds,
            respond_with: String::new(),
        },
        build_client(true).expect("build client"),
        RetryPolicy::new(attempts, 1.0, Duration::ZERO),
        Deadline::new(Duration::from_secs(10)),
    )
}

fn map_request(max_depth: u16, max_breadth: u16, timeout_seconds: u64) -> MapRequest {
    MapRequest {
        url: "https://docs.example.test".into(),
        instructions: String::new(),
        max_depth,
        max_breadth,
        limit: 50,
        timeout_seconds,
        verbose: true,
    }
}

fn map_with_responses(
    keys: &[&str],
    attempts: usize,
    responses: Vec<Response>,
) -> (Result<MapOutcome, ProviderError>, Vec<String>) {
    let fixture = Fixture::start_sequence(responses);
    let outcome = block_on(route(&fixture.url, keys, attempts, 30).map(map_request(1, 20, 150)));
    (outcome, fixture.finish_all())
}

fn failure_kinds(error: &ProviderError) -> (AttemptErrorKind, Vec<Option<AttemptErrorKind>>) {
    (
        error.kind,
        error
            .attempts
            .iter()
            .map(|attempt| attempt.error_kind)
            .collect(),
    )
}

#[test]
fn a_site_map_decodes_its_base_url_and_results_with_their_secrets_redacted() {
    let site = decode(&sample("tavily/map.json")).expect("stored map decodes");

    assert_eq!(
        (site.base_url, site.results, site.response_time),
        (
            "https://docs.example.test".to_owned(),
            vec![
                "https://docs.example.test/api?token=********".to_owned(),
                "https://docs.example.test/guides".to_owned(),
            ],
            0.42,
        )
    );
}

#[test]
fn a_site_without_pages_decodes_to_no_results() {
    let site = decode(&map_with("results", json!([]))).expect("an empty site decodes");

    assert_eq!(
        (site.base_url, site.results),
        ("https://docs.example.test".to_owned(), Vec::<String>::new())
    );
}

#[test]
fn a_body_missing_its_base_url_or_results_or_naming_a_non_http_page_is_refused() {
    let refused = [
        "{}".to_owned(),
        map_with("results", Value::Null),
        map_with("base_url", Value::Null),
        map_with("results", json!(["not-a-url"])),
    ]
    .map(|body| decode(&body).err());

    assert_eq!(
        refused.map(|message| message.map(|message| message.split(':').next().map(str::to_owned))),
        [
            Some(Some("invalid Tavily map response".to_owned())),
            Some(Some("invalid Tavily map response".to_owned())),
            Some(Some("invalid Tavily map response".to_owned())),
            Some(Some(
                "results must contain only HTTP(S) URLs with a host".to_owned()
            )),
        ]
    );
}

#[test]
fn a_malformed_success_body_is_a_runtime_failure() {
    let (outcome, _) = map_with_responses(
        &[KEY],
        1,
        vec![Response::json(200, &map_with("results", Value::Null))],
    );

    assert_eq!(
        failure_kinds(&outcome.expect_err("a malformed body fails")),
        (
            AttemptErrorKind::Runtime,
            vec![Some(AttemptErrorKind::Runtime)]
        )
    );
}

#[test]
fn a_map_posts_every_option_with_the_key_as_a_bearer_token() {
    let fixture = Fixture::start_json(200, &sample("tavily/map.json"));
    let request = MapRequest {
        instructions: "Only public API documentation".into(),
        limit: 25,
        ..map_request(2, 10, 150)
    };

    let outcome = block_on(route(&fixture.url, &[KEY], 1, 30).map(request));
    let sent = fixture.finish();

    assert_eq!(
        (
            outcome.map(|outcome| (outcome.provider, outcome.url)).ok(),
            sent.split(' ').next(),
            path(&sent),
            header(&sent, "authorization"),
            request_json(&sent),
        ),
        (
            Some(("tavily", "https://docs.example.test".to_owned())),
            Some("POST"),
            "/map".to_owned(),
            Some(format!("Bearer {KEY}")),
            json!({
                "url": "https://docs.example.test",
                "instructions": "Only public API documentation",
                "max_depth": 2,
                "max_breadth": 10,
                "limit": 25,
                "timeout": 150
            }),
        )
    );
}

#[test]
fn boundary_depths_breadths_and_timeouts_are_sent_unchanged_without_empty_instructions() {
    let sent = [(1, 1, 10), (5, 500, 150)].map(|(depth, breadth, timeout)| {
        let fixture = Fixture::start_json(200, &sample("tavily/map.json"));
        block_on(route(&fixture.url, &[KEY], 1, 30).map(map_request(depth, breadth, timeout)))
            .expect("map succeeds");
        request_json(&fixture.finish())
    });

    assert_eq!(
        sent,
        [
            json!({"url": "https://docs.example.test", "max_depth": 1, "max_breadth": 1, "limit": 50, "timeout": 10}),
            json!({"url": "https://docs.example.test", "max_depth": 5, "max_breadth": 500, "limit": 50, "timeout": 150}),
        ]
    );
}

#[test]
fn a_refused_parameter_and_a_refused_key_keep_their_failure_kinds() {
    let kinds = [400, 401].map(|status| {
        let (outcome, _) = map_with_responses(
            &[KEY],
            1,
            vec![Response::json(status, &sample("tavily/error.json"))],
        );
        let error = outcome.expect_err("an error answer fails");
        (error.kind, error.message)
    });

    assert_eq!(
        kinds,
        [
            (
                AttemptErrorKind::Parameter,
                "max_depth is invalid".to_owned()
            ),
            (AttemptErrorKind::Auth, "max_depth is invalid".to_owned()),
        ]
    );
}

#[test]
fn a_rate_limited_key_rotates_to_the_next_key() {
    let keys = [KEY, SECOND_KEY];
    let (outcome, requests) = map_with_responses(
        &keys,
        1,
        vec![
            Response::json(429, &sample("tavily/error.json")),
            Response::json(200, &sample("tavily/map.json")),
        ],
    );
    let attempts = outcome.expect("the next key succeeds").attempts;
    // The credential cursor persists across maps, so the first key may be either.
    let first = attempts[0].credential_index;
    let second = (first + 1) % keys.len();

    assert_eq!(
        (
            attempts
                .iter()
                .map(|attempt| (attempt.error_kind, attempt.rotation_count))
                .collect::<Vec<_>>(),
            requests
                .iter()
                .map(|request| header(request, "authorization"))
                .collect::<Vec<_>>(),
        ),
        (
            vec![(Some(AttemptErrorKind::RateLimited), 0), (None, 1)],
            vec![
                Some(format!("Bearer {}", keys[first])),
                Some(format!("Bearer {}", keys[second])),
            ],
        )
    );
}

#[test]
fn a_server_failure_is_retried_with_the_same_key() {
    let (outcome, _) = map_with_responses(
        &[KEY],
        2,
        vec![
            Response::json(500, &sample("tavily/error.json")),
            Response::json(200, &sample("tavily/map.json")),
        ],
    );

    assert_eq!(
        outcome
            .expect("the retry succeeds")
            .attempts
            .iter()
            .map(|attempt| (attempt.error_kind, attempt.retry_count))
            .collect::<Vec<_>>(),
        vec![(Some(AttemptErrorKind::Network), 0), (None, 1)]
    );
}

#[test]
fn each_attempt_ends_at_the_configured_timeout_inside_a_longer_deadline() {
    let fixture = Fixture::start_unanswered();
    let route = route(&fixture.url, &[KEY], 1, 1);

    let error = advance_after_request(
        &fixture,
        Duration::from_secs(1),
        route.map(map_request(1, 20, 150)),
    )
    .expect_err("the unanswered attempt times out");
    fixture.finish();

    assert_eq!(
        (
            failure_kinds(&error),
            error.attempts.first().map(|attempt| attempt.duration_ms),
        ),
        (
            (
                AttemptErrorKind::Timeout,
                vec![Some(AttemptErrorKind::Timeout)]
            ),
            Some(1_000),
        )
    );
}
