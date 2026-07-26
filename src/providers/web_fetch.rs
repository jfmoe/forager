use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};

use reqwest::{Client, RequestBuilder};
use serde::Deserialize;
use serde_json::json;

use crate::config::{self, WebFetchProviderConfig};
use crate::credentials::CredentialPool;
use crate::net::{RetryPolicy, error_kind_for_status, truncate_message};
use crate::providers::{ProviderError, ProviderId};
use crate::types::{AttemptErrorKind, Deadline, ProviderAttempt};

pub(crate) struct FetchRequest {
    pub(crate) url: String,
    pub(crate) verbose: bool,
}

pub(crate) struct ProviderFetchOutcome {
    pub(crate) provider: &'static str,
    pub(crate) content: String,
    pub(crate) attempts: Vec<ProviderAttempt>,
    pub(crate) diagnostic: Option<String>,
}

pub(crate) trait WebFetch: Send + Sync {
    fn fetch<'a>(
        &'a self,
        request: &'a FetchRequest,
    ) -> Pin<Box<dyn Future<Output = Result<ProviderFetchOutcome, ProviderError>> + Send + 'a>>;
}

pub(crate) fn jina(
    config: WebFetchProviderConfig,
    client: Client,
    credentials: CredentialPool,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Box<dyn WebFetch> {
    Box::new(Jina(HttpFetchProvider {
        id: ProviderId::Jina,
        config,
        client,
        credentials,
        retry_policy,
        deadline,
    }))
}

pub(crate) fn tavily(
    config: WebFetchProviderConfig,
    client: Client,
    credentials: CredentialPool,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Box<dyn WebFetch> {
    Box::new(Tavily(HttpFetchProvider {
        id: ProviderId::Tavily,
        config,
        client,
        credentials,
        retry_policy,
        deadline,
    }))
}

pub(crate) fn firecrawl(
    config: WebFetchProviderConfig,
    client: Client,
    credentials: CredentialPool,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Box<dyn WebFetch> {
    Box::new(Firecrawl(HttpFetchProvider {
        id: ProviderId::Firecrawl,
        config,
        client,
        credentials,
        retry_policy,
        deadline,
    }))
}

struct Jina(HttpFetchProvider);
struct Tavily(HttpFetchProvider);
struct Firecrawl(HttpFetchProvider);

impl WebFetch for Jina {
    fn fetch<'a>(
        &'a self,
        request: &'a FetchRequest,
    ) -> Pin<Box<dyn Future<Output = Result<ProviderFetchOutcome, ProviderError>> + Send + 'a>>
    {
        Box::pin(self.0.execute(request))
    }
}

impl WebFetch for Tavily {
    fn fetch<'a>(
        &'a self,
        request: &'a FetchRequest,
    ) -> Pin<Box<dyn Future<Output = Result<ProviderFetchOutcome, ProviderError>> + Send + 'a>>
    {
        Box::pin(self.0.execute(request))
    }
}

impl WebFetch for Firecrawl {
    fn fetch<'a>(
        &'a self,
        request: &'a FetchRequest,
    ) -> Pin<Box<dyn Future<Output = Result<ProviderFetchOutcome, ProviderError>> + Send + 'a>>
    {
        Box::pin(self.0.execute(request))
    }
}

struct HttpFetchProvider {
    id: ProviderId,
    config: WebFetchProviderConfig,
    client: Client,
    credentials: CredentialPool,
    retry_policy: RetryPolicy,
    deadline: Deadline,
}

impl HttpFetchProvider {
    async fn execute(&self, request: &FetchRequest) -> Result<ProviderFetchOutcome, ProviderError> {
        let selection = self.credentials.claim();
        let mut attempts = Vec::new();
        let mut credential_index = selection.index;
        let mut retry_count = 0;
        let mut rotation_count = 0;

        loop {
            let Some(remaining) = self.deadline.remaining() else {
                return Err(self.terminal_error(
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
                self.send_once(request, self.credentials.key(credential_index)),
            )
            .await;
            match response {
                Ok(Ok(outcome)) => {
                    attempts.push(ProviderAttempt {
                        provider: self.name(),
                        seam: "web_fetch",
                        error_kind: None,
                        http_status: Some(outcome.status),
                        duration_ms: millis(started.elapsed()),
                        credential_index,
                        retry_count,
                        rotation_count,
                        message: String::new(),
                    });
                    return Ok(ProviderFetchOutcome {
                        provider: self.name(),
                        content: self.redacted_text(&outcome.content),
                        attempts,
                        diagnostic: selection.diagnostic,
                    });
                }
                Ok(Err(failure)) => {
                    let kind = failure.kind;
                    attempts.push(ProviderAttempt {
                        provider: self.name(),
                        seam: "web_fetch",
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
                        if self
                            .deadline
                            .remaining()
                            .is_none_or(|remaining| wait >= remaining)
                        {
                            return Err(self.terminal_error(
                                AttemptErrorKind::Timeout,
                                attempts,
                                request.verbose,
                                selection.diagnostic.clone(),
                            ));
                        }
                        tokio::time::sleep(wait).await;
                        continue;
                    }
                    return Err(self.terminal_error(
                        kind,
                        attempts,
                        request.verbose,
                        selection.diagnostic.clone(),
                    ));
                }
                Err(_) => {
                    attempts.push(ProviderAttempt {
                        provider: self.name(),
                        seam: "web_fetch",
                        error_kind: Some(AttemptErrorKind::Timeout),
                        http_status: None,
                        duration_ms: millis(started.elapsed()),
                        credential_index,
                        retry_count,
                        rotation_count,
                        message: format!("{} request timed out", self.name()),
                    });
                    if attempts.len() < self.retry_policy.max_attempts() {
                        retry_count += 1;
                        let wait = self.retry_policy.wait(retry_count);
                        if self
                            .deadline
                            .remaining()
                            .is_some_and(|remaining| wait < remaining)
                        {
                            tokio::time::sleep(wait).await;
                            continue;
                        }
                    }
                    return Err(self.terminal_error(
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
        request: &FetchRequest,
        credential: &str,
    ) -> Result<FetchResponse, AttemptFailure> {
        let request_builder = self.request(request, credential);
        let response = request_builder
            .send()
            .await
            .map_err(|error| AttemptFailure {
                kind: if error.is_timeout() {
                    AttemptErrorKind::Timeout
                } else {
                    AttemptErrorKind::Network
                },
                status: error.status().map(|status| status.as_u16()),
                message: self.redacted_error(&error.to_string()),
            })?;
        let status = response.status();
        let body = response.text().await.map_err(|error| AttemptFailure {
            kind: if error.is_timeout() {
                AttemptErrorKind::Timeout
            } else {
                AttemptErrorKind::Network
            },
            status: Some(status.as_u16()),
            message: self.redacted_error(&error.to_string()),
        })?;
        if !status.is_success() {
            return Err(AttemptFailure {
                kind: error_kind_for_status(status, &body),
                status: Some(status.as_u16()),
                message: self.redacted_error(&failure_message(&body, status.as_u16())),
            });
        }
        self.decode(&body)
            .map(|content| FetchResponse {
                status: status.as_u16(),
                content,
            })
            .map_err(|message| AttemptFailure {
                kind: AttemptErrorKind::Runtime,
                status: Some(status.as_u16()),
                message: self.redacted_error(&message),
            })
    }

    fn request(&self, request: &FetchRequest, credential: &str) -> RequestBuilder {
        match self.id {
            ProviderId::Jina => {
                let endpoint = format!("{}/{}", self.config.url.trim_end_matches('/'), request.url);
                let mut request = self
                    .client
                    .get(endpoint)
                    .bearer_auth(credential)
                    .header("x-return-format", "markdown")
                    .header("accept", "text/plain, text/markdown, */*");
                if !self.config.respond_with.is_empty() {
                    request = request.header("x-respond-with", &self.config.respond_with);
                }
                request
            }
            ProviderId::Tavily => self
                .client
                .post(format!("{}/extract", self.config.url.trim_end_matches('/')))
                .bearer_auth(credential)
                .json(&json!({"urls": [&request.url], "format": "markdown"})),
            ProviderId::Firecrawl => self
                .client
                .post(format!("{}/scrape", self.config.url.trim_end_matches('/')))
                .bearer_auth(credential)
                .json(&json!({
                    "url": &request.url,
                    "formats": ["markdown"],
                    "timeout": 60000
                })),
            _ => unreachable!("only web fetch providers make fetch requests"),
        }
    }

    fn decode(&self, body: &str) -> Result<String, String> {
        match self.id {
            ProviderId::Jina => Ok(body.trim().to_owned()),
            ProviderId::Tavily => serde_json::from_str::<TavilyResponse>(body)
                .map_err(|error| format!("invalid Tavily response: {error}"))
                .map(|response| {
                    response
                        .results
                        .into_iter()
                        .next()
                        .map(|result| result.raw_content)
                        .unwrap_or_default()
                }),
            ProviderId::Firecrawl => serde_json::from_str::<FirecrawlResponse>(body)
                .map_err(|error| format!("invalid Firecrawl response: {error}"))
                .map(|response| response.data.markdown),
            _ => unreachable!("only web fetch providers decode fetch responses"),
        }
    }

    fn name(&self) -> &'static str {
        match self.id {
            ProviderId::Jina => "jina",
            ProviderId::Tavily => "tavily",
            ProviderId::Firecrawl => "firecrawl",
            _ => unreachable!("only web fetch providers have fetch names"),
        }
    }

    fn redacted_text(&self, value: &str) -> String {
        let redacted_endpoint = config::redact_url(&self.config.url);
        config::redact_urls(
            &self
                .credentials
                .redact(value)
                .replace(&self.config.url, &redacted_endpoint),
        )
    }

    fn redacted_error(&self, value: &str) -> String {
        truncate_message(&self.redacted_text(value))
    }

    fn terminal_error(
        &self,
        kind: AttemptErrorKind,
        attempts: Vec<ProviderAttempt>,
        verbose: bool,
        diagnostic: Option<String>,
    ) -> ProviderError {
        let message = attempts
            .last()
            .map(|attempt| attempt.message.clone())
            .unwrap_or_else(|| format!("{} request failed", self.name()));
        ProviderError {
            kind,
            message,
            attempts,
            verbose,
            diagnostic,
            redirected_library_id: None,
        }
    }
}

#[derive(Deserialize)]
struct TavilyResponse {
    #[serde(default)]
    results: Vec<TavilyResult>,
}

#[derive(Deserialize)]
struct TavilyResult {
    #[serde(default)]
    raw_content: String,
}

#[derive(Deserialize)]
struct FirecrawlResponse {
    #[serde(default)]
    data: FirecrawlData,
}

#[derive(Default, Deserialize)]
struct FirecrawlData {
    #[serde(default)]
    markdown: String,
}

struct AttemptFailure {
    kind: AttemptErrorKind,
    status: Option<u16>,
    message: String,
}

struct FetchResponse {
    status: u16,
    content: String,
}

fn failure_message(body: &str, status: u16) -> String {
    let message = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/message")
                .or_else(|| value.get("message"))
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        })
        .filter(|message| !message.trim().is_empty())
        .unwrap_or_else(|| format!("HTTP {status}"));
    truncate_message(&message)
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
