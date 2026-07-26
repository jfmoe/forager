use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use reqwest::{Client, Response};
use serde::Serialize;
use serde_json::Value;

use crate::config::{self, XaiRuntimeConfig};
use crate::credentials::CredentialPool;
use crate::net::{RetryPolicy, error_kind_for_status, truncate_message};
use crate::providers::ProviderError;
use crate::providers::execution::{AttemptFailure, ExecutionSettings, execute};
use crate::types::{AttemptErrorKind, Deadline, SearchOutcome, Source};

#[derive(Clone, Debug)]
pub(crate) struct SearchRequest {
    pub(crate) query: String,
    pub(crate) model: Option<String>,
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
        let model = request
            .model
            .clone()
            .unwrap_or_else(|| self.config.model.clone());
        let query = request.query;
        let execution = execute(
            &self.credentials,
            ExecutionSettings {
                provider: "xai",
                seam: "main_search",
                retry_policy: self.retry_policy,
                deadline: self.deadline,
                attempt_timeout: self.deadline.remaining().unwrap_or_default(),
                verbose: request.verbose,
                timeout_message: "xAI Responses request timed out",
            },
            |credential| {
                let model = model.clone();
                let query = query.clone();
                async move { self.send_once(&query, &model, &credential).await }
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
        credential: &str,
    ) -> Result<(u16, (String, Vec<Source>)), AttemptFailure> {
        let endpoint = format!("{}/responses", self.config.url.trim_end_matches('/'));
        let body = ResponsesRequest {
            model,
            input: query,
            stream: true,
            tools: self
                .config
                .tools
                .iter()
                .map(|tool| Tool { kind: tool })
                .collect(),
        };
        let response = self
            .client
            .post(endpoint)
            .bearer_auth(credential)
            .header("accept", "text/event-stream")
            .json(&body)
            .send()
            .await
            .map_err(|error| AttemptFailure {
                kind: if error.is_timeout() {
                    AttemptErrorKind::Timeout
                } else {
                    AttemptErrorKind::Network
                },
                status: error.status().map(|status| status.as_u16()),
                message: self.redacted_message(&error.to_string()),
            })?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(AttemptFailure {
                kind: error_kind_for_status(status, &body),
                status: Some(status.as_u16()),
                message: self.redacted_message(if body.trim().is_empty() {
                    "xAI Responses request failed"
                } else {
                    &body
                }),
            });
        }
        let outcome = self.completed_response(response).await?;
        Ok((status.as_u16(), outcome))
    }

    async fn completed_response(
        &self,
        response: Response,
    ) -> Result<(String, Vec<Source>), AttemptFailure> {
        let mut events = response.bytes_stream().eventsource();
        while let Some(event) = events.next().await {
            let event = event.map_err(|error| AttemptFailure {
                kind: AttemptErrorKind::Network,
                status: Some(200),
                message: self
                    .redacted_message(&format!("xAI Responses returned invalid SSE: {error}")),
            })?;
            if event.data.trim().is_empty() || event.data.trim() == "[DONE]" {
                continue;
            }
            let payload: Value =
                serde_json::from_str(&event.data).map_err(|error| AttemptFailure {
                    kind: AttemptErrorKind::Runtime,
                    status: Some(200),
                    message: self.redacted_message(&format!(
                        "xAI Responses returned invalid event JSON: {error}"
                    )),
                })?;
            match payload.get("type").and_then(Value::as_str) {
                Some("response.completed") => {
                    let response = payload.get("response").ok_or_else(|| AttemptFailure {
                        kind: AttemptErrorKind::Runtime,
                        status: Some(200),
                        message: "xAI response.completed omitted response data".into(),
                    })?;
                    return self.normalize_completed(response);
                }
                Some("response.failed" | "response.incomplete") => {
                    return Err(AttemptFailure {
                        kind: AttemptErrorKind::Runtime,
                        status: Some(200),
                        message: self.redacted_message(&terminal_message(&payload)),
                    });
                }
                _ => {}
            }
        }
        Err(AttemptFailure {
            kind: AttemptErrorKind::Runtime,
            status: Some(200),
            message: "xAI Responses stream ended before response.completed".into(),
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
                text_parts.push(self.redacted_text(text));
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
                    .map(config::redact_url)
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
                    .map(|title| self.redacted_text(title))
                    .unwrap_or_else(|| url.clone());
                sources.push(Source {
                    title,
                    url,
                    published_date: None,
                    author: None,
                    text: None,
                    highlights: Vec::new(),
                });
            }
        }
        let answer = text_parts.join("\n\n");
        if answer.is_empty() {
            return Err(AttemptFailure {
                kind: AttemptErrorKind::Runtime,
                status: Some(200),
                message: "xAI response.completed contained no output_text".into(),
            });
        }
        Ok((answer, sources))
    }

    fn redacted_text(&self, value: &str) -> String {
        self.credentials.redact(&config::redact_urls(value))
    }

    fn redacted_message(&self, value: &str) -> String {
        let endpoint = config::redact_url(&self.config.url);
        truncate_message(
            &self
                .redacted_text(value)
                .replace(&self.config.url, &endpoint),
        )
    }
}

#[derive(Serialize)]
struct ResponsesRequest<'a> {
    model: &'a str,
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
