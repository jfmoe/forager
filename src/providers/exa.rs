use std::time::{Duration, Instant};

use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::{self, ExaRuntimeConfig};
use crate::credentials::CredentialPool;
use crate::net::{RetryPolicy, error_kind_for_status, truncate_message};
use crate::types::{AttemptErrorKind, Deadline, ProviderAttempt, SearchOutcome, Source};

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SearchType {
    Neural,
    Keyword,
    Auto,
}

#[derive(Clone, Debug)]
pub(crate) struct ExaSearchRequest {
    pub(crate) query: String,
    pub(crate) num_results: u16,
    pub(crate) search_type: SearchType,
    pub(crate) include_text: bool,
    pub(crate) include_highlights: bool,
    pub(crate) start_published_date: Option<String>,
    pub(crate) include_domains: Vec<String>,
    pub(crate) exclude_domains: Vec<String>,
    pub(crate) category: Option<String>,
    pub(crate) verbose: bool,
}

#[derive(Debug, Error)]
#[error("{message}")]
pub struct ProviderError {
    pub kind: AttemptErrorKind,
    pub message: String,
    pub attempts: Vec<ProviderAttempt>,
    pub verbose: bool,
    pub diagnostic: Option<String>,
}

pub(crate) struct Exa {
    config: ExaRuntimeConfig,
    client: Client,
    credentials: CredentialPool,
    retry_policy: RetryPolicy,
    deadline: Deadline,
}

impl Exa {
    pub(crate) fn new(
        config: ExaRuntimeConfig,
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
        request: ExaSearchRequest,
    ) -> Result<SearchOutcome, ProviderError> {
        let selection = self.credentials.claim();
        let mut attempts = Vec::new();
        let mut credential_index = selection.index;
        let mut retry_count = 0;
        let mut rotation_count = 0;

        loop {
            let Some(remaining) = self.deadline.remaining() else {
                return Err(terminal_error(
                    AttemptErrorKind::Timeout,
                    attempts,
                    request.verbose,
                    selection.diagnostic.clone(),
                ));
            };
            let attempt_limit = remaining.min(Duration::from_secs(self.config.timeout_seconds));
            let started = Instant::now();
            let response = tokio::time::timeout(
                attempt_limit,
                self.send_once(&request, self.credentials.key(credential_index)),
            )
            .await;

            match response {
                Ok(Ok(outcome)) => {
                    attempts.push(ProviderAttempt {
                        provider: "exa",
                        seam: "docs_search",
                        error_kind: None,
                        http_status: Some(200),
                        duration_ms: millis(started.elapsed()),
                        credential_index,
                        retry_count,
                        rotation_count,
                        message: String::new(),
                    });
                    return Ok(SearchOutcome {
                        provider: "exa",
                        query: request.query,
                        results: outcome,
                        attempts: if request.verbose {
                            attempts
                        } else {
                            Vec::new()
                        },
                        diagnostic: selection.diagnostic,
                    });
                }
                Ok(Err(failure)) => {
                    let kind = failure.kind;
                    attempts.push(ProviderAttempt {
                        provider: "exa",
                        seam: "docs_search",
                        error_kind: Some(kind),
                        http_status: failure.status,
                        duration_ms: millis(started.elapsed()),
                        credential_index,
                        retry_count,
                        rotation_count,
                        message: failure.message,
                    });

                    if kind.rotates_credential() && rotation_count + 1 < self.credentials.len() {
                        rotation_count += 1;
                        credential_index = self
                            .credentials
                            .rotated_index(selection.index, rotation_count);
                        continue;
                    }
                    if kind.is_retryable() && attempts.len() < self.retry_policy.max_attempts() {
                        retry_count += 1;
                        let wait = self.retry_policy.wait(retry_count);
                        let Some(remaining) = self.deadline.remaining() else {
                            return Err(terminal_error(
                                AttemptErrorKind::Timeout,
                                attempts,
                                request.verbose,
                                selection.diagnostic.clone(),
                            ));
                        };
                        if wait >= remaining {
                            return Err(terminal_error(
                                AttemptErrorKind::Timeout,
                                attempts,
                                request.verbose,
                                selection.diagnostic.clone(),
                            ));
                        }
                        tokio::time::sleep(wait).await;
                        continue;
                    }
                    return Err(terminal_error(
                        kind,
                        attempts,
                        request.verbose,
                        selection.diagnostic.clone(),
                    ));
                }
                Err(_) => {
                    attempts.push(ProviderAttempt {
                        provider: "exa",
                        seam: "docs_search",
                        error_kind: Some(AttemptErrorKind::Timeout),
                        http_status: None,
                        duration_ms: millis(started.elapsed()),
                        credential_index,
                        retry_count,
                        rotation_count,
                        message: "Exa request timed out".into(),
                    });
                    if attempts.len() < self.retry_policy.max_attempts() {
                        retry_count += 1;
                        continue;
                    }
                    return Err(terminal_error(
                        AttemptErrorKind::Timeout,
                        attempts,
                        request.verbose,
                        selection.diagnostic.clone(),
                    ));
                }
            }
        }
    }

    async fn send_once(
        &self,
        request: &ExaSearchRequest,
        key: &str,
    ) -> Result<Vec<Source>, AttemptFailure> {
        let response = self
            .client
            .post(format!("{}/search", self.config.url.trim_end_matches('/')))
            .header("x-api-key", key)
            .json(&ExaRequestBody::from(request))
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
        let body = response.text().await.map_err(|error| AttemptFailure {
            kind: if error.is_timeout() {
                AttemptErrorKind::Timeout
            } else {
                AttemptErrorKind::Network
            },
            status: Some(status.as_u16()),
            message: self.redacted_message(&error.to_string()),
        })?;
        if !status.is_success() {
            return Err(AttemptFailure {
                kind: error_kind_for_status(status, &body),
                status: Some(status.as_u16()),
                message: self.redacted_message(&failure_message(&body, status.as_u16())),
            });
        }
        let response: ExaResponse =
            serde_json::from_str(&body).map_err(|error| AttemptFailure {
                kind: AttemptErrorKind::Runtime,
                status: Some(status.as_u16()),
                message: self.redacted_message(&format!("invalid Exa response: {error}")),
            })?;
        Ok(response
            .results
            .into_iter()
            .map(|result| self.normalize_source(result))
            .collect())
    }

    fn normalize_source(&self, result: ExaResult) -> Source {
        Source {
            title: self.credentials.redact(&result.title),
            url: self.credentials.redact(&config::redact_url(&result.url)),
            published_date: result
                .published_date
                .map(|value| self.credentials.redact(&value)),
            author: result.author.map(|value| self.credentials.redact(&value)),
            text: result.text.map(|value| self.credentials.redact(&value)),
            highlights: result
                .highlights
                .iter()
                .map(|value| self.credentials.redact(value))
                .collect(),
        }
    }

    fn redacted_message(&self, message: &str) -> String {
        let redacted_endpoint = config::redact_url(&self.config.url);
        truncate_message(
            &self
                .credentials
                .redact(message)
                .replace(&self.config.url, &redacted_endpoint),
        )
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExaRequestBody<'a> {
    query: &'a str,
    num_results: u16,
    #[serde(rename = "type")]
    search_type: SearchType,
    #[serde(skip_serializing_if = "Option::is_none")]
    contents: Option<ExaContents>,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_published_date: Option<&'a str>,
    #[serde(skip_serializing_if = "slice_is_empty")]
    include_domains: &'a [String],
    #[serde(skip_serializing_if = "slice_is_empty")]
    exclude_domains: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    category: Option<&'a str>,
}

impl<'a> From<&'a ExaSearchRequest> for ExaRequestBody<'a> {
    fn from(request: &'a ExaSearchRequest) -> Self {
        let contents =
            (request.include_text || request.include_highlights).then_some(ExaContents {
                text: request.include_text,
                highlights: request.include_highlights,
            });
        Self {
            query: &request.query,
            num_results: request.num_results,
            search_type: request.search_type,
            contents,
            start_published_date: request.start_published_date.as_deref(),
            include_domains: &request.include_domains,
            exclude_domains: &request.exclude_domains,
            category: request.category.as_deref(),
        }
    }
}

#[derive(Serialize)]
struct ExaContents {
    text: bool,
    highlights: bool,
}

#[derive(Deserialize)]
struct ExaResponse {
    #[serde(default)]
    results: Vec<ExaResult>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExaResult {
    #[serde(default)]
    title: String,
    url: String,
    published_date: Option<String>,
    author: Option<String>,
    text: Option<String>,
    #[serde(default)]
    highlights: Vec<String>,
}

struct AttemptFailure {
    kind: AttemptErrorKind,
    status: Option<u16>,
    message: String,
}

fn terminal_error(
    kind: AttemptErrorKind,
    attempts: Vec<ProviderAttempt>,
    verbose: bool,
    diagnostic: Option<String>,
) -> ProviderError {
    let message = attempts.last().map_or_else(
        || "Exa request failed".into(),
        |attempt| attempt.message.clone(),
    );
    ProviderError {
        kind,
        message,
        attempts,
        verbose,
        diagnostic,
    }
}

fn failure_message(body: &str, status: u16) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(|error| error.get("message").or(Some(error)))
                .and_then(|message| message.as_str())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| format!("Exa returned HTTP {status}"))
}

fn millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

fn slice_is_empty<T>(values: &[T]) -> bool {
    values.is_empty()
}
