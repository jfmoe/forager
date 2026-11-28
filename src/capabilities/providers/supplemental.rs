use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::config::WebFetchProviderConfig;
use crate::credentials::CredentialPool;
use crate::net::{AttemptFailure, RetryPolicy, read_complete_protocol, send_provider_request};
use crate::providers::ProviderId;
use crate::providers::execution::{self, ExecutionSettings};
use crate::providers::shared::{is_http_url, redacted_urls_message};
use crate::redact::{Secret, redact_url, redact_urls};
use crate::types::ProviderError;
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
                timeout_message: "supplemental search timed out",
                model: None,
                transport: Some("http"),
                endpoint_host: None,
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
    ) -> Result<(Option<u16>, Vec<Source>), AttemptFailure> {
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
                    chunks_per_source: 1,
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
        let sources = decode_sources(self.provider, &body.text, limit, status, &self.credentials)?;
        Ok((Some(status), sources))
    }
}

/// Decodes a successful answer `body` of `provider` into at most `limit` sources, with their
/// secrets redacted as the outcome shows them. A body whose results all lack an HTTP(S) URL is
/// an evidence failure, unlike a body without results.
fn decode_sources(
    provider: ProviderId,
    body: &str,
    limit: u16,
    status: u16,
    credentials: &CredentialPool,
) -> Result<Vec<Source>, AttemptFailure> {
    let name = provider.name();
    let results = if provider == ProviderId::Tavily {
        serde_json::from_str::<TavilySearchResponse>(body).map(|response| response.results)
    } else {
        serde_json::from_str::<FirecrawlSearchResponse>(body).map(|response| response.data.web)
    }
    .map_err(|error| AttemptFailure {
        kind: AttemptErrorKind::Runtime,
        status: Some(status),
        message: redacted_urls_message(
            &format!("invalid {name} search response: {error}"),
            credentials,
        ),
    })?;
    let results_were_empty = results.is_empty();
    let mut sources = Vec::new();
    let result_limit = usize::from(limit);
    for source in results {
        if sources.len() == result_limit {
            break;
        }
        let Some(url) = source
            .url
            .map(|url| url.trim().to_owned())
            .filter(|url| is_http_url(url))
        else {
            continue;
        };
        sources.push(Source {
            title: credentials.redact(&source.title),
            url: credentials.redact(&redact_url(&url)),
            published_date: source
                .published_date
                .map(|value| credentials.redact(&value)),
            author: source.author.map(|value| credentials.redact(&value)),
            text: source
                .content
                .or(source.description)
                .map(|value| redact_urls(&credentials.redact(&value))),
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
            message: format!("{name} search returned no valid candidate"),
        });
    }
    Ok(sources)
}

#[derive(Serialize)]
struct TavilyRequest<'a> {
    query: &'a str,
    max_results: u16,
    search_depth: &'static str,
    chunks_per_source: u8,
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

#[cfg(test)]
#[path = "supplemental_tests.rs"]
mod tests;
