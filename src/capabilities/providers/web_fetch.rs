use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use reqwest::{Client, RequestBuilder};
use serde::Deserialize;
use serde_json::json;

use crate::config::WebFetchProviderConfig;
use crate::credentials::CredentialPool;
use crate::net::{
    AttemptFailure, CONTENT_TRUNCATED_DIAGNOSTIC, RetryPolicy, combine_diagnostics,
    json_string_prefix, read_truncatable_content, send_provider_request, truncate_message,
};
use crate::providers::execution::{ExecutionSettings, execute_v2};
use crate::providers::shared::redacted_urls_message;
use crate::providers::{ProviderError, ProviderId};
use crate::redact::Secret;
use crate::types::{AttemptErrorKind, AttemptTarget, Deadline, ProviderAttempt};

#[derive(Clone)]
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

pub(crate) fn new(
    id: ProviderId,
    config: WebFetchProviderConfig,
    client: Client,
    credentials: CredentialPool,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Box<dyn WebFetch> {
    Box::new(HttpFetchProvider {
        id,
        config,
        client,
        credentials,
        retry_policy,
        deadline,
    })
}

impl WebFetch for HttpFetchProvider {
    fn fetch<'a>(
        &'a self,
        request: &'a FetchRequest,
    ) -> Pin<Box<dyn Future<Output = Result<ProviderFetchOutcome, ProviderError>> + Send + 'a>>
    {
        Box::pin(self.execute(request))
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

struct FetchBody {
    content: String,
    truncated: bool,
}

impl HttpFetchProvider {
    async fn execute(&self, request: &FetchRequest) -> Result<ProviderFetchOutcome, ProviderError> {
        let execution = execute_v2(
            &self.credentials,
            ExecutionSettings {
                provider: self.id.name(),
                target: AttemptTarget::seam("web_fetch"),
                retry_policy: self.retry_policy,
                deadline: self.deadline,
                attempt_timeout: Duration::from_secs(self.config.timeout_seconds),
                verbose: request.verbose,
                timeout_message: match self.id {
                    ProviderId::Jina => "jina request timed out",
                    ProviderId::Tavily => "tavily request timed out",
                    ProviderId::Firecrawl => "firecrawl request timed out",
                    _ => unreachable!("only web fetch providers make fetch requests"),
                },
                model: None,
                transport: Some("http"),
                endpoint_host: None,
                breaker_event: None,
            },
            |credential, _| async move { self.send_once(request, &credential).await },
        )
        .await?;
        Ok(ProviderFetchOutcome {
            provider: self.id.name(),
            content: execution.value.content,
            attempts: execution.attempts,
            diagnostic: combine_diagnostics(
                [
                    execution.diagnostic,
                    execution
                        .value
                        .truncated
                        .then(|| CONTENT_TRUNCATED_DIAGNOSTIC.to_owned()),
                ]
                .into_iter()
                .flatten(),
            ),
        })
    }

    async fn send_once(
        &self,
        request: &FetchRequest,
        credential: &Secret,
    ) -> Result<(u16, FetchBody), AttemptFailure> {
        let request_builder = self.request(request, credential);
        let response = send_provider_request(request_builder, &self.credentials).await?;
        let body = read_truncatable_content(response, &self.credentials, failure_message).await?;
        let status = body.status;
        self.decode(&body.text)
            .or_else(|message| {
                if body.truncated {
                    Ok(self.decode_truncated(&body.text))
                } else {
                    Err(message)
                }
            })
            .map(|content| {
                (
                    status,
                    FetchBody {
                        content,
                        truncated: body.truncated,
                    },
                )
            })
            .map_err(|message| AttemptFailure {
                kind: AttemptErrorKind::Runtime,
                status: Some(status),
                message: self.redacted_error(&message),
            })
    }

    fn request(&self, request: &FetchRequest, credential: &Secret) -> RequestBuilder {
        match self.id {
            ProviderId::Jina => {
                let endpoint = format!("{}/{}", self.config.url.trim_end_matches('/'), request.url);
                let mut request = self
                    .client
                    .get(endpoint)
                    .bearer_auth(credential.expose())
                    .header("x-return-format", "markdown")
                    .header("accept", "application/json");
                if !self.config.respond_with.is_empty() {
                    request = request.header("x-respond-with", &self.config.respond_with);
                }
                request
            }
            ProviderId::Tavily => self
                .client
                .post(format!("{}/extract", self.config.url.trim_end_matches('/')))
                .bearer_auth(credential.expose())
                .json(&json!({
                    "urls": [&request.url],
                    "format": "markdown",
                    "extract_depth": "basic"
                })),
            ProviderId::Firecrawl => self
                .client
                .post(format!("{}/scrape", self.config.url.trim_end_matches('/')))
                .bearer_auth(credential.expose())
                .json(&json!({
                    "url": &request.url,
                    "formats": ["markdown"],
                    "onlyMainContent": true,
                    "timeout": 60000
                })),
            _ => unreachable!("only web fetch providers make fetch requests"),
        }
    }

    fn decode(&self, body: &str) -> Result<String, String> {
        match self.id {
            ProviderId::Jina => serde_json::from_str::<JinaResponse>(body)
                .map_err(|error| format!("invalid Jina response: {error}"))
                .map(|response| response.data.content),
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

    fn decode_truncated(&self, body: &str) -> String {
        match self.id {
            ProviderId::Jina => {
                json_string_prefix(body, "content").unwrap_or_else(|| body.to_owned())
            }
            ProviderId::Tavily => {
                json_string_prefix(body, "raw_content").unwrap_or_else(|| body.to_owned())
            }
            ProviderId::Firecrawl => {
                json_string_prefix(body, "markdown").unwrap_or_else(|| body.to_owned())
            }
            _ => unreachable!("only web fetch providers decode fetch responses"),
        }
    }

    fn redacted_error(&self, value: &str) -> String {
        redacted_urls_message(value, &self.credentials)
    }
}

#[derive(Deserialize)]
struct JinaResponse {
    data: JinaData,
}

#[derive(Deserialize)]
struct JinaData {
    content: String,
}

#[derive(Deserialize)]
struct TavilyResponse {
    results: Vec<TavilyResult>,
}

#[derive(Deserialize)]
struct TavilyResult {
    raw_content: String,
}

#[derive(Deserialize)]
struct FirecrawlResponse {
    data: FirecrawlData,
}

#[derive(Deserialize)]
struct FirecrawlData {
    markdown: String,
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
