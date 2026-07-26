use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::config::{self, WebFetchProviderConfig};
use crate::credentials::CredentialPool;
use crate::net::{RetryPolicy, error_kind_for_status, truncate_message};
use crate::providers::ProviderError;
use crate::providers::execution::{AttemptFailure, ExecutionSettings, execute};
use crate::types::{AttemptErrorKind, Deadline, MapOutcome};

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
            },
            move |credential| async move { self.send_once(request_ref, &credential).await },
        )
        .await?;
        let response = execution.value;
        Ok(MapOutcome {
            provider: "tavily",
            url: redact(&request.url, &self.config, &self.credentials),
            base_url: redact(&response.base_url, &self.config, &self.credentials),
            results: response
                .results
                .iter()
                .map(|url| redact(url, &self.config, &self.credentials))
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
        credential: &str,
    ) -> Result<(u16, TavilyMapResponse), AttemptFailure> {
        let endpoint = format!("{}/map", self.config.url.trim_end_matches('/'));
        let response = self
            .client
            .post(endpoint)
            .bearer_auth(credential)
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
        serde_json::from_str(&body)
            .map(|response| (status.as_u16(), response))
            .map_err(|error| AttemptFailure {
                kind: AttemptErrorKind::Runtime,
                status: Some(status.as_u16()),
                message: self.redacted_message(&format!("invalid Tavily map response: {error}")),
            })
    }

    fn redacted_message(&self, message: &str) -> String {
        truncate_message(&redact(message, &self.config, &self.credentials))
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
            timeout: request.timeout_seconds,
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

fn redact(value: &str, config: &WebFetchProviderConfig, credentials: &CredentialPool) -> String {
    let redacted_endpoint = config::redact_url(&config.url);
    config::redact_urls(
        &credentials
            .redact(value)
            .replace(&config.url, &redacted_endpoint),
    )
}
