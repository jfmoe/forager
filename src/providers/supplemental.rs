use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::config::WebFetchProviderConfig;
use crate::credentials::CredentialPool;
use crate::net::{RetryPolicy, error_kind_for_status, read_response_body, response_body_limit};
use crate::providers::ProviderError;
use crate::providers::execution::{self, AttemptFailure, ExecutionSettings};
use crate::providers::shared::redacted_urls_message;
use crate::redact::{Secret, redact_url, redact_urls};
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
        let outcome = execution::execute_v2(
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
        let endpoint = format!("{}/search", self.config.url.trim_end_matches('/'));
        let mut request = self
            .client
            .post(endpoint)
            .header("accept", "application/json");
        request = if self.provider == "tavily" {
            request
                .bearer_auth(credential.expose())
                .json(&TavilyRequest {
                    query,
                    max_results: limit,
                })
        } else {
            request
                .bearer_auth(credential.expose())
                .json(&FirecrawlRequest { query, limit })
        };
        let response = request.send().await.map_err(|error| AttemptFailure {
            kind: AttemptErrorKind::Network,
            status: error.status().map(|status| status.as_u16()),
            message: redacted_urls_message(&error.to_string(), &self.credentials),
            redirected_library_id: None,
        })?;
        let status = response.status();
        let body = read_response_body(response, response_body_limit(status))
            .await
            .map_err(|error| AttemptFailure {
                kind: AttemptErrorKind::Network,
                status: Some(status.as_u16()),
                message: redacted_urls_message(&error.to_string(), &self.credentials),
                redirected_library_id: None,
            })?;
        if !status.is_success() {
            return Err(AttemptFailure {
                kind: error_kind_for_status(status, &body.text),
                status: Some(status.as_u16()),
                message: redacted_urls_message(&body.text, &self.credentials),
                redirected_library_id: None,
            });
        }
        let results = if self.provider == "tavily" {
            serde_json::from_str::<TavilySearchResponse>(&body.text)
                .map(|response| response.results)
        } else {
            serde_json::from_str::<FirecrawlSearchResponse>(&body.text)
                .map(|response| response.data.web)
        }
        .map_err(|error| AttemptFailure {
            kind: AttemptErrorKind::Runtime,
            status: Some(status.as_u16()),
            message: redacted_urls_message(
                &format!("invalid {} search response: {error}", self.provider),
                &self.credentials,
            ),
            redirected_library_id: None,
        })?;
        Ok((
            status.as_u16(),
            results
                .into_iter()
                .take(usize::from(limit))
                .map(|source| Source {
                    title: self.credentials.redact(&source.title),
                    url: self.credentials.redact(&redact_url(&source.url)),
                    published_date: source
                        .published_date
                        .map(|value| self.credentials.redact(&value)),
                    author: None,
                    text: source
                        .content
                        .or(source.description)
                        .map(|value| redact_urls(&self.credentials.redact(&value))),
                    highlights: Vec::new(),
                })
                .collect(),
        ))
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
