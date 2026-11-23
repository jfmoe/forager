//! The HTTP Web Fetch providers behind one adapter: Jina, Tavily and Firecrawl.
//!
//! The no-wildcard rule covers every match in this module, so a new Web Fetch provider is a
//! compile error at each request, decoding and message site that must handle it.
#![deny(clippy::wildcard_enum_match_arm)]

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use reqwest::{Client, RequestBuilder};
use serde::Deserialize;
use serde_json::json;

use crate::catalog::{self, Credentials};
use crate::config::WebFetchProviderConfig;
use crate::credentials::CredentialPool;
use crate::net::{
    AttemptFailure, CONTENT_TRUNCATED_DIAGNOSTIC, RetryPolicy, combine_diagnostics,
    json_string_prefix, read_truncatable_content, send_provider_request, truncate_message,
};
use crate::providers::ProviderId;
use crate::providers::execution::{ExecutionSettings, execute_optional, execute_v2};
use crate::providers::shared::redacted_urls_message;
use crate::redact::Secret;
use crate::types::ProviderError;
use crate::types::{AttemptErrorKind, AttemptTarget, Deadline, ProviderAttempt};

#[derive(Clone)]
pub(crate) struct FetchRequest {
    pub(crate) url: String,
    pub(crate) verbose: bool,
}

/// Returns whether the URL names a PDF, for the thin-content gate.
pub(crate) fn is_pdf_url(url: &str) -> bool {
    url.split(['?', '#'])
        .next()
        .is_some_and(|path| path.to_ascii_lowercase().ends_with(".pdf"))
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
        service: Service::of(id),
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
    service: Service,
    config: WebFetchProviderConfig,
    client: Client,
    credentials: CredentialPool,
    retry_policy: RetryPolicy,
    deadline: Deadline,
}

/// The Web Fetch provider an adapter sends to.
#[derive(Clone, Copy)]
enum Service {
    Jina,
    Tavily,
    Firecrawl,
}

impl Service {
    fn of(id: ProviderId) -> Self {
        match id {
            ProviderId::Jina => Self::Jina,
            ProviderId::Tavily => Self::Tavily,
            ProviderId::Firecrawl => Self::Firecrawl,
            ProviderId::Xai
            | ProviderId::Openai
            | ProviderId::Exa
            | ProviderId::ArxivApi
            | ProviderId::SsrnCrossref
            | ProviderId::Serpapi
            | ProviderId::XiaohongshuBrowser
            | ProviderId::HnApi
            | ProviderId::YoutubeApi
            | ProviderId::YoutubeWeb
            | ProviderId::GeminiApi
            | ProviderId::PolymarketApi
            | ProviderId::RedditBrowser => {
                unreachable!("only web fetch providers make fetch requests")
            }
        }
    }

    const fn id(self) -> ProviderId {
        match self {
            Self::Jina => ProviderId::Jina,
            Self::Tavily => ProviderId::Tavily,
            Self::Firecrawl => ProviderId::Firecrawl,
        }
    }

    /// Decodes the page content of an answer `body`. When the response cap `truncated` the body,
    /// a body that no longer parses yields the content read before the cut.
    fn content(self, body: &str, truncated: bool) -> Result<String, String> {
        let decoded = match self {
            Self::Jina => serde_json::from_str::<JinaResponse>(body)
                .map_err(|error| format!("invalid Jina response: {error}"))
                .map(|response| response.data.content),
            Self::Tavily => serde_json::from_str::<TavilyResponse>(body)
                .map_err(|error| format!("invalid Tavily response: {error}"))
                .map(|response| {
                    response
                        .results
                        .into_iter()
                        .next()
                        .map(|result| result.raw_content)
                        .unwrap_or_default()
                }),
            Self::Firecrawl => serde_json::from_str::<FirecrawlResponse>(body)
                .map_err(|error| format!("invalid Firecrawl response: {error}"))
                .map(|response| response.data.markdown),
        };
        decoded.or_else(|message| {
            if truncated {
                Ok(self.truncated_content(body))
            } else {
                Err(message)
            }
        })
    }

    fn truncated_content(self, body: &str) -> String {
        let key = match self {
            Self::Jina => "content",
            Self::Tavily => "raw_content",
            Self::Firecrawl => "markdown",
        };
        json_string_prefix(body, key).unwrap_or_else(|| body.to_owned())
    }
}

struct FetchBody {
    content: String,
    truncated: bool,
}

impl HttpFetchProvider {
    async fn execute(&self, request: &FetchRequest) -> Result<ProviderFetchOutcome, ProviderError> {
        let settings = ExecutionSettings {
            provider: self.service.id().name(),
            target: AttemptTarget::seam("web_fetch"),
            retry_policy: self.retry_policy,
            deadline: self.deadline,
            attempt_timeout: Duration::from_secs(self.config.timeout_seconds),
            verbose: request.verbose,
            timeout_message: match self.service {
                Service::Jina => "jina request timed out",
                Service::Tavily => "tavily request timed out",
                Service::Firecrawl => "firecrawl request timed out",
            },
            model: None,
            transport: Some("http"),
            endpoint_host: None,
        };
        let execution = match catalog::registration(self.service.id()).credentials {
            Credentials::Optional => {
                execute_optional(&self.credentials, settings, |credential, _| async move {
                    self.send_once(request, credential.as_ref()).await
                })
                .await
            }
            Credentials::Required => {
                execute_v2(&self.credentials, settings, |credential, _| async move {
                    self.send_once(request, Some(&credential)).await
                })
                .await
            }
            Credentials::Unused => unreachable!("every web fetch provider reads keys"),
        }?;
        Ok(ProviderFetchOutcome {
            provider: self.service.id().name(),
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

    /// Sends one request; `credential` is `None` for an anonymous request.
    async fn send_once(
        &self,
        request: &FetchRequest,
        credential: Option<&Secret>,
    ) -> Result<(Option<u16>, FetchBody), AttemptFailure> {
        let request_builder = self.request(request, credential);
        let response = send_provider_request(request_builder, &self.credentials).await?;
        let body = read_truncatable_content(response, &self.credentials, failure_message).await?;
        let status = body.status;
        self.service
            .content(&body.text, body.truncated)
            .map(|content| {
                (
                    Some(status),
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

    fn request(&self, request: &FetchRequest, credential: Option<&Secret>) -> RequestBuilder {
        let url = &request.url;
        match self.service {
            Service::Jina => {
                let endpoint = format!("{}/{}", self.config.url.trim_end_matches('/'), url);
                let mut request = authorized(self.client.get(endpoint), credential)
                    .header("x-return-format", "markdown")
                    .header("accept", "application/json");
                if !self.config.respond_with.is_empty() {
                    request = request.header("x-respond-with", &self.config.respond_with);
                }
                request
            }
            Service::Tavily => authorized(
                self.client
                    .post(format!("{}/extract", self.config.url.trim_end_matches('/'))),
                credential,
            )
            .json(&json!({
                "urls": [url],
                "format": "markdown",
                "extract_depth": "basic"
            })),
            Service::Firecrawl => authorized(
                self.client
                    .post(format!("{}/scrape", self.config.url.trim_end_matches('/'))),
                credential,
            )
            .json(&json!({
                "url": url,
                "formats": ["markdown"],
                "onlyMainContent": true,
                "timeout": 60000,
                // The PDF parser options Firecrawl applies in scrape (ADR-0029).
                "parsers": [{"type": "pdf", "mode": "auto", "pageMarkers": true}],
            })),
        }
    }

    fn redacted_error(&self, value: &str) -> String {
        redacted_urls_message(value, &self.credentials)
    }
}

/// Adds the credential as a bearer token; an anonymous request sends no `Authorization`.
fn authorized(request: RequestBuilder, credential: Option<&Secret>) -> RequestBuilder {
    match credential {
        Some(credential) => request.bearer_auth(credential.expose()),
        None => request,
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
                .or_else(|| value.get("error"))
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        })
        .filter(|message| !message.trim().is_empty())
        .unwrap_or_else(|| format!("HTTP {status}"));
    truncate_message(&message)
}

#[cfg(test)]
#[path = "web_fetch_tests.rs"]
mod tests;
