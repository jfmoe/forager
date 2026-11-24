//! Main search through xAI: decoding a completed answer, the request, and the HTTP semantics of
//! the Responses stream. The X platform route's tests are in `xai_x_tests.rs`.

use std::time::Duration;

use serde_json::{Value, json};

use super::Xai;
use crate::config::{MainSearchProviderConfig, XaiRuntimeConfig};
use crate::fixture_server::{Fixture, Response, block_on, header, path, request_json};
use crate::main_search_samples::{answer_with, failed_event, stored_answer};
use crate::net::{AttemptFailure, RetryPolicy, build_client};
use crate::providers::build_main_search;
use crate::providers::{MainSearchRequest, MainSearchRequestKind, RouteContext};
use crate::samples::sample;
use crate::types::{AttemptErrorKind, Deadline, ProviderAttempt, ProviderError, SearchOutcome};
use crate::x_samples::completed_events;

const KEY: &str = "xai-key";
const SECOND_KEY: &str = "xai-key-2";
const THIRD_KEY: &str = "xai-key-3";

fn config(url: &str, keys: &[&str]) -> XaiRuntimeConfig {
    XaiRuntimeConfig {
        url: format!("{url}/v1"),
        keys: keys.iter().map(|key| (*key).into()).collect(),
        model: "test-model".to_owned(),
        tools: vec!["web_search".to_owned()],
    }
}

/// Decodes the completed `response` to a search, with the configured key as the only secret.
fn decode(response: &Value) -> Result<(String, Vec<(String, String)>), AttemptFailure> {
    let route = Xai::build(
        config("http://127.0.0.1:9", &[KEY]),
        RouteContext {
            client: build_client(true).expect("build client"),
            retry_policy: RetryPolicy::new(1, 1.0, Duration::ZERO),
            deadline: Deadline::new(Duration::from_secs(10)),
        },
    );
    route
        .main_search()
        .decode(response, MainSearchRequestKind::Search)
        .map(|(answer, sources)| {
            (
                answer,
                sources
                    .into_iter()
                    .map(|source| (source.title, source.url))
                    .collect(),
            )
        })
}

fn completed(response: &Value) -> Response {
    Response::sse(200, &completed_events(response))
}

/// A stream that ends with the stored `response.failed` event carrying the structured error
/// `code`.
fn failed(code: &str) -> Response {
    let mut event = failed_event("xai");
    event["error"]["code"] = json!(code);
    Response::sse(200, &format!("event: response.failed\ndata: {event}\n\n"))
}

/// The stored error answer of a refused request, its reason replaced by `reason`.
fn refusal(status: u16, reason: &str) -> Response {
    let mut body: Value = serde_json::from_str(&sample("xai/error.json")).expect("stored JSON");
    body["error"] = json!(reason);
    Response::json(status, &body.to_string())
}

/// Runs one main search through xAI as the provider factory builds it, against a fixture that
/// answers with `responses`, with `keys` in pool order and `attempts` attempts per request.
fn search(
    keys: &[&str],
    attempts: usize,
    responses: Vec<Response>,
) -> (Result<SearchOutcome, ProviderError>, Vec<String>) {
    let fixture = Fixture::start_sequence(responses);
    let main_search = build_main_search(
        MainSearchProviderConfig::Xai(config(&fixture.url, keys)),
        build_client(true).expect("build client"),
        RetryPolicy::new(attempts, 1.0, Duration::ZERO),
        Deadline::new(Duration::from_secs(10)),
    );
    let outcome = block_on(main_search.search(MainSearchRequest {
        query: "What changed?".into(),
        model: None,
        verbose: true,
    }));
    (outcome, fixture.finish_all())
}

/// Each attempt's error kind, retry count and rotation count.
fn attempt_counts(attempts: &[ProviderAttempt]) -> Vec<(Option<AttemptErrorKind>, usize, usize)> {
    attempts
        .iter()
        .map(|attempt| {
            (
                attempt.error_kind,
                attempt.retry_count,
                attempt.rotation_count,
            )
        })
        .collect()
}

fn bearers(requests: &[String]) -> Vec<Option<String>> {
    requests
        .iter()
        .map(|request| header(request, "authorization"))
        .collect()
}

#[test]
fn a_search_streams_the_query_to_responses_with_the_configured_model_and_tools() {
    let (outcome, requests) = search(&[KEY], 1, vec![completed(&stored_answer("xai"))]);
    outcome.expect("xai answers");
    let request = &requests[0];
    let body = request_json(request);

    assert_eq!(
        (
            request.split_whitespace().next(),
            path(request),
            header(request, "authorization"),
            header(request, "accept"),
            &body["model"],
            &body["stream"],
            &body["tools"],
            &body["input"][0]["role"],
            body["input"][0]["content"]
                .as_str()
                .is_some_and(|content| content.contains("What changed?")),
            body["instructions"].is_string(),
        ),
        (
            Some("POST"),
            "/v1/responses".to_owned(),
            Some(format!("Bearer {KEY}")),
            Some("text/event-stream".to_owned()),
            &json!("test-model"),
            &json!(true),
            &json!([{"type": "web_search"}]),
            &json!("user"),
            true,
            true,
        )
    );
}

#[test]
fn a_completed_answer_drops_hidden_reasoning_and_lists_each_cited_source_redacted() {
    let (answer, sources) = decode(&stored_answer("xai")).expect("the answer decodes");

    assert_eq!(
        (answer.as_str(), sources),
        (
            "Final answer [[1]](https://example.test/inline)",
            vec![
                (
                    "Source A".to_owned(),
                    "https://example.test/a?token=********".to_owned()
                ),
                (String::new(), "https://example.test/inline".to_owned()),
                ("Tail".to_owned(), "https://example.test/tail".to_owned()),
            ]
        )
    );
}

#[test]
fn an_answer_keeps_its_content_while_its_sources_are_redacted() {
    let text = "xai-key https://example.test/answer?token=answer-secret";
    let response = answer_with(
        "xai",
        text,
        &[(
            "https://example.test/source?token=canary-secret",
            "xai-key source title",
        )],
    );

    assert_eq!(
        decode(&response).expect("the answer decodes"),
        (
            text.to_owned(),
            vec![(
                "******** source title".to_owned(),
                "https://example.test/source?token=********".to_owned()
            )]
        )
    );
}

#[test]
fn an_answer_that_normalization_empties_is_a_runtime_failure() {
    let response = answer_with(
        "xai",
        "<think>hidden-only</think>",
        &[("https://example.test/hidden", "Hidden source")],
    );

    let error = decode(&response).expect_err("no answer is left");
    assert_eq!(
        (error.kind, error.message.as_str()),
        (
            AttemptErrorKind::Runtime,
            "main search response contained no answer after normalization"
        )
    );
}

#[test]
fn a_refused_search_maps_its_status_and_keeps_the_key_out_of_its_message() {
    let cases = [
        (401, AttemptErrorKind::Auth),
        (429, AttemptErrorKind::RateLimited),
        (503, AttemptErrorKind::Network),
    ];

    for (status, expected) in cases {
        let (outcome, requests) = search(
            &[KEY],
            1,
            vec![refusal(status, &format!("refused for {KEY}"))],
        );
        let error = outcome.expect_err("the search is refused");

        assert_eq!(
            (
                error.kind,
                error
                    .attempts
                    .iter()
                    .map(|attempt| attempt.http_status)
                    .collect::<Vec<_>>(),
                error.message.contains(KEY),
                requests.len(),
            ),
            (expected, vec![Some(status)], false, 1),
            "status {status}"
        );
    }
}

#[test]
fn a_structured_server_error_event_is_retried_as_a_network_failure() {
    let (outcome, requests) = search(
        &[KEY],
        2,
        vec![failed("server_error"), completed(&stored_answer("xai"))],
    );
    let outcome = outcome.expect("the retry answers");

    assert_eq!(
        (
            outcome.answer.as_str(),
            attempt_counts(&outcome.attempts),
            requests.len(),
        ),
        (
            "Final answer [[1]](https://example.test/inline)",
            vec![(Some(AttemptErrorKind::Network), 0, 0), (None, 1, 0)],
            2,
        )
    );
}

#[test]
fn a_structured_rate_limit_event_rotates_to_the_next_key() {
    let keys = [KEY, SECOND_KEY];
    let (outcome, requests) = search(
        &keys,
        1,
        vec![
            failed("rate_limit_exceeded"),
            completed(&stored_answer("xai")),
        ],
    );
    let outcome = outcome.expect("the next key answers");
    let first = outcome.attempts[0].credential_index;

    assert_eq!(
        (attempt_counts(&outcome.attempts), bearers(&requests)),
        (
            vec![(Some(AttemptErrorKind::RateLimited), 0, 0), (None, 0, 1)],
            vec![
                Some(format!("Bearer {}", keys[first])),
                Some(format!("Bearer {}", keys[(first + 1) % keys.len()])),
            ],
        )
    );
}

#[test]
fn key_rotation_does_not_consume_the_network_retry_quota() {
    let keys = [KEY, SECOND_KEY, THIRD_KEY];
    let rate_limited = || refusal(429, "rate limited");
    let unavailable = || refusal(503, "network failure");
    let (outcome, requests) = search(
        &keys,
        2,
        vec![rate_limited(), rate_limited(), unavailable(), unavailable()],
    );
    let error = outcome.expect_err("the last key keeps failing");
    let first = error.attempts[0].credential_index;
    let key = |offset: usize| Some(format!("Bearer {}", keys[(first + offset) % keys.len()]));

    assert_eq!(
        (
            error.kind,
            attempt_counts(&error.attempts),
            bearers(&requests)
        ),
        (
            AttemptErrorKind::Network,
            vec![
                (Some(AttemptErrorKind::RateLimited), 0, 0),
                (Some(AttemptErrorKind::RateLimited), 0, 1),
                (Some(AttemptErrorKind::Network), 0, 2),
                (Some(AttemptErrorKind::Network), 1, 2),
            ],
            vec![key(0), key(1), key(2), key(2)],
        )
    );
}

#[test]
fn a_truncated_response_body_is_a_network_failure_that_is_retried() {
    let (outcome, requests) = search(
        &[KEY],
        2,
        vec![
            Response::sse(200, "").with_declared_content_length(1),
            completed(&stored_answer("xai")),
        ],
    );
    let outcome = outcome.expect("the retry answers");

    assert_eq!(
        (attempt_counts(&outcome.attempts), requests.len()),
        (
            vec![(Some(AttemptErrorKind::Network), 0, 0), (None, 1, 0)],
            2
        )
    );
}
