use std::time::Duration;

use serde_json::Value;

use super::decode_sources;
use crate::config::WebFetchProviderConfig;
use crate::credentials::CredentialPool;
use crate::fixture_server::{Fixture, Response, block_on};
use crate::net::{RetryPolicy, build_client};
use crate::providers::{ProviderId, build_web_search};
use crate::samples::sample;
use crate::types::{AttemptErrorKind, Deadline, ProviderError, SupplementalSearchOutcome};

const KEY: &str = "first-key";

fn stored(name: &str) -> Value {
    serde_json::from_str(&sample(name)).expect("stored JSON")
}

/// Decodes `body` as a successful answer of `provider` with a limit of `limit` sources, each
/// source as JSON, and a failure as its kind and message.
fn decode(
    provider: ProviderId,
    body: &str,
    limit: u16,
) -> Result<Value, (AttemptErrorKind, String)> {
    decode_sources(
        provider,
        body,
        limit,
        200,
        &CredentialPool::new(provider.name(), vec![KEY.into()]),
    )
    .map(|sources| serde_json::to_value(sources).expect("encode sources"))
    .map_err(|failure| (failure.kind, failure.message))
}

/// Searches `query` with a limit of `limit` through the route of `provider`, built as the
/// provider factory builds it, on a fixture answering `response`; returns the outcome and the
/// request the fixture received.
fn search(
    provider: ProviderId,
    query: &str,
    limit: u16,
    response: Response,
) -> (Result<SupplementalSearchOutcome, ProviderError>, String) {
    let fixture = Fixture::start_sequence(vec![response]);
    let route = build_web_search(
        provider,
        WebFetchProviderConfig {
            url: fixture.url.clone(),
            keys: vec![KEY.into()],
            timeout_seconds: 30,
            respond_with: String::new(),
        },
        build_client(true).expect("build client"),
        RetryPolicy::new(1, 1.0, Duration::ZERO),
        Deadline::new(Duration::from_secs(10)),
    );
    let outcome = block_on(route.search(query, limit));
    (outcome, fixture.finish())
}

mod tavily {
    use serde_json::{Value, json};

    use super::{KEY, decode, search, stored};
    use crate::fixture_server::{Response, header, path, request_json};
    use crate::providers::ProviderId;
    use crate::types::AttemptErrorKind;

    fn stored_with_results(results: Value) -> String {
        let mut body = stored("tavily/search.json");
        body["results"] = results;
        body.to_string()
    }

    #[test]
    fn a_search_answer_decodes_each_result_with_its_content_as_text() {
        assert_eq!(
            decode(
                ProviderId::Tavily,
                &stored("tavily/search.json").to_string(),
                5
            ),
            Ok(json!([{
                "title": "Supplemental",
                "url": "https://example.test/extra",
                "text": "Provider-native summary of the page."
            }]))
        );
    }

    #[test]
    fn results_without_an_http_url_are_skipped_and_kept_urls_are_trimmed_and_redacted() {
        let base = stored("tavily/search.json")["results"][0].clone();
        let with_url = |url: Value| {
            let mut result = base.clone();
            result["url"] = url;
            result
        };
        let mut missing = base.clone();
        missing
            .as_object_mut()
            .expect("stored result")
            .remove("url");
        let body = stored_with_results(json!([
            missing,
            with_url(Value::Null),
            with_url(json!("   ")),
            with_url(json!("ftp://example.test/result")),
            with_url(json!("https://example.test/source?token=canary-secret")),
            with_url(json!("  https://example.test/extra  ")),
        ]));

        let urls = decode(ProviderId::Tavily, &body, 20).map(|sources| {
            sources
                .as_array()
                .expect("sources")
                .iter()
                .map(|source| source["url"].clone())
                .collect::<Vec<_>>()
        });

        assert_eq!(
            urls,
            Ok(vec![
                json!("https://example.test/source?token=********"),
                json!("https://example.test/extra"),
            ])
        );
    }

    #[test]
    fn the_limit_caps_the_decoded_sources() {
        let result = stored("tavily/search.json")["results"][0].clone();
        let body = stored_with_results(json!([result.clone(), result.clone(), result]));

        let count =
            decode(ProviderId::Tavily, &body, 2).map(|sources| sources.as_array().map(Vec::len));

        assert_eq!(count, Ok(Some(2)));
    }

    #[test]
    fn an_answer_without_results_is_an_empty_success() {
        assert_eq!(
            decode(ProviderId::Tavily, &stored_with_results(json!([])), 5),
            Ok(json!([]))
        );
    }

    #[test]
    fn an_answer_whose_results_all_lack_an_http_url_is_an_evidence_failure() {
        let body = stored_with_results(json!([{"url": null}, {"title": "missing"}, {"url": "  "}]));

        assert_eq!(
            decode(ProviderId::Tavily, &body, 5),
            Err((
                AttemptErrorKind::Evidence,
                "tavily search returned no valid candidate".to_owned()
            ))
        );
    }

    #[test]
    fn malformed_answers_are_runtime_failures() {
        for body in [
            r"{}",
            r#"{"results":{}}"#,
            r#"{"results":[{"url":42}]}"#,
            r#"{"results":["not an object"]}"#,
        ] {
            let failure = decode(ProviderId::Tavily, body, 5).expect_err("the answer is refused");

            assert_eq!(
                (
                    failure.0,
                    failure.1.starts_with("invalid tavily search response: ")
                ),
                (AttemptErrorKind::Runtime, true),
                "{body}: {}",
                failure.1
            );
        }
    }

    #[test]
    fn a_search_posts_the_query_for_advanced_results_with_the_key_as_a_bearer_token() {
        let (outcome, request) = search(
            ProviderId::Tavily,
            "Current information",
            20,
            Response::json(200, &stored("tavily/search.json").to_string()),
        );

        assert_eq!(
            (
                outcome.map(|outcome| outcome.sources.len()).ok(),
                request.split(' ').next(),
                path(&request),
                header(&request, "authorization"),
                request_json(&request),
            ),
            (
                Some(1),
                Some("POST"),
                "/search".to_owned(),
                Some(format!("Bearer {KEY}")),
                json!({
                    "query": "Current information",
                    "max_results": 20,
                    "search_depth": "advanced",
                    "chunks_per_source": 1,
                    "include_raw_content": false,
                    "include_answer": false
                }),
            )
        );
    }

    #[test]
    fn a_refusal_names_its_urls_without_credentials_or_secrets() {
        let mut body = stored("tavily/error.json");
        body["message"] = json!(
            "upstream rejected https://user:password@example.test/private?token=response-secret"
        );

        let (outcome, _) = search(
            ProviderId::Tavily,
            "Current information",
            3,
            Response::json(400, &body.to_string()),
        );
        let error = outcome.expect_err("the refusal fails the search");

        assert_eq!(
            error
                .attempts
                .iter()
                .map(|attempt| attempt.message.as_str())
                .collect::<Vec<_>>(),
            vec![r#"{"message":"upstream rejected https://example.test/private?token=********"}"#]
        );
    }
}

mod firecrawl {
    use serde_json::json;

    use super::{KEY, decode, search, stored};
    use crate::fixture_server::{Response, header, path, request_json};
    use crate::providers::ProviderId;
    use crate::types::AttemptErrorKind;

    #[test]
    fn a_search_answer_decodes_each_web_result_with_its_description_as_text() {
        assert_eq!(
            decode(
                ProviderId::Firecrawl,
                &stored("firecrawl/search.json").to_string(),
                5
            ),
            Ok(json!([{
                "title": "Fallback",
                "url": "https://example.test/fallback",
                "published_date": "2026-08-06",
                "author": "Reporter",
                "text": "Firecrawl summary"
            }]))
        );
    }

    #[test]
    fn an_answer_without_web_results_is_an_empty_success() {
        let mut body = stored("firecrawl/search.json");
        body["data"]["web"] = json!([]);

        assert_eq!(
            decode(ProviderId::Firecrawl, &body.to_string(), 5),
            Ok(json!([]))
        );
    }

    #[test]
    fn malformed_containers_are_runtime_failures() {
        for body in [
            r"{}",
            r#"{"data":[]}"#,
            r#"{"data":{}}"#,
            r#"{"data":{"web":{}}}"#,
        ] {
            let failure =
                decode(ProviderId::Firecrawl, body, 5).expect_err("the answer is refused");

            assert_eq!(
                (
                    failure.0,
                    failure.1.starts_with("invalid firecrawl search response: ")
                ),
                (AttemptErrorKind::Runtime, true),
                "{body}: {}",
                failure.1
            );
        }
    }

    #[test]
    fn a_search_posts_the_query_and_limit_with_the_key_as_a_bearer_token() {
        let (outcome, request) = search(
            ProviderId::Firecrawl,
            "Current information",
            3,
            Response::json(200, &stored("firecrawl/search.json").to_string()),
        );

        assert_eq!(
            (
                outcome.map(|outcome| outcome.sources.len()).ok(),
                request.split(' ').next(),
                path(&request),
                header(&request, "authorization"),
                request_json(&request),
            ),
            (
                Some(1),
                Some("POST"),
                "/search".to_owned(),
                Some(format!("Bearer {KEY}")),
                json!({"query": "Current information", "limit": 3}),
            )
        );
    }
}
