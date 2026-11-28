use std::time::Duration;

use serde_json::{Value, json};

use super::Openai;
use crate::config::{MainSearchProviderConfig, OpenaiRuntimeConfig};
use crate::fixture_server::{Fixture, Response, block_on, header, path, request_json};
use crate::main_search_samples::{completed_events, failed_event, stored_answer};
use crate::net::{RetryPolicy, build_client};
use crate::providers::constructors::build_openai;
use crate::providers::{MainSearchRequest, build_main_search};
use crate::types::{AttemptErrorKind, Deadline, ProviderError, SearchOutcome};

const KEY: &str = "openai-key";

fn config(url: &str) -> OpenaiRuntimeConfig {
    OpenaiRuntimeConfig {
        url: format!("{url}/v1"),
        keys: vec![KEY.into()],
        model: "openai-model".to_owned(),
    }
}

fn route(url: &str) -> Openai {
    build_openai(
        config(url),
        build_client(true).expect("build client"),
        RetryPolicy::new(1, 1.0, Duration::ZERO),
        Deadline::new(Duration::from_secs(10)),
    )
}

/// Runs one main search through OpenAI as the provider factory builds it, against a fixture
/// that answers with `body`.
fn search(body: &str) -> (Result<SearchOutcome, ProviderError>, String) {
    let fixture = Fixture::start_sequence(vec![Response::sse(200, body)]);
    let main_search = build_main_search(
        MainSearchProviderConfig::Openai(config(&fixture.url)),
        build_client(true).expect("build client"),
        RetryPolicy::new(1, 1.0, Duration::ZERO),
        Deadline::new(Duration::from_secs(10)),
    );
    let outcome = block_on(main_search.search(MainSearchRequest {
        query: "What changed?".into(),
        model: None,
    }));
    (outcome, fixture.finish())
}

fn sources(response: &Value) -> Vec<(String, String)> {
    let (_, sources) = route("http://127.0.0.1:9")
        .main_search()
        .decode(response)
        .expect("the answer decodes");
    sources
        .into_iter()
        .map(|source| (source.title, source.url))
        .collect()
}

#[test]
fn a_search_sends_the_web_search_tool_to_responses_with_the_configured_model() {
    let (outcome, request) = search(&completed_events(&stored_answer("openai")));
    let outcome = outcome.expect("openai answers");
    let body = request_json(&request);

    assert_eq!(
        (
            outcome.provider,
            request.split_whitespace().next(),
            path(&request),
            header(&request, "authorization"),
            &body["model"],
            &body["stream"],
            &body["tools"],
            body["instructions"].is_string(),
        ),
        (
            "openai",
            Some("POST"),
            "/v1/responses".to_owned(),
            Some(format!("Bearer {KEY}")),
            &json!("openai-model"),
            &json!(true),
            &json!([{"type": "web_search"}]),
            true,
        )
    );
}

#[test]
fn sources_drop_only_the_tracking_parameter_openai_appends() {
    assert_eq!(
        sources(&stored_answer("openai")),
        [
            (
                "Page".to_owned(),
                "https://example.test/page?id=7".to_owned()
            ),
            ("Other".to_owned(), "https://example.test/other".to_owned()),
        ]
    );
}

#[test]
fn a_failed_terminal_event_maps_its_structured_code_and_hides_the_key() {
    let mut event = failed_event("openai");
    event["error"]["code"] = json!("insufficient_quota");
    event["error"]["message"] = json!(format!("quota of {KEY} is used up"));

    let (outcome, _) = search(&format!("data: {event}\n\n"));
    let error = outcome.expect_err("the terminal event fails the search");

    assert_eq!(
        (error.kind, error.message.as_str()),
        (
            AttemptErrorKind::QuotaExhausted,
            "OpenAI Responses ended with response.failed: quota of ******** is used up"
        )
    );
}
