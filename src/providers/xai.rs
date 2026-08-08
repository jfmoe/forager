use eventsource_stream::{EventStreamError, Eventsource};
use futures_util::StreamExt;
use reqwest::{Client, Response};
use serde::Serialize;
use serde_json::Value;

use crate::config::XaiRuntimeConfig;
use crate::credentials::CredentialPool;
use crate::net::{
    CappedStreamError, MAX_ERROR_BODY_BYTES, MAX_RESPONSE_BYTES, RetryPolicy, capped_stream,
    error_kind_for_status, read_response_body,
};
use crate::providers::execution::{AttemptFailure, ExecutionSettings, execute_v2};
use crate::providers::shared::redacted_urls_message;
use crate::providers::{MainSearchRequestKind, ProviderError};
use crate::redact::{Secret, redact_url, redact_urls};
use crate::types::{AttemptErrorKind, Deadline, SearchOutcome, Source};

#[derive(Clone, Debug)]
pub(crate) struct SearchRequest {
    pub(crate) query: String,
    pub(crate) model: Option<String>,
    pub(crate) allow_model_fallback: bool,
    pub(crate) verbose: bool,
}

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
        request: SearchRequest,
    ) -> Result<SearchOutcome, ProviderError> {
        self.execute(request, MainSearchRequestKind::Search).await
    }

    pub(crate) async fn probe(
        &self,
        request: SearchRequest,
    ) -> Result<SearchOutcome, ProviderError> {
        self.execute(request, MainSearchRequestKind::ModelProbe)
            .await
    }

    async fn execute(
        &self,
        request: SearchRequest,
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
                seam: "main_search",
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
            input: &input,
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
            let body = read_response_body(response, MAX_ERROR_BODY_BYTES)
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
        let outcome = self.completed_response(response).await?;
        Ok((status.as_u16(), outcome))
    }

    async fn completed_response(
        &self,
        response: Response,
    ) -> Result<(String, Vec<Source>), AttemptFailure> {
        let mut events = capped_stream(response.bytes_stream(), MAX_RESPONSE_BYTES).eventsource();
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
            if event.data.trim().is_empty() || event.data.trim() == "[DONE]" {
                continue;
            }
            let payload: Value =
                serde_json::from_str(&event.data).map_err(|error| AttemptFailure {
                    kind: AttemptErrorKind::Runtime,
                    status: Some(200),
                    message: redacted_urls_message(
                        &format!("xAI Responses returned invalid event JSON: {error}"),
                        &self.credentials,
                    ),
                    redirected_library_id: None,
                })?;
            match payload.get("type").and_then(Value::as_str) {
                Some("response.completed") => {
                    let response = payload.get("response").ok_or_else(|| AttemptFailure {
                        kind: AttemptErrorKind::Runtime,
                        status: Some(200),
                        message: "xAI response.completed omitted response data".into(),
                        redirected_library_id: None,
                    })?;
                    return self.normalize_completed(response);
                }
                Some("response.failed" | "response.incomplete") => {
                    return Err(AttemptFailure {
                        kind: AttemptErrorKind::Runtime,
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
        Err(AttemptFailure {
            kind: AttemptErrorKind::Runtime,
            status: Some(200),
            message: "xAI Responses stream ended before response.completed".into(),
            redirected_library_id: None,
        })
    }

    fn normalize_completed(
        &self,
        response: &Value,
    ) -> Result<(String, Vec<Source>), AttemptFailure> {
        let mut text_parts = Vec::new();
        let mut sources = Vec::new();
        let mut seen = std::collections::HashSet::new();
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
                    .map(redact_url)
                    .map(|url| self.credentials.redact(&url))
                else {
                    continue;
                };
                if !seen.insert(url.clone()) {
                    continue;
                }
                let title = annotation
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|title| !title.is_empty())
                    .map_or_else(String::new, |title| self.redacted_text(title));
                sources.push(Source {
                    title,
                    url,
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

    fn redacted_text(&self, value: &str) -> String {
        self.credentials.redact(&redact_urls(value))
    }
}

#[derive(Serialize)]
struct ResponsesRequest<'a> {
    model: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<&'static str>,
    input: &'a str,
    stream: bool,
    tools: Vec<Tool<'a>>,
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
