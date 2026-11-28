use std::time::Duration;

use super::{FetchRequest, ProviderFetchOutcome, Service, WebFetch};
use crate::config::WebFetchProviderConfig;
use crate::fixture_server::{Fixture, Response, block_on, header};
use crate::net::{RetryPolicy, build_client};
use crate::providers::{ProviderId, build_web_fetch};
use crate::types::{AttemptErrorKind, Deadline, ProviderError};
use crate::web_samples::stored_with;

const ARTICLE: &str = "https://example.test/article";
const KEY: &str = "first-key";
const SECOND_KEY: &str = "second-key";

fn config(url: &str, keys: &[&str], timeout_seconds: u64) -> WebFetchProviderConfig {
    WebFetchProviderConfig {
        url: url.to_owned(),
        keys: keys.iter().map(|key| (*key).into()).collect(),
        timeout_seconds,
        respond_with: String::new(),
    }
}

/// The route as the provider factory builds it, with `attempts` attempts per request and a
/// command deadline of `deadline`.
fn route(
    id: ProviderId,
    config: WebFetchProviderConfig,
    attempts: usize,
    deadline: Duration,
) -> Box<dyn WebFetch> {
    build_web_fetch(
        id,
        config,
        build_client(true).expect("build client"),
        RetryPolicy::new(attempts, 1.0, Duration::ZERO),
        Deadline::new(deadline),
    )
}

fn request(url: &str) -> FetchRequest {
    FetchRequest { url: url.into() }
}

/// Fetches the article with `id` on a fixture answering `responses` in turn.
fn fetch_with_responses(
    id: ProviderId,
    keys: &[&str],
    attempts: usize,
    responses: Vec<Response>,
) -> (Result<ProviderFetchOutcome, ProviderError>, Vec<String>) {
    let fixture = Fixture::start_sequence(responses);
    let route = route(
        id,
        config(&fixture.url, keys, 30),
        attempts,
        Duration::from_secs(10),
    );
    let outcome = block_on(route.fetch(&request(ARTICLE)));
    (outcome, fixture.finish_all())
}

fn authorizations(requests: &[String]) -> Vec<Option<String>> {
    requests
        .iter()
        .map(|request| header(request, "authorization"))
        .collect()
}

/// Each attempt as its error kind and credential rotations, in order.
fn rotations(outcome: &ProviderFetchOutcome) -> Vec<(Option<AttemptErrorKind>, usize)> {
    outcome
        .attempts
        .iter()
        .map(|attempt| (attempt.error_kind, attempt.rotation_count))
        .collect()
}

/// The bearer tokens of two keys in the order a rotating fetch sends them. The credential cursor
/// persists across fetches, so the first key may be either.
fn rotated_bearers(outcome: &ProviderFetchOutcome) -> Vec<Option<String>> {
    let keys = [KEY, SECOND_KEY];
    let first = outcome.attempts[0].credential_index;
    [first, (first + 1) % keys.len()]
        .map(|index| Some(format!("Bearer {}", keys[index])))
        .into()
}

mod firecrawl {
    use serde_json::json;

    use super::{
        ARTICLE, KEY, SECOND_KEY, Service, authorizations, fetch_with_responses, rotated_bearers,
        rotations, stored_with,
    };
    use crate::fixture_server::{Response, header, path, request_json};
    use crate::providers::ProviderId;
    use crate::samples::sample;
    use crate::types::AttemptErrorKind;

    #[test]
    fn a_scrape_answer_decodes_to_its_markdown() {
        assert_eq!(
            Service::Firecrawl.content(&sample("firecrawl/scrape.json"), false),
            Ok("# Dark Matter Halos & Galaxies\n\n## Introduction\n\nPaper body text.".to_owned())
        );
    }

    #[test]
    fn a_scrape_posts_the_page_for_its_main_markdown_with_the_key_as_a_bearer_token() {
        let (outcome, requests) = fetch_with_responses(
            ProviderId::Firecrawl,
            &[KEY],
            1,
            vec![Response::json(200, &sample("firecrawl/scrape.json"))],
        );
        let sent = &requests[0];

        assert_eq!(
            (
                outcome.ok().map(|outcome| outcome.provider),
                sent.split(' ').next(),
                path(sent),
                header(sent, "authorization"),
                request_json(sent),
            ),
            (
                Some("firecrawl"),
                Some("POST"),
                "/scrape".to_owned(),
                Some(format!("Bearer {KEY}")),
                json!({
                    "url": ARTICLE,
                    "formats": ["markdown"],
                    "onlyMainContent": true,
                    "timeout": 60000,
                    "parsers": [{"type": "pdf", "mode": "auto", "pageMarkers": true}]
                }),
            )
        );
    }

    #[test]
    fn an_unsupported_site_is_a_parameter_failure_with_firecrawls_reason() {
        let (outcome, _) = fetch_with_responses(
            ProviderId::Firecrawl,
            &[KEY],
            1,
            vec![Response::json(403, &sample("firecrawl/error.json"))],
        );
        let error = outcome.err().expect("a refused site fails");

        assert_eq!(
            (error.kind, error.message),
            (
                AttemptErrorKind::Parameter,
                "We apologize for the inconvenience but we do not support this site.".to_owned()
            )
        );
    }

    #[test]
    fn a_key_without_payment_rotates_to_the_next_key() {
        let rich = "Rotated Firecrawl content. ".repeat(30);
        let (outcome, requests) = fetch_with_responses(
            ProviderId::Firecrawl,
            &[KEY, SECOND_KEY],
            1,
            vec![
                Response::json(402, &sample("firecrawl/error.json")),
                Response::json(
                    200,
                    &stored_with("firecrawl/scrape.json", &[("/data/markdown", &rich)]),
                ),
            ],
        );
        let outcome = outcome.expect("the next key succeeds");

        assert_eq!(
            (rotations(&outcome), authorizations(&requests)),
            (
                vec![(Some(AttemptErrorKind::QuotaExhausted), 0), (None, 1)],
                rotated_bearers(&outcome),
            )
        );
    }
}

mod jina {
    use std::time::Duration;

    use super::{ARTICLE, KEY, config, fetch_with_responses, request, route, stored_with};
    use crate::fixture_server::{
        Fixture, Response, Route, advance_after_request, block_on, header,
    };
    use crate::net::{CONTENT_TRUNCATED_DIAGNOSTIC, MAX_RESPONSE_BYTES};
    use crate::providers::ProviderId;
    use crate::types::AttemptErrorKind;

    fn read(content: &str) -> String {
        stored_with("jina/read.json", &[("/data/content", content)])
    }

    /// The stored Reader error with `message` as its reason.
    fn refusal(message: &str) -> String {
        stored_with(
            "jina/error.json",
            &[("/message", message), ("/readableMessage", message)],
        )
    }

    fn article() -> String {
        read(&"Anonymous fallback content. ".repeat(30))
    }

    /// Each attempt as its error kind and whether it sent no credential, in order.
    fn anonymity(
        attempts: &[crate::types::ProviderAttempt],
    ) -> Vec<(Option<AttemptErrorKind>, bool)> {
        attempts
            .iter()
            .map(|attempt| (attempt.error_kind, attempt.anonymous))
            .collect()
    }

    #[test]
    fn a_read_sends_the_target_in_its_path_with_the_reader_headers_and_keeps_the_content() {
        let source = "https://example.test/source?token=source-secret#fragment";
        let content = format!(
            "{}\n{source}",
            "A substantial fixture paragraph. ".repeat(20)
        );
        let fixture = Fixture::start_json(200, &read(&content));
        let route = route(
            ProviderId::Jina,
            crate::config::WebFetchProviderConfig {
                respond_with: "readerlm-v2".into(),
                ..config(&fixture.url, &[KEY], 30)
            },
            1,
            Duration::from_secs(10),
        );

        let outcome = block_on(route.fetch(&request(source))).expect("the read succeeds");
        let sent = fixture.finish();

        assert_eq!(
            (
                outcome.content,
                outcome.attempts[0].target.seam_name(),
                sent.lines().next().map(str::to_owned),
                [
                    "authorization",
                    "x-return-format",
                    "accept",
                    "x-respond-with",
                    "x-retain-links",
                    "x-target-selector",
                    "x-remove-selector",
                ]
                .map(|name| header(&sent, name)),
            ),
            (
                content,
                Some("web_fetch"),
                Some("GET /https://example.test/source?token=source-secret HTTP/1.1".to_owned()),
                [
                    Some(format!("Bearer {KEY}")),
                    Some("markdown".to_owned()),
                    Some("application/json".to_owned()),
                    Some("readerlm-v2".to_owned()),
                    None,
                    None,
                    None,
                ],
            )
        );
    }

    #[test]
    fn a_fetch_target_naming_a_remote_site_reaches_the_loopback_fixture() {
        let fixture = Fixture::start_routed(vec![Route::new(
            "GET",
            "/https://example.test/article",
            Response::json(200, &read("fixture article")),
        )]);
        let route = route(
            ProviderId::Jina,
            config(&fixture.url, &[], 5),
            1,
            Duration::from_secs(10),
        );

        let outcome = block_on(route.fetch(&request(ARTICLE)));
        fixture.finish_all();

        assert_eq!(
            outcome.ok().map(|outcome| outcome.content),
            Some("fixture article".to_owned())
        );
    }

    #[test]
    fn a_reader_answer_without_data_is_a_runtime_failure() {
        let mut body: serde_json::Value =
            serde_json::from_str(&read("unused")).expect("stored Reader JSON");
        body["data"] = serde_json::Value::Null;
        let (outcome, _) = fetch_with_responses(
            ProviderId::Jina,
            &[KEY],
            1,
            vec![Response::json(200, &body.to_string())],
        );
        let error = outcome.err().expect("a body without data fails");

        assert_eq!(
            (
                error.kind,
                anonymity(&error.attempts),
                error.message.starts_with("invalid Jina response"),
            ),
            (
                AttemptErrorKind::Runtime,
                vec![(Some(AttemptErrorKind::Runtime), false)],
                true,
            )
        );
    }

    #[test]
    fn content_beyond_the_response_cap_is_cut_on_a_character_boundary_with_a_diagnostic() {
        let prefix = r#"{"data":{"content":""#;
        let retained = "a".repeat(MAX_RESPONSE_BYTES - prefix.len() - 1);
        let body = format!("{prefix}{retained}€unreachable suffix");

        let (outcome, _) = fetch_with_responses(
            ProviderId::Jina,
            &[KEY],
            1,
            vec![Response::json(200, &body)],
        );
        let outcome = outcome.expect("the truncated read succeeds");

        assert_eq!(
            (outcome.content == retained, outcome.diagnostic.as_deref()),
            (true, Some(CONTENT_TRUNCATED_DIAGNOSTIC))
        );
    }

    #[test]
    fn without_keys_the_page_is_read_anonymously() {
        let (outcome, requests) = fetch_with_responses(
            ProviderId::Jina,
            &[],
            1,
            vec![Response::json(200, &article())],
        );
        let outcome = outcome.expect("the anonymous read succeeds");

        assert_eq!(
            (
                anonymity(&outcome.attempts),
                ["authorization", "x-return-format", "accept"]
                    .map(|name| header(&requests[0], name)),
            ),
            (
                vec![(None, true)],
                [
                    None,
                    Some("markdown".to_owned()),
                    Some("application/json".to_owned()),
                ],
            )
        );
    }

    #[test]
    fn a_key_failure_is_followed_by_one_anonymous_request() {
        for (status, kind) in [
            (401, AttemptErrorKind::Auth),
            (402, AttemptErrorKind::QuotaExhausted),
            (429, AttemptErrorKind::RateLimited),
        ] {
            let (outcome, requests) = fetch_with_responses(
                ProviderId::Jina,
                &[KEY],
                1,
                vec![
                    Response::json(status, &refusal("key refused")),
                    Response::json(200, &article()),
                ],
            );

            assert_eq!(
                (
                    outcome.ok().map(|outcome| anonymity(&outcome.attempts)),
                    requests
                        .iter()
                        .map(|request| header(request, "authorization"))
                        .collect::<Vec<_>>(),
                ),
                (
                    Some(vec![(Some(kind), false), (None, true)]),
                    vec![Some(format!("Bearer {KEY}")), None],
                ),
                "status {status}"
            );
        }
    }

    #[test]
    fn a_failure_unrelated_to_the_key_sends_no_anonymous_request() {
        for (status, kind) in [
            (503, AttemptErrorKind::Network),
            (408, AttemptErrorKind::Timeout),
        ] {
            let (outcome, requests) = fetch_with_responses(
                ProviderId::Jina,
                &[KEY],
                1,
                vec![Response::json(status, &refusal("unavailable"))],
            );
            let error = outcome.err().expect("the keyed request fails");

            assert_eq!(
                (error.kind, anonymity(&error.attempts), requests.len()),
                (kind, vec![(Some(kind), false)], 1),
                "status {status}"
            );
        }
    }

    #[test]
    fn an_anonymous_refusal_ends_the_read_without_retrying() {
        for (status, body, kind, reason) in [
            (
                403,
                crate::samples::sample("jina/error.json"),
                AttemptErrorKind::Parameter,
                "Anonymous access to domain x.com blocked until Sun Oct 18 2026",
            ),
            (
                429,
                refusal("anonymous limit reached"),
                AttemptErrorKind::RateLimited,
                "anonymous limit reached",
            ),
        ] {
            // Retries are allowed, so a single request shows the refusal is not retried.
            let (outcome, requests) = fetch_with_responses(
                ProviderId::Jina,
                &[],
                3,
                vec![Response::json(status, &body)],
            );
            let error = outcome.err().expect("the anonymous request is refused");

            assert_eq!(
                (
                    error.kind,
                    anonymity(&error.attempts),
                    error.message.contains(reason),
                    requests.len(),
                ),
                (kind, vec![(Some(kind), true)], true, 1),
                "status {status}: {}",
                error.message
            );
        }
    }

    #[test]
    fn each_attempt_ends_at_the_configured_timeout_inside_a_longer_deadline() {
        let fixture = Fixture::start_unanswered();
        let url = fixture.url.clone();

        let error = advance_after_request(&fixture, Duration::from_secs(2), async move {
            route(
                ProviderId::Jina,
                config(&url, &[KEY], 2),
                1,
                Duration::from_secs(10),
            )
            .fetch(&request(ARTICLE))
            .await
        })
        .err()
        .expect("the unanswered attempt times out");
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
                vec![(Some(AttemptErrorKind::Timeout), 2_000)]
            )
        );
    }
}

mod tavily {
    use serde_json::json;

    use super::{
        ARTICLE, KEY, SECOND_KEY, Service, authorizations, fetch_with_responses, rotated_bearers,
        rotations, stored_with,
    };
    use crate::fixture_server::{Response, header, path, request_json};
    use crate::net::{CONTENT_TRUNCATED_DIAGNOSTIC, MAX_RESPONSE_BYTES};
    use crate::providers::ProviderId;
    use crate::samples::sample;
    use crate::types::AttemptErrorKind;

    fn extract(content: &str) -> String {
        stored_with(
            "tavily/extract.json",
            &[("/results/0/raw_content", content)],
        )
    }

    #[test]
    fn an_extract_answer_decodes_to_the_raw_content_of_its_first_result() {
        assert_eq!(
            Service::Tavily.content(&sample("tavily/extract.json"), false),
            Ok("# Example Article\n\nArticle body text.".to_owned())
        );
    }

    #[test]
    fn an_extract_posts_the_page_for_basic_markdown_with_the_key_as_a_bearer_token() {
        let (outcome, requests) = fetch_with_responses(
            ProviderId::Tavily,
            &[KEY],
            1,
            vec![Response::json(200, &sample("tavily/extract.json"))],
        );
        let sent = &requests[0];

        assert_eq!(
            (
                outcome
                    .ok()
                    .map(|outcome| (outcome.provider, outcome.content)),
                sent.split(' ').next(),
                path(sent),
                header(sent, "authorization"),
                request_json(sent),
            ),
            (
                Some((
                    "tavily",
                    "# Example Article\n\nArticle body text.".to_owned()
                )),
                Some("POST"),
                "/extract".to_owned(),
                Some(format!("Bearer {KEY}")),
                json!({
                    "urls": [ARTICLE],
                    "format": "markdown",
                    "extract_depth": "basic"
                }),
            )
        );
    }

    #[test]
    fn a_truncated_answer_keeps_the_content_read_before_the_cut_with_a_diagnostic() {
        let prefix = r#"{"results":[{"raw_content":""#;
        let body = format!("{prefix}{}", "t".repeat(MAX_RESPONSE_BYTES));

        let (outcome, _) = fetch_with_responses(
            ProviderId::Tavily,
            &[KEY],
            1,
            vec![Response::json(200, &body)],
        );
        let outcome = outcome.expect("the truncated extract succeeds");

        assert_eq!(
            (outcome.content.len(), outcome.diagnostic.as_deref()),
            (
                MAX_RESPONSE_BYTES - prefix.len(),
                Some(CONTENT_TRUNCATED_DIAGNOSTIC)
            )
        );
    }

    #[test]
    fn a_rate_limited_key_rotates_to_the_next_key() {
        let (outcome, requests) = fetch_with_responses(
            ProviderId::Tavily,
            &[KEY, SECOND_KEY],
            1,
            vec![
                Response::json(429, &sample("tavily/error.json")),
                Response::json(200, &extract(&"Rotated Tavily content. ".repeat(30))),
            ],
        );
        let outcome = outcome.expect("the next key succeeds");

        assert_eq!(
            (rotations(&outcome), authorizations(&requests)),
            (
                vec![(Some(AttemptErrorKind::RateLimited), 0), (None, 1)],
                rotated_bearers(&outcome),
            )
        );
    }

    #[test]
    fn a_server_failure_after_rotation_ends_the_fetch_as_a_network_failure() {
        let (outcome, _) = fetch_with_responses(
            ProviderId::Tavily,
            &[KEY, SECOND_KEY],
            1,
            vec![
                Response::json(429, &sample("tavily/error.json")),
                Response::json(500, &sample("tavily/error.json")),
            ],
        );
        let error = outcome.err().expect("the rotated key fails too");

        assert_eq!(
            (
                error.kind,
                error
                    .attempts
                    .iter()
                    .map(|attempt| attempt.error_kind)
                    .collect::<Vec<_>>(),
            ),
            (
                AttemptErrorKind::Network,
                vec![
                    Some(AttemptErrorKind::RateLimited),
                    Some(AttemptErrorKind::Network)
                ],
            )
        );
    }
}
