use std::time::Duration;

use serde_json::{Value, json};

use super::{Exa, ExaOperation, ExaSearchRequest, ExaSimilarRequest, SearchType, decode_results};
use crate::config::{DocsSearchProviderConfig, ExaRuntimeConfig};
use crate::credentials::CredentialPool;
use crate::fixture_server::{
    Fixture, Response, advance_after_request, block_on, header, path, request_json,
};
use crate::net::{MAX_RESPONSE_BYTES, RetryPolicy, build_client};
use crate::providers::constructors::build_exa;
use crate::providers::{DocsSearch, build_docs_search};
use crate::samples::sample;
use crate::types::{AttemptErrorKind, AttemptTarget, Deadline, ExaOutcome, ProviderError, Source};

const KEY: &str = "first-key";
const SECOND_KEY: &str = "second-key";
const SOURCE: &str = "https://example.test/source";

fn stored(name: &str) -> String {
    sample(&format!("exa/{name}"))
}

/// The stored body `name` with `field` of its first result set to `value`.
fn first_result_with(name: &str, field: &str, value: Value) -> String {
    let mut body: Value = serde_json::from_str(&stored(name)).expect("stored Exa JSON");
    body["results"][0][field] = value;
    body.to_string()
}

/// The stored search body without results.
fn no_results() -> String {
    let mut body: Value = serde_json::from_str(&stored("search.json")).expect("stored Exa JSON");
    body["results"] = json!([]);
    body.to_string()
}

/// The stored error answer with `message` as its reason.
fn error_with(message: &str) -> String {
    let mut body: Value = serde_json::from_str(&stored("error.json")).expect("stored error JSON");
    body["error"]["message"] = Value::String(message.to_owned());
    body.to_string()
}

fn search_request(query: &str) -> ExaSearchRequest {
    ExaSearchRequest {
        query: query.into(),
        num_results: 5,
        search_type: SearchType::Auto,
        text_max_characters: None,
        include_highlights: false,
        start_published_date: None,
        include_domains: Vec::new(),
        exclude_domains: Vec::new(),
        category: None,
        verbose: true,
    }
}

fn similar_request() -> ExaSimilarRequest {
    ExaSimilarRequest {
        url: SOURCE.into(),
        num_results: 5,
        verbose: true,
    }
}

fn decode(text: &str, operation: &ExaOperation) -> Result<Vec<Source>, String> {
    decode_results(
        text,
        operation,
        &CredentialPool::new("exa", vec![KEY.into()]),
    )
}

fn config(url: &str, keys: &[&str], timeout_seconds: u64) -> ExaRuntimeConfig {
    ExaRuntimeConfig {
        url: url.to_owned(),
        keys: keys.iter().map(|key| (*key).into()).collect(),
        timeout_seconds,
    }
}

/// The route as the provider factory builds it, on the fixture at `url`, with `attempts` attempts
/// per request, a per-attempt timeout of `timeout_seconds` and a command deadline of `deadline`.
fn route(
    url: &str,
    keys: &[&str],
    attempts: usize,
    timeout_seconds: u64,
    deadline: Duration,
) -> Exa {
    build_exa(
        config(url, keys, timeout_seconds),
        build_client(true).expect("build client"),
        RetryPolicy::new(attempts, 1.0, Duration::ZERO),
        Deadline::new(deadline),
    )
}

fn search_with_responses(
    keys: &[&str],
    attempts: usize,
    responses: Vec<Response>,
) -> (Result<ExaOutcome, ProviderError>, Vec<String>) {
    let fixture = Fixture::start_sequence(responses);
    let route = route(&fixture.url, keys, attempts, 30, Duration::from_secs(10));
    let outcome = block_on(route.search(search_request("rust async drop")));
    (outcome, fixture.finish_all())
}

fn keys_sent(requests: &[String]) -> Vec<Option<String>> {
    requests
        .iter()
        .map(|request| header(request, "x-api-key"))
        .collect()
}

#[test]
fn a_search_result_decodes_with_its_secrets_redacted() {
    let operation = ExaOperation::Search(ExaSearchRequest {
        text_max_characters: Some(3000),
        include_highlights: true,
        ..search_request("rust async drop")
    });

    let results = decode(&stored("search.json"), &operation).expect("stored search decodes");

    assert_eq!(
        (
            serde_json::to_value(&results).expect("encode results"),
            results[0].id.as_deref(),
        ),
        (
            json!([{
                "title": "Fixture result",
                "url": "https://example.test/result?api_key=********&safe=1",
                "published_date": "2026-07-25T00:00:00.000Z",
                "author": "Example",
                "text": "Fixture body ********",
                "highlights": ["Fixture highlight ********", "Secondary detail"],
                "image": "https://example.test/image.png",
                "favicon": "https://example.test/favicon.ico"
            }]),
            Some("https://example.test/result"),
        )
    );
}

#[test]
fn a_search_keeps_only_the_contents_it_asked_for() {
    let kept = [(None, false), (Some(500), false), (None, true)].map(
        |(text_max_characters, include_highlights)| {
            let operation = ExaOperation::Search(ExaSearchRequest {
                text_max_characters,
                include_highlights,
                ..search_request("rust async drop")
            });
            let results = decode(&stored("search.json"), &operation).expect("stored search");
            (results[0].text.clone(), results[0].highlights.clone())
        },
    );

    assert_eq!(
        kept,
        [
            (None, Vec::new()),
            (Some("Fixture body ********".to_owned()), Vec::new()),
            (
                None,
                vec![
                    "Fixture highlight ********".to_owned(),
                    "Secondary detail".to_owned()
                ]
            ),
        ]
    );
}

#[test]
fn a_result_without_a_title_keeps_its_url_and_media() {
    let operation = ExaOperation::Search(search_request("rust async drop"));

    let results = decode(
        &first_result_with("search.json", "title", Value::Null),
        &operation,
    )
    .expect("a result without a title decodes");

    assert_eq!(
        serde_json::to_value(&results).expect("encode results"),
        json!([{
            "url": "https://example.test/result?api_key=********&safe=1",
            "published_date": "2026-07-25T00:00:00.000Z",
            "author": "Example",
            "image": "https://example.test/image.png",
            "favicon": "https://example.test/favicon.ico"
        }])
    );
}

#[test]
fn a_similar_page_keeps_its_text_and_highlights_without_asking() {
    let results = decode(
        &stored("similar.json"),
        &ExaOperation::Similar(similar_request()),
    )
    .expect("stored similar pages decode");

    assert_eq!(
        serde_json::to_value(&results).expect("encode results"),
        json!([{
            "title": "Similar fixture",
            "url": "https://example.test/similar?api_key=********&safe=1",
            "published_date": "2026-07-25T00:00:00.000Z",
            "author": "Example",
            "text": "Similar body",
            "highlights": ["Similar highlight"]
        }])
    );
}

#[test]
fn an_empty_result_set_decodes_to_no_results() {
    let results = decode(
        &no_results(),
        &ExaOperation::Search(search_request("no matches")),
    );

    assert_eq!(results.map(|results| results.len()), Ok(0));
}

#[test]
fn a_body_without_results_or_naming_a_non_http_result_is_refused() {
    let operation = ExaOperation::Search(search_request("missing results"));
    let refused = [
        "{}".to_owned(),
        error_with("upstream failed"),
        first_result_with("search.json", "url", json!("ftp://example.test/result")),
    ]
    .map(|body| decode(&body, &operation).err());

    assert_eq!(
        refused.map(|message| message.map(|message| {
            message
                .split(" at line")
                .next()
                .map(str::to_owned)
                .unwrap_or_default()
        })),
        [
            Some("invalid Exa response: missing field `results`".to_owned()),
            Some("invalid Exa response: missing field `results`".to_owned()),
            Some("invalid Exa response: result URL must use HTTP(S)".to_owned()),
        ]
    );
}

#[test]
fn a_refused_success_body_is_a_runtime_failure() {
    let kinds = [
        "{}".to_owned(),
        first_result_with("search.json", "url", json!("ftp://example.test/result")),
    ]
    .map(|body| {
        let (outcome, _) = search_with_responses(&[KEY], 2, vec![Response::json(200, &body)]);
        let error = outcome.expect_err("a refused body fails");
        (error.kind, error.attempts.len())
    });

    assert_eq!(
        kinds,
        [
            (AttemptErrorKind::Runtime, 1),
            (AttemptErrorKind::Runtime, 1)
        ]
    );
}

#[test]
fn a_success_body_is_read_whole_up_to_four_mibibytes_and_refused_beyond() {
    let body = stored("search.json");
    let outcomes = [0, 1].map(|excess| {
        let padded = format!(
            "{body}{}",
            " ".repeat(MAX_RESPONSE_BYTES - body.len() + excess)
        );
        let (outcome, _) = search_with_responses(&[KEY], 1, vec![Response::json(200, &padded)]);
        outcome
            .map(|outcome| outcome.results.len())
            .map_err(|error| (error.kind, error.message))
    });

    assert_eq!(
        outcomes,
        [
            Ok(1),
            Err((
                AttemptErrorKind::Runtime,
                "response exceeded 4 MiB".to_owned()
            )),
        ]
    );
}

#[test]
fn a_search_posts_every_option_with_the_key_in_its_header() {
    let fixture = Fixture::start_json(200, &stored("search.json"));
    let request = ExaSearchRequest {
        num_results: 2,
        search_type: SearchType::Neural,
        text_max_characters: Some(3000),
        include_highlights: true,
        start_published_date: Some("2026-07-01".into()),
        include_domains: vec!["rust-lang.org".into(), "docs.rs".into()],
        exclude_domains: vec!["example.com".into()],
        category: Some("research paper".into()),
        ..search_request("rust async drop")
    };

    let outcome =
        block_on(route(&fixture.url, &[KEY], 1, 30, Duration::from_secs(10)).search(request));
    let sent = fixture.finish();

    assert_eq!(
        (
            outcome
                .map(|outcome| (
                    outcome.provider,
                    outcome.input.operation(),
                    outcome.input.value().to_owned()
                ))
                .ok(),
            sent.split(' ').next(),
            path(&sent),
            header(&sent, "x-api-key"),
            header(&sent, "accept"),
            request_json(&sent),
        ),
        (
            Some(("exa", "search", "rust async drop".to_owned())),
            Some("POST"),
            "/search".to_owned(),
            Some(KEY.to_owned()),
            Some("application/json".to_owned()),
            json!({
                "query": "rust async drop",
                "numResults": 2,
                "type": "neural",
                "contents": {"text": {"maxCharacters": 3000}, "highlights": true},
                "startPublishedDate": "2026-07-01",
                "includeDomains": ["rust-lang.org", "docs.rs"],
                "excludeDomains": ["example.com"],
                "category": "research paper"
            }),
        )
    );
}

#[test]
fn a_search_asks_for_contents_only_when_it_wants_text_or_highlights() {
    let sent = [(None, false), (Some(500), false), (None, true)].map(
        |(text_max_characters, include_highlights)| {
            let fixture = Fixture::start_json(200, &stored("search.json"));
            let request = ExaSearchRequest {
                text_max_characters,
                include_highlights,
                ..search_request("rust async drop")
            };
            block_on(route(&fixture.url, &[KEY], 1, 30, Duration::from_secs(10)).search(request))
                .expect("search succeeds");
            request_json(&fixture.finish())
        },
    );

    assert_eq!(
        sent,
        [
            json!({"query": "rust async drop", "numResults": 5, "type": "auto"}),
            json!({
                "query": "rust async drop",
                "numResults": 5,
                "type": "auto",
                "contents": {"text": {"maxCharacters": 500}, "highlights": false}
            }),
            json!({
                "query": "rust async drop",
                "numResults": 5,
                "type": "auto",
                "contents": {"text": false, "highlights": true}
            }),
        ]
    );
}

#[test]
fn a_similar_posts_its_url_and_count_to_find_similar() {
    let fixture = Fixture::start_json(200, &stored("similar.json"));
    let request = ExaSimilarRequest {
        num_results: 2,
        ..similar_request()
    };

    let outcome =
        block_on(route(&fixture.url, &[KEY], 1, 30, Duration::from_secs(10)).similar(request));
    let sent = fixture.finish();

    assert_eq!(
        (
            outcome
                .map(|outcome| (
                    outcome.provider,
                    outcome.input.operation(),
                    outcome.input.value().to_owned()
                ))
                .ok(),
            sent.split(' ').next(),
            path(&sent),
            header(&sent, "x-api-key"),
            request_json(&sent),
        ),
        (
            Some(("exa", "similar", SOURCE.to_owned())),
            Some("POST"),
            "/findSimilar".to_owned(),
            Some(KEY.to_owned()),
            json!({"url": SOURCE, "numResults": 2}),
        )
    );
}

#[test]
fn a_refused_key_fails_once_as_auth_without_echoing_the_key() {
    let canary = "credential-canary-do-not-print";
    let body = error_with(&format!("{canary} {}", "failure detail ".repeat(900)));
    let (outcome, _) = search_with_responses(&[canary], 2, vec![Response::json(401, &body)]);
    let error = outcome.expect_err("a refused key fails");

    assert_eq!(
        (
            error.kind,
            error.attempts.len(),
            error.message.contains(canary),
            error
                .attempts
                .iter()
                .any(|attempt| attempt.message.contains(canary)),
        ),
        (AttemptErrorKind::Auth, 1, false, false)
    );
}

#[test]
fn a_refused_request_reports_its_reason_with_the_urls_redacted() {
    let body = error_with(
        "upstream rejected https://user:password@example.test/private?api_key=response-secret",
    );
    let (outcome, _) = search_with_responses(&[KEY], 1, vec![Response::json(400, &body)]);
    let error = outcome.expect_err("a refused request fails");

    assert_eq!(
        (error.kind, error.message),
        (
            AttemptErrorKind::Parameter,
            "upstream rejected https://example.test/private?api_key=********".to_owned()
        )
    );
}

#[test]
fn a_rate_limited_or_exhausted_key_rotates_to_the_next_key() {
    let keys = [KEY, SECOND_KEY];
    let rotations = [
        ("rate limited", AttemptErrorKind::RateLimited),
        ("monthly quota exhausted", AttemptErrorKind::QuotaExhausted),
    ]
    .map(|(reason, kind)| {
        let (outcome, requests) = search_with_responses(
            &keys,
            1,
            vec![
                Response::json(429, &error_with(reason)),
                Response::json(200, &no_results()),
            ],
        );
        let attempts = outcome.expect("the next key succeeds").attempts;
        // The credential cursor persists across searches, so the first key may be either.
        let first = attempts[0].credential_index;
        let second = (first + 1) % keys.len();
        (
            attempts
                .iter()
                .map(|attempt| {
                    (
                        attempt.error_kind,
                        attempt.rotation_count,
                        attempt.anonymous,
                    )
                })
                .collect::<Vec<_>>(),
            keys_sent(&requests),
            vec![(Some(kind), 0, false), (None, 1, false)],
            vec![Some(keys[first].to_owned()), Some(keys[second].to_owned())],
        )
    });

    for (attempts, sent, expected_attempts, expected_keys) in rotations {
        assert_eq!((attempts, sent), (expected_attempts, expected_keys));
    }
}

#[test]
fn a_rate_limited_similar_rotates_and_names_its_operation() {
    let keys = [KEY, SECOND_KEY];
    let fixture = Fixture::start_sequence(vec![
        Response::json(429, &stored("error.json")),
        Response::json(200, &stored("similar.json")),
    ]);
    let outcome = block_on(
        route(&fixture.url, &keys, 1, 30, Duration::from_secs(10)).similar(similar_request()),
    );
    let requests = fixture.finish_all();
    let attempts = outcome.expect("the next key succeeds").attempts;
    let first = attempts[0].credential_index;

    assert_eq!(
        (
            attempts
                .iter()
                .map(|attempt| (attempt.target, attempt.error_kind))
                .collect::<Vec<_>>(),
            keys_sent(&requests),
        ),
        (
            vec![
                (
                    AttemptTarget::operation("similar"),
                    Some(AttemptErrorKind::RateLimited)
                ),
                (AttemptTarget::operation("similar"), None),
            ],
            vec![
                Some(keys[first].to_owned()),
                Some(keys[(first + 1) % keys.len()].to_owned()),
            ],
        )
    );
}

#[test]
fn a_search_rate_limited_on_every_key_fails_after_trying_each_once() {
    let keys = [KEY, SECOND_KEY, "third-key"];
    let (outcome, requests) = search_with_responses(
        &keys,
        2,
        (0..keys.len())
            .map(|_| Response::json(429, &stored("error.json")))
            .collect(),
    );
    let error = outcome.expect_err("every key is rate limited");
    let mut sent = keys_sent(&requests);
    sent.sort();

    assert_eq!(
        (error.kind, error.attempts.len(), sent),
        (
            AttemptErrorKind::RateLimited,
            keys.len(),
            vec![
                Some(KEY.to_owned()),
                Some(SECOND_KEY.to_owned()),
                Some("third-key".to_owned()),
            ],
        )
    );
}

#[test]
fn a_server_failure_is_retried_with_the_same_key() {
    let (outcome, requests) = search_with_responses(
        &[KEY],
        2,
        vec![
            Response::json(503, &error_with("temporarily unavailable")),
            Response::json(200, &no_results()),
        ],
    );

    assert_eq!(
        (
            outcome
                .expect("the retry succeeds")
                .attempts
                .iter()
                .map(|attempt| (
                    attempt.error_kind,
                    attempt.retry_count,
                    attempt.rotation_count
                ))
                .collect::<Vec<_>>(),
            keys_sent(&requests),
        ),
        (
            vec![(Some(AttemptErrorKind::Network), 0, 0), (None, 1, 0)],
            vec![Some(KEY.to_owned()), Some(KEY.to_owned())],
        )
    );
}

#[test]
fn each_attempt_ends_at_the_configured_timeout_inside_a_longer_deadline() {
    let fixture = Fixture::start_unanswered();
    let url = fixture.url.clone();

    let error = advance_after_request(&fixture, Duration::from_secs(1), async move {
        route(&url, &[KEY], 1, 1, Duration::from_secs(10))
            .search(search_request("timeout"))
            .await
    })
    .expect_err("the unanswered attempt times out");
    fixture.finish();

    assert_eq!(
        (
            error.kind,
            error
                .attempts
                .iter()
                .map(|attempt| (attempt.error_kind, attempt.duration_ms))
                .collect::<Vec<_>>(),
        ),
        (
            AttemptErrorKind::Timeout,
            vec![(Some(AttemptErrorKind::Timeout), 1_000)]
        )
    );
}

#[test]
fn an_attempt_ends_at_a_command_deadline_shorter_than_its_timeout() {
    let fixture = Fixture::start_unanswered();
    let url = fixture.url.clone();

    let error = advance_after_request(&fixture, Duration::from_secs(1), async move {
        route(&url, &[KEY], 2, 30, Duration::from_secs(1))
            .search(search_request("timeout"))
            .await
    })
    .expect_err("the command deadline ends the attempt");
    fixture.finish();

    assert_eq!(
        (
            error.kind,
            error
                .attempts
                .iter()
                .map(|attempt| (attempt.error_kind, attempt.duration_ms))
                .collect::<Vec<_>>(),
        ),
        (
            AttemptErrorKind::Timeout,
            vec![(Some(AttemptErrorKind::Timeout), 1_000)]
        )
    );
}

#[test]
fn separately_built_routes_draw_search_and_similar_keys_from_one_cursor() {
    let keys = [KEY, SECOND_KEY];
    let fixture = Fixture::start_sequence(vec![
        Response::json(200, &no_results()),
        Response::json(200, &stored("similar.json")),
    ]);

    let attempts = block_on(async {
        let search = route(&fixture.url, &keys, 1, 30, Duration::from_secs(10))
            .search(search_request("first invocation"))
            .await
            .expect("search succeeds");
        let similar = route(&fixture.url, &keys, 1, 30, Duration::from_secs(10))
            .similar(similar_request())
            .await
            .expect("similar succeeds");
        (search.attempts, similar.attempts)
    });
    let requests = fixture.finish_all();
    let first = attempts.0[0].credential_index;

    assert_eq!(
        (
            requests
                .iter()
                .map(|request| path(request))
                .collect::<Vec<_>>(),
            keys_sent(&requests),
        ),
        (
            vec!["/search".to_owned(), "/findSimilar".to_owned()],
            vec![
                Some(keys[first].to_owned()),
                Some(keys[(first + 1) % keys.len()].to_owned()),
            ],
        )
    );
}

/// The documentation search route as the provider factory builds it, on the fixture at `url`.
fn docs_route(url: &str) -> Box<dyn DocsSearch> {
    build_docs_search(
        DocsSearchProviderConfig::Exa(config(url, &[KEY], 30)),
        build_client(true).expect("build client"),
        RetryPolicy::new(1, 1.0, Duration::ZERO),
        Deadline::new(Duration::from_secs(10)),
    )
}

#[test]
fn a_documentation_search_asks_for_highlights_of_automatic_results() {
    let fixture = Fixture::start_json(200, &stored("search.json"));

    block_on(docs_route(&fixture.url).search("Rust ownership", 1))
        .expect("documentation search succeeds");
    let sent = fixture.finish();

    assert_eq!(
        (path(&sent), request_json(&sent)),
        (
            "/search".to_owned(),
            json!({
                "query": "Rust ownership",
                "numResults": 1,
                "type": "auto",
                "contents": {"text": false, "highlights": true}
            }),
        )
    );
}

#[test]
fn a_documentation_result_becomes_a_candidate_summarized_by_its_first_highlight() {
    let fixture = Fixture::start_json(200, &stored("search.json"));

    let outcome = block_on(docs_route(&fixture.url).search("Rust ownership", 1))
        .expect("documentation search succeeds");
    fixture.finish();

    assert_eq!(
        (
            serde_json::to_value(&outcome.candidate_sources).expect("encode candidates"),
            outcome
                .attempts
                .iter()
                .map(|attempt| (attempt.provider, attempt.target))
                .collect::<Vec<_>>(),
        ),
        (
            json!([{
                "provider": "exa",
                "capability": "docs_search",
                "title": "Fixture result",
                "url": "https://example.test/result?api_key=********&safe=1",
                "summary": "Fixture highlight ********",
                "provider_data": {
                    "id": "https://example.test/result",
                    "highlights": ["Fixture highlight ********", "Secondary detail"],
                    "published_date": "2026-07-25T00:00:00.000Z",
                    "author": "Example",
                    "image": "https://example.test/image.png",
                    "favicon": "https://example.test/favicon.ico"
                }
            }]),
            vec![("exa", AttemptTarget::seam("docs_search"))],
        )
    );
}
