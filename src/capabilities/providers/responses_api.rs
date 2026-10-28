//! The Responses API transport that every provider speaking the protocol shares: the request
//! body, the SSE stream, terminal failure events, and the text and citations of a completed
//! response. Each caller builds its own request and decodes the completed response itself;
//! failure messages name the caller's service.

use eventsource_stream::Event;
use futures_util::{Stream, StreamExt};
use reqwest::{Client, StatusCode};
use serde::Serialize;
use serde_json::Value;

use crate::credentials::CredentialPool;
use crate::net::{
    AttemptFailure, RetryPolicy, error_kind_for_status, read_capped_sse, send_provider_request,
};
use crate::providers::execution::ExecutionSettings;
use crate::providers::shared::redacted_urls_message;
use crate::redact::Secret;
use crate::types::{AttemptErrorKind, AttemptTarget, Deadline, Source};

/// One streamed Responses API request.
#[derive(Serialize)]
pub(super) struct ResponsesRequest<'a> {
    model: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<&'a str>,
    input: [InputMessage<'a>; 1],
    stream: bool,
    tools: Vec<Value>,
    /// The structured output format, for a caller that decodes JSON.
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<&'static str>,
}

#[derive(Serialize)]
struct InputMessage<'a> {
    role: &'static str,
    content: &'a str,
}

impl<'a> ResponsesRequest<'a> {
    /// Returns a request that sends `input` as the user's message.
    pub(super) fn new(
        model: &'a str,
        instructions: Option<&'a str>,
        input: &'a str,
        tools: Vec<Value>,
    ) -> Self {
        Self {
            model,
            instructions,
            input: [InputMessage {
                role: "user",
                content: input,
            }],
            stream: true,
            tools,
            text: None,
            tool_choice: None,
        }
    }

    /// Requires the model to call a tool before it answers.
    pub(super) fn requiring_tool(mut self) -> Self {
        self.tool_choice = Some("required");
        self
    }

    /// Asks for output in the `format` of the Responses API `text.format` field.
    pub(super) fn with_format(mut self, format: Value) -> Self {
        self.text = Some(Value::Object(serde_json::Map::from_iter([(
            "format".to_owned(),
            format,
        )])));
        self
    }
}

/// The Responses endpoint of one provider.
pub(super) struct ResponsesEndpoint<'a> {
    /// The provider its attempts record.
    pub(super) provider: &'static str,
    /// The service failure messages name, such as `xAI Responses`.
    pub(super) service: &'static str,
    pub(super) timeout_message: &'static str,
    /// The API base URL; requests go to its `/responses` path.
    pub(super) url: &'a str,
    pub(super) client: &'a Client,
    pub(super) credentials: &'a CredentialPool,
}

impl ResponsesEndpoint<'_> {
    /// Returns the settings of one request: streamed, bounded only by the command deadline.
    pub(super) fn settings(
        &self,
        target: AttemptTarget,
        model: String,
        retry_policy: RetryPolicy,
        deadline: Deadline,
    ) -> ExecutionSettings {
        ExecutionSettings {
            provider: self.provider,
            target,
            retry_policy,
            deadline,
            attempt_timeout: deadline.remaining().unwrap_or_default(),
            verbose: false,
            timeout_message: self.timeout_message,
            model: Some(model),
            transport: Some("sse"),
            endpoint_host: reqwest::Url::parse(self.url)
                .ok()
                .and_then(|url| url.host_str().map(ToOwned::to_owned)),
        }
    }

    /// Sends one request and returns the HTTP status and the completed response.
    pub(super) async fn complete(
        &self,
        body: &ResponsesRequest<'_>,
        credential: &Secret,
    ) -> Result<(u16, Value), AttemptFailure> {
        let endpoint = format!("{}/responses", self.url.trim_end_matches('/'));
        let request = self
            .client
            .post(endpoint)
            .bearer_auth(credential.expose())
            .header("accept", "text/event-stream")
            .json(body);
        let response = send_provider_request(request, self.credentials).await?;
        let service = self.service;
        let (status, events) = read_capped_sse(
            response,
            self.credentials,
            |body, _| request_failure_message(service, body),
            format!("{service} returned invalid SSE"),
        )
        .await?;
        let completed = completed_from_events(service, events, self.credentials).await?;
        Ok((status, completed))
    }
}

/// Reads the stream to its `response.completed` payload; a failed or incomplete response fails
/// the attempt.
async fn completed_from_events<S>(
    service: &str,
    mut events: S,
    credentials: &CredentialPool,
) -> Result<Value, AttemptFailure>
where
    S: Stream<Item = Result<Event, AttemptFailure>> + Unpin,
{
    let mut completed = None;
    while let Some(event) = events.next().await {
        let event = event?;
        let Some(mut payload) = parse_event_data(&event.data).map_err(|error| AttemptFailure {
            kind: AttemptErrorKind::Runtime,
            status: Some(200),
            message: redacted_urls_message(
                &format!("{service} returned invalid event JSON: {error}"),
                credentials,
            ),
        })?
        else {
            continue;
        };
        match payload.get("type").and_then(Value::as_str) {
            Some("response.completed") => {
                let response = payload
                    .get_mut("response")
                    .map(Value::take)
                    .ok_or_else(|| AttemptFailure {
                        kind: AttemptErrorKind::Runtime,
                        status: Some(200),
                        message: format!("{service} response.completed omitted response data"),
                    })?;
                completed = Some(response);
            }
            Some("response.failed" | "response.incomplete") => {
                return Err(AttemptFailure {
                    kind: terminal_error_kind(&payload),
                    status: Some(200),
                    message: redacted_urls_message(
                        &terminal_message(service, &payload),
                        credentials,
                    ),
                });
            }
            _ => {}
        }
    }
    completed.ok_or_else(|| AttemptFailure {
        kind: AttemptErrorKind::Runtime,
        status: Some(200),
        message: format!("{service} stream ended before response.completed"),
    })
}

/// Returns the output text of a completed response and the sources its `url_citation`
/// annotations name.
pub(super) fn output_text(
    service: &str,
    response: &Value,
) -> Result<(String, Vec<Source>), AttemptFailure> {
    let mut text_parts = Vec::new();
    let mut sources = Vec::new();
    for content in response
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("content").and_then(Value::as_array))
        .flatten()
        .filter(|content| content.get("type").and_then(Value::as_str) == Some("output_text"))
    {
        if let Some(text) = content
            .get("text")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            text_parts.push(text.to_owned());
        }
        for annotation in content
            .get("annotations")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|annotation| {
                annotation.get("type").and_then(Value::as_str) == Some("url_citation")
            })
        {
            let Some(url) = annotation
                .get("url")
                .and_then(Value::as_str)
                .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
            else {
                continue;
            };
            let title = annotation
                .get("title")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|title| !title.is_empty())
                .unwrap_or_default()
                .to_owned();
            sources.push(Source {
                title,
                url: url.to_owned(),
                published_date: None,
                author: None,
                text: None,
                highlights: Vec::new(),
                id: None,
                image: None,
                favicon: None,
            });
        }
    }
    let answer = text_parts.join("\n\n");
    if answer.is_empty() {
        return Err(AttemptFailure {
            kind: AttemptErrorKind::Runtime,
            status: Some(200),
            message: format!("{service} response.completed contained no output_text"),
        });
    }
    Ok((answer, sources))
}

fn terminal_message(service: &str, event: &Value) -> String {
    let event_type = event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("a terminal failure");
    event
        .get("error")
        .or_else(|| {
            event
                .get("response")
                .and_then(|response| response.get("error"))
        })
        .and_then(|error| {
            error
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| error.as_str())
        })
        .map_or_else(
            || format!("{service} ended with {event_type}"),
            |message| format!("{service} ended with {event_type}: {message}"),
        )
}

fn request_failure_message(service: &str, body: &str) -> String {
    if body.trim().is_empty() {
        format!("{service} request failed")
    } else {
        body.to_owned()
    }
}

fn parse_event_data(data: &str) -> Result<Option<Value>, serde_json::Error> {
    let data = data.trim();
    if data.is_empty() || data == "[DONE]" {
        return Ok(None);
    }
    serde_json::from_str(data).map(Some)
}

fn terminal_error_kind(event: &Value) -> AttemptErrorKind {
    let response = event.get("response");
    let error = event.get("error");
    let response_error = response.and_then(|response| response.get("error"));
    let incomplete_reason = event
        .get("incomplete_details")
        .or_else(|| response.and_then(|response| response.get("incomplete_details")))
        .and_then(|details| details.get("reason"));

    [
        event.get("code"),
        error.and_then(|error| error.get("code")),
        error.and_then(|error| error.get("type")),
        error.filter(|error| error.is_string() || error.is_number()),
        response.and_then(|response| response.get("code")),
        response_error.and_then(|error| error.get("code")),
        response_error.and_then(|error| error.get("type")),
        response_error.filter(|error| error.is_string() || error.is_number()),
        incomplete_reason,
        error.and_then(|error| error.get("status")),
        response_error.and_then(|error| error.get("status")),
        event.get("status"),
        response.and_then(|response| response.get("status")),
    ]
    .into_iter()
    .flatten()
    .find_map(classify_terminal_signal)
    .unwrap_or(AttemptErrorKind::Runtime)
}

fn classify_terminal_signal(signal: &Value) -> Option<AttemptErrorKind> {
    if let Some(status) = signal
        .as_u64()
        .and_then(|status| u16::try_from(status).ok())
        .or_else(|| signal.as_str().and_then(|status| status.parse().ok()))
        .and_then(|status| StatusCode::from_u16(status).ok())
    {
        let kind = error_kind_for_status(status, "");
        return (kind != AttemptErrorKind::Runtime).then_some(kind);
    }
    match signal.as_str()? {
        "server_error" | "network_error" | "connection_error" | "overloaded" => {
            Some(AttemptErrorKind::Network)
        }
        "timeout" | "request_timeout" => Some(AttemptErrorKind::Timeout),
        "rate_limited" | "rate_limit_exceeded" => Some(AttemptErrorKind::RateLimited),
        "insufficient_quota" | "quota_exceeded" => Some(AttemptErrorKind::QuotaExhausted),
        "authentication_error" | "invalid_api_key" | "unauthorized" | "permission_denied" => {
            Some(AttemptErrorKind::Auth)
        }
        "invalid_request" | "invalid_request_error" | "invalid_parameter" => {
            Some(AttemptErrorKind::Parameter)
        }
        "content_filter" | "max_output_tokens" | "max_tokens" => Some(AttemptErrorKind::Quality),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use eventsource_stream::Event;
    use futures_util::stream;

    use crate::credentials::CredentialPool;
    use crate::net::AttemptFailure;
    use crate::types::AttemptErrorKind;

    use super::{completed_from_events, output_text, parse_event_data, terminal_error_kind};

    #[test]
    fn terminal_classification_uses_only_structured_signals() {
        let cases = [
            (
                serde_json::json!({"error": {"code": "server_error"}}),
                AttemptErrorKind::Network,
            ),
            (
                serde_json::json!({"response": {"error": {"code": "request_timeout"}}}),
                AttemptErrorKind::Timeout,
            ),
            (
                serde_json::json!({"error": {"code": "rate_limit_exceeded"}}),
                AttemptErrorKind::RateLimited,
            ),
            (
                serde_json::json!({"response": {"error": {"code": "insufficient_quota"}}}),
                AttemptErrorKind::QuotaExhausted,
            ),
            (
                serde_json::json!({"status": 429, "error": {"code": "insufficient_quota"}}),
                AttemptErrorKind::QuotaExhausted,
            ),
            (
                serde_json::json!({"error": {"code": "invalid_api_key"}}),
                AttemptErrorKind::Auth,
            ),
            (
                serde_json::json!({"error": {"code": "invalid_request_error"}}),
                AttemptErrorKind::Parameter,
            ),
            (
                serde_json::json!({"response": {"incomplete_details": {"reason": "content_filter"}}}),
                AttemptErrorKind::Quality,
            ),
            (
                serde_json::json!({"response": {"incomplete_details": {"reason": "max_output_tokens"}}}),
                AttemptErrorKind::Quality,
            ),
            (
                serde_json::json!({"status": 503}),
                AttemptErrorKind::Network,
            ),
            (
                serde_json::json!({"error": {"message": "rate limit exceeded"}}),
                AttemptErrorKind::Runtime,
            ),
            (
                serde_json::json!({"response": {"status": "failed"}}),
                AttemptErrorKind::Runtime,
            ),
        ];

        for (payload, expected) in cases {
            assert_eq!(terminal_error_kind(&payload), expected, "{payload}");
        }
    }

    #[test]
    fn completed_response_requires_nonempty_output_text() {
        let completed = serde_json::json!({
            "output": [{"content": [{"type": "output_text", "text": "answer"}]}]
        });
        let missing_output_text = serde_json::json!({
            "output": [{"content": [{"type": "reasoning", "text": "hidden"}]}]
        });

        assert_eq!(
            output_text("Test Responses", &completed)
                .ok()
                .map(|(answer, _)| answer),
            Some("answer".into())
        );
        assert_eq!(
            output_text("Test Responses", &missing_output_text)
                .err()
                .map(|error| error.kind),
            Some(AttemptErrorKind::Runtime)
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
    fn event_stream_rejects_transport_errors_and_missing_completion() {
        let credentials = CredentialPool::new("xai", vec!["key".into()]);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");

        runtime.block_on(async {
            let transport = stream::iter([Err(AttemptFailure {
                kind: AttemptErrorKind::Network,
                status: Some(200),
                message: "connection reset".into(),
            })]);
            assert_eq!(
                completed_from_events("Test Responses", transport, &credentials)
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
                completed_from_events("Test Responses", missing_completion, &credentials)
                    .await
                    .err()
                    .map(|error| error.kind),
                Some(AttemptErrorKind::Runtime)
            );
        });
    }
}
