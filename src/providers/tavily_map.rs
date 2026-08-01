use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::config::WebFetchProviderConfig;
use crate::credentials::CredentialPool;
use crate::net::{RetryPolicy, error_kind_for_status};
use crate::providers::ProviderError;
use crate::providers::execution::{AttemptFailure, ExecutionSettings, execute};
use crate::providers::shared::{redact_urls, redacted_urls_message};
use crate::redact::Secret;
use crate::types::{AttemptErrorKind, Deadline, MapOutcome};

const TAVILY_MAP_MAX_TIMEOUT_SECONDS: u64 = 180;

#[derive(Clone, Debug)]
pub(crate) struct MapRequest {
    pub(crate) url: String,
    pub(crate) instructions: String,
    pub(crate) max_depth: u16,
    pub(crate) max_breadth: u16,
    pub(crate) limit: u16,
    pub(crate) timeout_seconds: u64,
    pub(crate) verbose: bool,
}

pub(crate) struct TavilyMap {
    config: WebFetchProviderConfig,
    client: Client,
    credentials: CredentialPool,
    retry_policy: RetryPolicy,
    deadline: Deadline,
}

impl TavilyMap {
    pub(crate) fn new(
        config: WebFetchProviderConfig,
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

    pub(crate) async fn map(&self, request: MapRequest) -> Result<MapOutcome, ProviderError> {
        let request_ref = &request;
        let execution = execute(
            &self.credentials,
            ExecutionSettings {
                provider: "tavily",
                seam: "site_map",
                retry_policy: self.retry_policy,
                deadline: self.deadline,
                attempt_timeout: Duration::from_secs(self.config.timeout_seconds),
                verbose: request.verbose,
                timeout_message: "Tavily site map request timed out",
                model: None,
                transport: Some("http"),
                endpoint_host: None,
                breaker_event: None,
            },
            move |credential| async move { self.send_once(request_ref, &credential).await },
        )
        .await?;
        let response = execution.value;
        Ok(MapOutcome {
            provider: "tavily",
            url: redact_urls(&request.url, &self.credentials),
            base_url: redact_urls(&response.base_url, &self.credentials),
            results: response
                .results
                .iter()
                .map(|url| redact_urls(url, &self.credentials))
                .collect(),
            response_time: response.response_time,
            attempts: if request.verbose {
                execution.attempts
            } else {
                Vec::new()
            },
            diagnostic: execution.diagnostic,
        })
    }

    async fn send_once(
        &self,
        request: &MapRequest,
        credential: &Secret,
    ) -> Result<(u16, TavilyMapResponse), AttemptFailure> {
        let endpoint = format!("{}/map", self.config.url.trim_end_matches('/'));
        let response = self
            .client
            .post(endpoint)
            .bearer_auth(credential.expose())
            .json(&TavilyMapBody::from(request))
            .send()
            .await
            .map_err(|error| AttemptFailure {
                kind: if error.is_timeout() {
                    AttemptErrorKind::Timeout
                } else {
                    AttemptErrorKind::Network
                },
                status: error.status().map(|status| status.as_u16()),
                message: redacted_urls_message(&error.to_string(), &self.credentials),
            })?;
        let status = response.status();
        let body = response.text().await.map_err(|error| AttemptFailure {
            kind: if error.is_timeout() {
                AttemptErrorKind::Timeout
            } else {
                AttemptErrorKind::Network
            },
            status: Some(status.as_u16()),
            message: redacted_urls_message(&error.to_string(), &self.credentials),
        })?;
        if !status.is_success() {
            return Err(AttemptFailure {
                kind: error_kind_for_status(status, &body),
                status: Some(status.as_u16()),
                message: redacted_urls_message(
                    &failure_message(&body, status.as_u16()),
                    &self.credentials,
                ),
            });
        }
        serde_json::from_str(&body)
            .map(|response| (status.as_u16(), response))
            .map_err(|error| AttemptFailure {
                kind: AttemptErrorKind::Runtime,
                status: Some(status.as_u16()),
                message: redacted_urls_message(
                    &format!("invalid Tavily map response: {error}"),
                    &self.credentials,
                ),
            })
    }
}

#[derive(Serialize)]
struct TavilyMapBody<'a> {
    url: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    instructions: &'a str,
    max_depth: u16,
    max_breadth: u16,
    limit: u16,
    timeout: u64,
}

impl<'a> From<&'a MapRequest> for TavilyMapBody<'a> {
    fn from(request: &'a MapRequest) -> Self {
        Self {
            url: &request.url,
            instructions: &request.instructions,
            max_depth: request.max_depth,
            max_breadth: request.max_breadth,
            limit: request.limit,
            timeout: request.timeout_seconds.min(TAVILY_MAP_MAX_TIMEOUT_SECONDS),
        }
    }
}

#[derive(Deserialize)]
struct TavilyMapResponse {
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    results: Vec<String>,
    #[serde(default)]
    response_time: f64,
}

fn failure_message(body: &str, status: u16) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/message")
                .or_else(|| value.get("message"))
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        })
        .filter(|message| !message.trim().is_empty())
        .unwrap_or_else(|| format!("Tavily returned HTTP {status}"))
}
