use eventsource_stream::{Event, EventStreamError, Eventsource};
use futures_util::{Stream, StreamExt};
use reqwest::{Client, Response, StatusCode};
use serde::Serialize;
use serde_json::Value;

use crate::config::XaiRuntimeConfig;
use crate::credentials::CredentialPool;
use crate::net::{
    CappedStreamError, MAX_RESPONSE_BYTES, ResponseBodyPolicy, RetryPolicy, capped_stream,
    error_kind_for_status, read_response_body,
};
use crate::providers::execution::{AttemptFailure, ExecutionSettings, execute_v2};
use crate::providers::shared::{
    normalize_main_search, redact_and_deduplicate_sources, redacted_urls_message,
};
use crate::providers::{MainSearchRequest, MainSearchRequestKind, ProviderError};
use crate::redact::Secret;
use crate::types::{AttemptErrorKind, AttemptTarget, Deadline, SearchOutcome, Source};

pub(crate) struct Xai {
    config: XaiRuntimeConfig,
    client: Client,
    credentials: CredentialPool,
    retry_policy: RetryPolicy,
    deadline: Deadline,
}

impl Xai {
    pub(crate) fn new(
        config: XaiRuntimeConfig,
        client: Client,
        credentials: CredentialPool,
        retry_policy: RetryPolicy,
        deadline: Deadline,
    ) -> Self {
        Self {
            config,
            client,
            credentials,
            retry_policy,
            deadline,
        }
    }

    pub(crate) async fn search(
        &self,
        request: MainSearchRequest,
    ) -> Result<SearchOutcome, ProviderError> {
        self.execute(request, MainSearchRequestKind::Search).await
    }

    pub(crate) async fn probe(
        &self,
        request: MainSearchRequest,
    ) -> Result<SearchOutcome, ProviderError> {
        self.execute(request, MainSearchRequestKind::ModelProbe)
            .await
    }

    async fn execute(
        &self,
        request: MainSearchRequest,
        request_kind: MainSearchRequestKind,
    ) -> Result<SearchOutcome, ProviderError> {
        let model = request
            .model
            .clone()
            .unwrap_or_else(|| self.config.model.clone());
        let query = request.query;
        let execution = execute_v2(
            &self.credentials,
            ExecutionSettings {
                provider: "xai",
                target: AttemptTarget::seam("main_search"),
                retry_policy: self.retry_policy,
                deadline: self.deadline,
                attempt_timeout: self.deadline.remaining().unwrap_or_default(),
                verbose: request.verbose,
                timeout_message: "xAI Responses request timed out",
                model: Some(model.clone()),
                transport: Some("sse"),
                endpoint_host: reqwest::Url::parse(&self.config.url)
                    .ok()
                    .and_then(|url| url.host_str().map(ToOwned::to_owned)),
                breaker_event: None,
            },
            |credential, _| {
                let model = model.clone();
                let query = query.clone();
                async move {
                    self.send_once(&query, &model, &credential, request_kind)
                        .await
                }
            },
        )
        .await?;
        let (answer, sources) = execution.value;
        Ok(SearchOutcome {
            provider: "xai",
            query,
            model,
            answer,
            sources,
            extra_sources: Vec::new(),
            capabilities: Vec::new(),
            capability_gaps: Vec::new(),
            attempts: execution.attempts,
            diagnostic: execution.diagnostic,
        })
    }

    async fn send_once(
        &self,
        query: &str,
        model: &str,
        credential: &Secret,
        request_kind: MainSearchRequestKind,
    ) -> Result<(u16, (String, Vec<Source>)), AttemptFailure> {
        let endpoint = format!("{}/responses", self.config.url.trim_end_matches('/'));
        let input = request_kind.input(query);
        let tools = if request_kind.uses_search_tools() {
            self.config
                .tools
                .iter()
                .map(|tool| Tool { kind: tool })
                .collect()
        } else {
            Vec::new()
        };
        let body = ResponsesRequest {
            model,
            instructions: request_kind.instruction(),
            input: [InputMessage {
                role: "user",
                content: &input,
            }],
            stream: true,
            tools,
        };
        let response = self
            .client
            .post(endpoint)
            .bearer_auth(credential.expose())
            .header("accept", "text/event-stream")
            .json(&body)
            .send()
            .await
            .map_err(|error| AttemptFailure {
                kind: AttemptErrorKind::Network,
                status: error.status().map(|status| status.as_u16()),
                message: redacted_urls_message(&error.to_string(), &self.credentials),
                redirected_library_id: None,
            })?;
        let status = response.status();
        if !status.is_success() {
            let body = read_response_body(response, ResponseBodyPolicy::Error)
                .await
                .map(|body| body.text)
                .unwrap_or_default();
            return Err(AttemptFailure {
                kind: error_kind_for_status(status, &body),
                status: Some(status.as_u16()),
                message: redacted_urls_message(
                    if body.trim().is_empty() {
                        "xAI Responses request failed"
                    } else {
                        &body
                    },
                    &self.credentials,
                ),
                redirected_library_id: None,
            });
        }
        let (answer, sources) = self.completed_response(response).await?;
        let outcome = if matches!(request_kind, MainSearchRequestKind::Search) {
            normalize_main_search(&answer, sources, &self.credentials)?
        } else {
            (
                answer,
                redact_and_deduplicate_sources(sources, &self.credentials),
            )
        };
        Ok((status.as_u16(), outcome))
    }

    async fn completed_response(
        &self,
        response: Response,
    ) -> Result<(String, Vec<Source>), AttemptFailure> {
        let events = capped_stream(response.bytes_stream(), MAX_RESPONSE_BYTES).eventsource();
        self.completed_from_events(events).await
    }

    async fn completed_from_events<S, E>(
        &self,
        mut events: S,
    ) -> Result<(String, Vec<Source>), AttemptFailure>
    where
        S: Stream<Item = Result<Event, EventStreamError<CappedStreamError<E>>>> + Unpin,
        E: std::fmt::Display,
    {
        let mut completed = None;
        while let Some(event) = events.next().await {
            let event = match event {
                Err(EventStreamError::Transport(CappedStreamError::LimitExceeded)) => {
                    return Err(AttemptFailure {
                        kind: AttemptErrorKind::Runtime,
                        status: Some(200),
                        message: "response exceeded 4 MiB".into(),
                        redirected_library_id: None,
                    });
                }
                Err(error) => {
                    return Err(AttemptFailure {
                        kind: AttemptErrorKind::Network,
                        status: Some(200),
                        message: redacted_urls_message(
                            &format!("xAI Responses returned invalid SSE: {error}"),
                            &self.credentials,
                        ),
                        redirected_library_id: None,
                    });
                }
                Ok(event) => event,
            };
            let Some(payload) = parse_event_data(&event.data).map_err(|error| AttemptFailure {
                kind: AttemptErrorKind::Runtime,
                status: Some(200),
                message: redacted_urls_message(
                    &format!("xAI Responses returned invalid event JSON: {error}"),
                    &self.credentials,
                ),
                redirected_library_id: None,
            })?
            else {
                continue;
            };
            match payload.get("type").and_then(Value::as_str) {
                Some("response.completed") => {
                    let response = payload.get("response").ok_or_else(|| AttemptFailure {
                        kind: AttemptErrorKind::Runtime,
                        status: Some(200),
                        message: "xAI response.completed omitted response data".into(),
                        redirected_library_id: None,
                    })?;
                    completed = Some(Self::normalize_completed(response)?);
                }
                Some("response.failed" | "response.incomplete") => {
                    return Err(AttemptFailure {
                        kind: terminal_error_kind(&payload),
                        status: Some(200),
                        message: redacted_urls_message(
                            &terminal_message(&payload),
                            &self.credentials,
                        ),
                        redirected_library_id: None,
                    });
                }
                _ => {}
            }
        }
        completed.ok_or_else(|| AttemptFailure {
            kind: AttemptErrorKind::Runtime,
            status: Some(200),
            message: "xAI Responses stream ended before response.completed".into(),
            redirected_library_id: None,
        })
    }

    fn normalize_completed(response: &Value) -> Result<(String, Vec<Source>), AttemptFailure> {
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
                message: "xAI response.completed contained no output_text".into(),
                redirected_library_id: None,
            });
        }
        Ok((answer, sources))
    }
}

#[derive(Serialize)]
struct ResponsesRequest<'a> {
    model: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<&'static str>,
    input: [InputMessage<'a>; 1],
    stream: bool,
    tools: Vec<Tool<'a>>,
}

#[derive(Serialize)]
struct InputMessage<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Serialize)]
struct Tool<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
}

fn terminal_message(event: &Value) -> String {
    let event_type = event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("xAI terminal failure");
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
            || format!("xAI Responses ended with {event_type}"),
            |message| format!("xAI Responses ended with {event_type}: {message}"),
        )
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
    use std::io;
    use std::time::Duration;

    use eventsource_stream::{Event, EventStreamError};
    use futures_util::stream;
    use serde_json::Value;

    use crate::config::XaiRuntimeConfig;
    use crate::credentials::CredentialPool;
    use crate::net::{CappedStreamError, RetryPolicy};
    use crate::providers::MainSearchRequestKind;
    use crate::types::{AttemptErrorKind, Deadline};

    use super::{InputMessage, ResponsesRequest, Tool, Xai, parse_event_data, terminal_error_kind};

    fn provider() -> Xai {
        Xai::new(
            XaiRuntimeConfig {
                url: "https://api.example/v1".into(),
                keys: vec!["key".into()],
                model: "model".into(),
                tools: vec!["web_search".into()],
            },
            reqwest::Client::new(),
            CredentialPool::new("xai", vec!["key".into()]),
            RetryPolicy::new(1, 1.0, Duration::ZERO),
            Deadline::new(Duration::from_secs(10)),
        )
    }

    #[test]
    fn request_profiles_use_the_same_role_array_input() {
        for request_kind in [
            MainSearchRequestKind::Search,
            MainSearchRequestKind::ModelProbe,
        ] {
            let input = request_kind.input("current releases");
            let tools = if request_kind.uses_search_tools() {
                vec![Tool { kind: "web_search" }]
            } else {
                Vec::new()
            };
            let body = serde_json::to_value(ResponsesRequest {
                model: "model",
                instructions: request_kind.instruction(),
                input: [InputMessage {
                    role: "user",
                    content: &input,
                }],
                stream: true,
                tools,
            })
            .expect("serialize request");

            assert_eq!(body["input"][0]["role"], "user");
            assert!(
                body["input"][0]["content"]
                    .as_str()
                    .is_some_and(|content| content.contains("current releases"))
            );
            match request_kind {
                MainSearchRequestKind::Search => {
                    assert!(body["instructions"].as_str().is_some());
                    assert_eq!(body["tools"][0]["type"], "web_search");
                }
                MainSearchRequestKind::ModelProbe => {
                    assert_eq!(body.get("instructions"), None);
                    assert_eq!(body["tools"], Value::Array(Vec::new()));
                    assert_eq!(body["input"][0]["content"], "current releases");
                }
            }
        }
    }

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
            Xai::normalize_completed(&completed)
                .ok()
                .map(|(answer, _)| answer),
            Some("answer".into())
        );
        assert_eq!(
            Xai::normalize_completed(&missing_output_text)
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
        let provider = provider();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");

        runtime.block_on(async {
            let transport = stream::iter([Err(EventStreamError::Transport(
                CappedStreamError::Transport(io::Error::other("connection reset")),
            ))]);
            assert_eq!(
                provider
                    .completed_from_events(transport)
                    .await
                    .err()
                    .map(|error| error.kind),
                Some(AttemptErrorKind::Network)
            );

            let missing_completion = stream::iter([Ok::<
                _,
                EventStreamError<CappedStreamError<io::Error>>,
            >(Event {
                data: r#"{"type":"response.output_text.delta","delta":"partial"}"#.into(),
                ..Event::default()
            })]);
            assert_eq!(
                provider
                    .completed_from_events(missing_completion)
                    .await
                    .err()
                    .map(|error| error.kind),
                Some(AttemptErrorKind::Runtime)
            );
        });
    }
}
