use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::config::ExaRuntimeConfig;
use crate::credentials::CredentialPool;
use crate::net::{ResponseBodyPolicy, RetryPolicy, error_kind_for_status, read_response_body};
use crate::providers::ProviderError;
use crate::providers::execution::{AttemptFailure, ExecutionSettings, execute_v2};
use crate::providers::shared::redacted_urls_message;
use crate::redact::{Secret, redact_url};
use crate::types::{AttemptErrorKind, AttemptTarget, Deadline, ExaInput, ExaOutcome, Source};

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

#[derive(Clone, Debug)]
pub(crate) struct ExaSimilarRequest {
    pub(crate) url: String,
    pub(crate) num_results: u16,
    pub(crate) verbose: bool,
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
    ) -> Result<ExaOutcome, ProviderError> {
        self.execute(ExaOperation::Search(request)).await
    }

    pub(crate) async fn similar(
        &self,
        request: ExaSimilarRequest,
    ) -> Result<ExaOutcome, ProviderError> {
        self.execute(ExaOperation::Similar(request)).await
    }

    async fn execute(&self, operation: ExaOperation) -> Result<ExaOutcome, ProviderError> {
        let operation_ref = &operation;
        let execution = execute_v2(
            &self.credentials,
            ExecutionSettings {
                provider: "exa",
                target: operation.target(),
                retry_policy: self.retry_policy,
                deadline: self.deadline,
                attempt_timeout: Duration::from_secs(self.config.timeout_seconds),
                verbose: operation.verbose(),
                timeout_message: "Exa request timed out",
                model: None,
                transport: Some("http"),
                endpoint_host: None,
                breaker_event: None,
            },
            |credential, _| async move {
                self.send_once(operation_ref, &credential)
                    .await
                    .map(|value| (200, value))
            },
        )
        .await?;
        Ok(ExaOutcome {
            provider: "exa",
            input: operation.output_input(),
            results: execution.value,
            attempts: if operation.verbose() {
                execution.attempts
            } else {
                Vec::new()
            },
            diagnostic: execution.diagnostic,
        })
    }

    async fn send_once(
        &self,
        operation: &ExaOperation,
        key: &Secret,
    ) -> Result<Vec<Source>, AttemptFailure> {
        let endpoint = format!(
            "{}/{}",
            self.config.url.trim_end_matches('/'),
            operation.endpoint()
        );
        let request = self
            .client
            .post(endpoint)
            .header("x-api-key", key.expose())
            .header("accept", "application/json");
        let request = match operation {
            ExaOperation::Search(search) => request.json(&ExaSearchBody::from(search)),
            ExaOperation::Similar(similar) => request.json(&ExaSimilarBody::from(similar)),
        };
        let response = request.send().await.map_err(|error| AttemptFailure {
            kind: AttemptErrorKind::Network,
            status: error.status().map(|status| status.as_u16()),
            message: redacted_urls_message(&error.to_string(), &self.credentials),
            redirected_library_id: None,
        })?;
        let status = response.status();
        let body = read_response_body(
            response,
            ResponseBodyPolicy::for_status(status, ResponseBodyPolicy::CompleteProtocol),
        )
        .await
        .map_err(|error| AttemptFailure {
            kind: error.attempt_error_kind(),
            status: Some(status.as_u16()),
            message: redacted_urls_message(&error.to_string(), &self.credentials),
            redirected_library_id: None,
        })?;
        if !status.is_success() {
            return Err(AttemptFailure {
                kind: error_kind_for_status(status, &body.text),
                status: Some(status.as_u16()),
                message: redacted_urls_message(
                    &failure_message(&body.text, status.as_u16()),
                    &self.credentials,
                ),
                redirected_library_id: None,
            });
        }
        let response: ExaResponse =
            serde_json::from_str(&body.text).map_err(|error| AttemptFailure {
                kind: AttemptErrorKind::Runtime,
                status: Some(status.as_u16()),
                message: redacted_urls_message(
                    &format!("invalid Exa response: {error}"),
                    &self.credentials,
                ),
                redirected_library_id: None,
            })?;
        if response
            .results
            .iter()
            .any(|result| !is_http_url(&result.url))
        {
            return Err(AttemptFailure {
                kind: AttemptErrorKind::Runtime,
                status: Some(status.as_u16()),
                message: "invalid Exa response: result URL must use HTTP(S)".into(),
                redirected_library_id: None,
            });
        }
        Ok(response
            .results
            .into_iter()
            .map(|result| self.normalize_source(operation, result))
            .collect())
    }

    fn normalize_source(&self, operation: &ExaOperation, result: ExaResult) -> Source {
        let (include_text, include_highlights) = match operation {
            ExaOperation::Search(request) => (request.include_text, request.include_highlights),
            ExaOperation::Similar(_) => (true, true),
        };
        Source {
            title: result
                .title
                .map(|value| self.credentials.redact(&value))
                .unwrap_or_default(),
            url: self.credentials.redact(&redact_url(&result.url)),
            published_date: result
                .published_date
                .map(|value| self.credentials.redact(&value)),
            author: result.author.map(|value| self.credentials.redact(&value)),
            text: include_text
                .then_some(result.text)
                .flatten()
                .map(|value| self.credentials.redact(&value)),
            highlights: if include_highlights {
                result
                    .highlights
                    .iter()
                    .map(|value| self.credentials.redact(value))
                    .collect()
            } else {
                Vec::new()
            },
            id: result.id.map(|value| self.credentials.redact(&value)),
            image: result
                .image
                .map(|value| self.credentials.redact(&redact_url(&value))),
            favicon: result
                .favicon
                .map(|value| self.credentials.redact(&redact_url(&value))),
        }
    }
}

enum ExaOperation {
    Search(ExaSearchRequest),
    Similar(ExaSimilarRequest),
}

impl ExaOperation {
    fn target(&self) -> AttemptTarget {
        match self {
            Self::Search(_) => AttemptTarget::seam("docs_search"),
            Self::Similar(_) => AttemptTarget::operation("similar"),
        }
    }

    fn endpoint(&self) -> &'static str {
        match self {
            Self::Search(_) => "search",
            Self::Similar(_) => "findSimilar",
        }
    }

    fn output_input(&self) -> ExaInput {
        match self {
            Self::Search(request) => ExaInput::Search {
                query: request.query.clone(),
            },
            Self::Similar(request) => ExaInput::Similar {
                url: request.url.clone(),
            },
        }
    }

    fn verbose(&self) -> bool {
        match self {
            Self::Search(request) => request.verbose,
            Self::Similar(request) => request.verbose,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExaSearchBody<'a> {
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

impl<'a> From<&'a ExaSearchRequest> for ExaSearchBody<'a> {
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
#[serde(rename_all = "camelCase")]
struct ExaSimilarBody<'a> {
    url: &'a str,
    num_results: u16,
}

impl<'a> From<&'a ExaSimilarRequest> for ExaSimilarBody<'a> {
    fn from(request: &'a ExaSimilarRequest) -> Self {
        Self {
            url: &request.url,
            num_results: request.num_results,
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
    results: Vec<ExaResult>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExaResult {
    title: Option<String>,
    url: String,
    id: Option<String>,
    published_date: Option<String>,
    author: Option<String>,
    text: Option<String>,
    #[serde(default)]
    highlights: Vec<String>,
    image: Option<String>,
    favicon: Option<String>,
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

fn slice_is_empty<T>(values: &[T]) -> bool {
    values.is_empty()
}

fn is_http_url(value: &str) -> bool {
    reqwest::Url::parse(value)
        .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host().is_some())
}
