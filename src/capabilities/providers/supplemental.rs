use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::config::WebFetchProviderConfig;
use crate::credentials::CredentialPool;
use crate::net::{AttemptFailure, RetryPolicy, read_complete_protocol, send_provider_request};
use crate::providers::execution::{self, ExecutionSettings};
use crate::providers::shared::redacted_urls_message;
use crate::providers::{ProviderError, ProviderId};
use crate::redact::{Secret, redact_url, redact_urls};
use crate::types::{AttemptErrorKind, AttemptTarget, Deadline, Source, SupplementalSearchOutcome};

pub(crate) struct SupplementalSearch {
    provider: ProviderId,
    config: WebFetchProviderConfig,
    client: Client,
    credentials: CredentialPool,
    retry_policy: RetryPolicy,
    deadline: Deadline,
}

impl SupplementalSearch {
    pub(crate) fn new(
        provider: ProviderId,
        config: WebFetchProviderConfig,
        client: Client,
        credentials: CredentialPool,
        retry_policy: RetryPolicy,
        deadline: Deadline,
    ) -> Self {
        Self {
            provider,
            config,
            client,
            credentials,
            retry_policy,
            deadline,
        }
    }

    pub(crate) async fn search(
        &self,
        query: &str,
        limit: u16,
    ) -> Result<SupplementalSearchOutcome, ProviderError> {
        let outcome = execution::execute_v2(
            &self.credentials,
            ExecutionSettings {
                provider: self.provider.name(),
                target: AttemptTarget::seam("web_search"),
                retry_policy: self.retry_policy,
                deadline: self.deadline,
                attempt_timeout: Duration::from_secs(self.config.timeout_seconds),
                verbose: false,
                timeout_message: "supplemental search timed out",
                model: None,
                transport: Some("http"),
                endpoint_host: None,
                breaker_event: None,
            },
            |credential, _| self.send_once(query, limit, credential),
        )
        .await?;
        Ok(SupplementalSearchOutcome {
            sources: outcome.value,
            attempts: outcome.attempts,
            diagnostic: outcome.diagnostic,
        })
    }

    async fn send_once(
        &self,
        query: &str,
        limit: u16,
        credential: Secret,
    ) -> Result<(u16, Vec<Source>), AttemptFailure> {
        let provider = self.provider.name();
        let endpoint = format!("{}/search", self.config.url.trim_end_matches('/'));
        let mut request = self
            .client
            .post(endpoint)
            .header("accept", "application/json");
        request = if self.provider == ProviderId::Tavily {
            request
                .bearer_auth(credential.expose())
                .json(&TavilyRequest {
                    query,
                    max_results: limit,
                    search_depth: "advanced",
                    include_raw_content: false,
                    include_answer: false,
                })
        } else {
            request
                .bearer_auth(credential.expose())
                .json(&FirecrawlRequest { query, limit })
        };
        let response = send_provider_request(request, &self.credentials).await?;
        let body = read_complete_protocol(response, &self.credentials, raw_error_body).await?;
        let status = body.status;
        let results = if self.provider == ProviderId::Tavily {
            serde_json::from_str::<TavilySearchResponse>(&body.text)
                .map(|response| response.results)
        } else {
            serde_json::from_str::<FirecrawlSearchResponse>(&body.text)
                .map(|response| response.data.web)
        }
        .map_err(|error| AttemptFailure {
            kind: AttemptErrorKind::Runtime,
            status: Some(status),
            message: redacted_urls_message(
                &format!("invalid {provider} search response: {error}"),
                &self.credentials,
            ),
        })?;
        let sources = self.normalize_sources(results, limit, status)?;
        Ok((status, sources))
    }

    fn normalize_sources(
        &self,
        results: Vec<SearchResult>,
        limit: u16,
        status: u16,
    ) -> Result<Vec<Source>, AttemptFailure> {
        let results_were_empty = results.is_empty();
        let mut sources = Vec::new();
        let result_limit = usize::from(limit);
        for source in results {
            if sources.len() == result_limit {
                break;
            }
            let Some(url) = source.url.map(|url| url.trim().to_owned()) else {
                continue;
            };
            if url.is_empty() {
                continue;
            }
            if !is_http_url(&url) {
                return Err(AttemptFailure {
                    kind: AttemptErrorKind::Runtime,
                    status: Some(status),
                    message: format!(
                        "invalid {} search response: result URL must use HTTP(S)",
                        self.provider.name()
                    ),
                });
            }
            sources.push(Source {
                title: self.credentials.redact(&source.title),
                url: self.credentials.redact(&redact_url(&url)),
                published_date: source
                    .published_date
                    .map(|value| self.credentials.redact(&value)),
                author: source.author.map(|value| self.credentials.redact(&value)),
                text: source
                    .content
                    .or(source.description)
                    .map(|value| redact_urls(&self.credentials.redact(&value))),
                highlights: Vec::new(),
                id: None,
                image: None,
                favicon: None,
            });
        }
        if !results_were_empty && sources.is_empty() {
            return Err(AttemptFailure {
                kind: AttemptErrorKind::Evidence,
                status: Some(status),
                message: format!(
                    "{} search returned no valid candidate",
                    self.provider.name()
                ),
            });
        }
        Ok(sources)
    }
}

fn is_http_url(value: &str) -> bool {
    reqwest::Url::parse(value)
        .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
}

#[derive(Serialize)]
struct TavilyRequest<'a> {
    query: &'a str,
    max_results: u16,
    search_depth: &'static str,
    include_raw_content: bool,
    include_answer: bool,
}

#[derive(Serialize)]
struct FirecrawlRequest<'a> {
    query: &'a str,
    limit: u16,
}

#[derive(Deserialize)]
struct TavilySearchResponse {
    results: Vec<SearchResult>,
}

#[derive(Deserialize)]
struct FirecrawlSearchResponse {
    data: FirecrawlSearchData,
}

#[derive(Deserialize)]
struct FirecrawlSearchData {
    web: Vec<SearchResult>,
}

#[derive(Deserialize)]
struct SearchResult {
    #[serde(default)]
    title: String,
    url: Option<String>,
    content: Option<String>,
    description: Option<String>,
    #[serde(rename = "publishedDate", alias = "published_date")]
    published_date: Option<String>,
    author: Option<String>,
}

fn raw_error_body(body: &str, _status: u16) -> String {
    body.to_owned()
}
