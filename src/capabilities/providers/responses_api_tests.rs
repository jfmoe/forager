//! The Responses stream rules every provider speaking the protocol shares: the events of a
//! stream, its terminal failures, the size cap and the text and citations of a completed
//! response.

use eventsource_stream::{Event, Eventsource};
use futures_util::{StreamExt, stream};
use serde_json::{Value, json};

use super::{
    ResponsesEndpoint, ResponsesRequest, completed_from_events, output_text, parse_event_data,
    terminal_error_kind,
};
use crate::credentials::CredentialPool;
use crate::fixture_server::{Fixture, block_on};
use crate::main_search_samples::{answer_with, completed_events, stored_answer};
use crate::net::{AttemptFailure, MAX_RESPONSE_BYTES, build_client};
use crate::types::AttemptErrorKind;

const SERVICE: &str = "Test Responses";
const KEY: &str = "responses-key";

fn credentials() -> CredentialPool {
    CredentialPool::new("xai", vec![KEY.into()])
}

/// Reads the SSE `body` to its completed response, as a stream that arrives whole.
fn completed(body: &str) -> Result<Value, AttemptFailure> {
    let events = Box::pin(
        stream::iter([Ok::<_, std::io::Error>(body.as_bytes().to_vec())])
            .eventsource()
            .map(|event| {
                event.map_err(|error| AttemptFailure {
                    kind: AttemptErrorKind::Network,
                    status: Some(200),
                    message: error.to_string(),
                })
            }),
    );
    block_on(completed_from_events(SERVICE, events, &credentials()))
}

/// Sends one request to a Responses endpoint at a fixture that streams `body`, and returns the
/// completed response.
fn complete_from(body: &str) -> Result<Value, AttemptFailure> {
    let fixture = Fixture::start(200, "text/event-stream", body);
    let client = build_client(true).expect("build client");
    let credentials = credentials();
    let url = format!("{}/v1", fixture.url);
    let endpoint = ResponsesEndpoint {
        provider: "xai",
        service: SERVICE,
        timeout_message: "Test Responses request timed out",
        url: &url,
        client: &client,
        credentials: &credentials,
    };
    let outcome = block_on(endpoint.complete(
        &ResponsesRequest::new("test-model", None, "query", Vec::new()),
        &KEY.into(),
    ));
    fixture.finish();
    outcome.map(|(_, response)| response)
}

fn failure<T>(result: Result<T, AttemptFailure>) -> (AttemptErrorKind, String) {
    let error = result.err().expect("the stream fails");
    (error.kind, error.message)
}

#[test]
fn terminal_classification_uses_only_structured_signals() {
    let cases = [
        (
            json!({"error": {"code": "server_error"}}),
            AttemptErrorKind::Network,
        ),
        (
            json!({"response": {"error": {"code": "request_timeout"}}}),
            AttemptErrorKind::Timeout,
        ),
        (
            json!({"error": {"code": "rate_limit_exceeded"}}),
            AttemptErrorKind::RateLimited,
        ),
        (
            json!({"response": {"error": {"code": "insufficient_quota"}}}),
            AttemptErrorKind::QuotaExhausted,
        ),
        (
            json!({"status": 429, "error": {"code": "insufficient_quota"}}),
            AttemptErrorKind::QuotaExhausted,
        ),
        (
            json!({"error": {"code": "invalid_api_key"}}),
            AttemptErrorKind::Auth,
        ),
        (
            json!({"error": {"code": "invalid_request_error"}}),
            AttemptErrorKind::Parameter,
        ),
        (
            json!({"response": {"incomplete_details": {"reason": "content_filter"}}}),
            AttemptErrorKind::Quality,
        ),
        (
            json!({"response": {"incomplete_details": {"reason": "max_output_tokens"}}}),
            AttemptErrorKind::Quality,
        ),
        (json!({"status": 503}), AttemptErrorKind::Network),
        (
            json!({"error": {"message": "rate limit exceeded"}}),
            AttemptErrorKind::Runtime,
        ),
        (
            json!({"response": {"status": "failed"}}),
            AttemptErrorKind::Runtime,
        ),
    ];

    for (payload, expected) in cases {
        assert_eq!(terminal_error_kind(&payload), expected, "{payload}");
    }
}

#[test]
fn completed_response_requires_nonempty_output_text() {
    let answers = ["xai", "openai"].map(|provider| {
        output_text(SERVICE, &stored_answer(provider))
            .ok()
            .is_some_and(|(answer, _)| !answer.is_empty())
    });
    let mut reasoning_only = stored_answer("xai");
    reasoning_only["output"][0]["content"] = json!([{"type": "reasoning", "text": "hidden"}]);

    assert_eq!(
        (answers, failure(output_text(SERVICE, &reasoning_only))),
        (
            [true, true],
            (
                AttemptErrorKind::Runtime,
                "Test Responses response.completed contained no output_text".to_owned()
            )
        )
    );
}

#[test]
fn an_untitled_citation_becomes_a_source_that_serializes_without_a_title() {
    let mut response = answer_with("xai", "answer", &[("https://example.test/untitled", "")]);
    response["output"][0]["content"][0]["annotations"][0]
        .as_object_mut()
        .expect("annotation")
        .remove("title");

    let (_, sources) = output_text(SERVICE, &response).expect("the answer decodes");

    assert_eq!(
        serde_json::to_value(&sources).expect("encode sources"),
        json!([{"url": "https://example.test/untitled"}])
    );
}

#[test]
fn event_data_ignores_controls_and_rejects_malformed_payloads() {
    assert_eq!(parse_event_data("").ok(), Some(None));
    assert_eq!(parse_event_data(" [DONE] ").ok(), Some(None));
    assert_eq!(
        parse_event_data(r#"{"type":"response.output_text.delta"}"#)
            .ok()
            .flatten()
            .and_then(|payload| payload["type"].as_str().map(str::to_owned)),
        Some("response.output_text.delta".into())
    );
    assert!(parse_event_data("{not-json}").is_err());
}

#[test]
fn a_stream_yields_its_completed_response_past_comments_controls_and_deltas() {
    let response = stored_answer("xai");
    let delta = json!({"type": "response.output_text.delta", "delta": "Final"});
    let body = format!(
        ": keepalive\n\ndata:\n\ndata: [DONE]\n\nevent: response.output_text.delta\ndata: {delta}\n\n{}",
        completed_events(&response)
    );

    assert_eq!(completed(&body).ok(), Some(response));
}

#[test]
fn unknown_terminals_missing_completion_and_malformed_frames_are_runtime_failures() {
    let cases = [
        (
            "data: {\"type\":\"response.failed\",\"error\":{\"message\":\"upstream failed\"}}\n\n",
            "Test Responses ended with response.failed: upstream failed",
        ),
        (
            "data: {\"type\":\"response.incomplete\",\"response\":{}}\n\n",
            "Test Responses ended with response.incomplete",
        ),
        (
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n",
            "Test Responses stream ended before response.completed",
        ),
        (
            "data: {not-json}\n\n",
            "Test Responses returned invalid event JSON: ",
        ),
    ];

    for (body, message) in cases {
        let (kind, actual) = failure(completed(body));

        assert_eq!(
            (kind, actual.starts_with(message)),
            (AttemptErrorKind::Runtime, true),
            "{body}: {actual}"
        );
    }
}

#[test]
fn event_stream_rejects_transport_errors_and_missing_completion() {
    let credentials = credentials();

    block_on(async {
        let transport = stream::iter([Err(AttemptFailure {
            kind: AttemptErrorKind::Network,
            status: Some(200),
            message: "connection reset".into(),
        })]);
        assert_eq!(
            completed_from_events(SERVICE, transport, &credentials)
                .await
                .err()
                .map(|error| error.kind),
            Some(AttemptErrorKind::Network)
        );

        let missing_completion = stream::iter([Ok::<_, AttemptFailure>(Event {
            data: r#"{"type":"response.output_text.delta","delta":"partial"}"#.into(),
            ..Event::default()
        })]);
        assert_eq!(
            completed_from_events(SERVICE, missing_completion, &credentials)
                .await
                .err()
                .map(|error| error.kind),
            Some(AttemptErrorKind::Runtime)
        );
    });
}

#[test]
fn a_stream_larger_than_four_mibibytes_is_a_runtime_failure() {
    let body = format!("data: {}\n\n", "x".repeat(MAX_RESPONSE_BYTES));

    assert_eq!(
        failure(complete_from(&body)),
        (
            AttemptErrorKind::Runtime,
            "response exceeded 4 MiB".to_owned()
        )
    );
}

#[test]
fn bytes_after_the_completed_event_count_toward_the_four_mibibyte_cap() {
    let body = format!(
        "{}data: {}\n\n",
        completed_events(&stored_answer("xai")),
        "x".repeat(MAX_RESPONSE_BYTES)
    );

    assert_eq!(
        failure(complete_from(&body)),
        (
            AttemptErrorKind::Runtime,
            "response exceeded 4 MiB".to_owned()
        )
    );
}

#[test]
fn a_complete_stream_of_exactly_four_mibibytes_is_accepted() {
    let response = stored_answer("xai");
    let completed = completed_events(&response);
    let padding = "x".repeat(MAX_RESPONSE_BYTES - completed.len() - 3);
    let body = format!(":{padding}\n\n{completed}");

    assert_eq!(
        (body.len(), complete_from(&body).ok()),
        (MAX_RESPONSE_BYTES, Some(response))
    );
}
