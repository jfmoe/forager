use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::config::{self, WebFetchProviderConfig};
use crate::credentials::CredentialPool;
use crate::net::{RetryPolicy, error_kind_for_status, truncate_message};
use crate::providers::ProviderError;
use crate::providers::execution::{self, AttemptFailure, ExecutionSettings};
use crate::types::{AttemptErrorKind, Deadline, Source, SupplementalSearchOutcome};

pub(crate) struct SupplementalSearch {
    provider: &'static str,
    config: WebFetchProviderConfig,
    client: Client,
    credentials: CredentialPool,
    retry_policy: RetryPolicy,
    deadline: Deadline,
}

impl SupplementalSearch {
    pub(crate) fn new(
        provider: &'static str,
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
        let outcome = execution::execute(
            &self.credentials,
            ExecutionSettings {
                provider: self.provider,
                seam: "web_search",
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
            |credential| self.send_once(query, limit, credential),
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
        credential: String,
    ) -> Result<(u16, Vec<Source>), AttemptFailure> {
        let endpoint = format!("{}/search", self.config.url.trim_end_matches('/'));
        let mut request = self
            .client
            .post(endpoint)
            .header("accept", "application/json");
        request = if self.provider == "tavily" {
            request
                .header("authorization", format!("Bearer {credential}"))
                .json(&TavilyRequest {
                    query,
                    max_results: limit,
                })
        } else {
            request
                .header("authorization", format!("Bearer {credential}"))
                .json(&FirecrawlRequest { query, limit })
        };
        let response = request.send().await.map_err(|error| AttemptFailure {
            kind: if error.is_timeout() {
                AttemptErrorKind::Timeout
            } else {
                AttemptErrorKind::Network
            },
            status: error.status().map(|status| status.as_u16()),
            message: self.redact(&error.to_string()),
        })?;
        let status = response.status();
        let body = response.text().await.map_err(|error| AttemptFailure {
            kind: AttemptErrorKind::Network,
            status: Some(status.as_u16()),
            message: self.redact(&error.to_string()),
        })?;
        if !status.is_success() {
            return Err(AttemptFailure {
                kind: error_kind_for_status(status, &body),
                status: Some(status.as_u16()),
                message: self.redact(&body),
            });
        }
        let results = if self.provider == "tavily" {
            serde_json::from_str::<TavilySearchResponse>(&body).map(|response| response.results)
        } else {
            serde_json::from_str::<FirecrawlSearchResponse>(&body).map(|response| response.data.web)
        }
        .map_err(|error| AttemptFailure {
            kind: AttemptErrorKind::Runtime,
            status: Some(status.as_u16()),
            message: self.redact(&format!(
                "invalid {} search response: {error}",
                self.provider
            )),
        })?;
        Ok((
            status.as_u16(),
            results
                .into_iter()
                .take(usize::from(limit))
                .map(|source| Source {
                    title: self.credentials.redact(&source.title),
                    url: self.credentials.redact(&config::redact_url(&source.url)),
                    published_date: source
                        .published_date
                        .map(|value| self.credentials.redact(&value)),
                    author: None,
                    text: source
                        .content
                        .or(source.description)
                        .map(|value| config::redact_urls(&self.credentials.redact(&value))),
                    highlights: Vec::new(),
                })
                .collect(),
        ))
    }

    fn redact(&self, message: &str) -> String {
        truncate_message(
            &self
                .credentials
                .redact(message)
                .replace(&self.config.url, &config::redact_url(&self.config.url)),
        )
    }
}

#[derive(Serialize)]
struct TavilyRequest<'a> {
    query: &'a str,
    max_results: u16,
}

#[derive(Serialize)]
struct FirecrawlRequest<'a> {
    query: &'a str,
    limit: u16,
}

#[derive(Deserialize)]
struct TavilySearchResponse {
    #[serde(default)]
    results: Vec<SearchResult>,
}

#[derive(Deserialize)]
struct FirecrawlSearchResponse {
    data: FirecrawlSearchData,
}

#[derive(Deserialize)]
struct FirecrawlSearchData {
    #[serde(default)]
    web: Vec<SearchResult>,
}

#[derive(Deserialize)]
struct SearchResult {
    #[serde(default)]
    title: String,
    url: String,
    content: Option<String>,
    description: Option<String>,
    #[serde(rename = "publishedDate")]
    published_date: Option<String>,
}
