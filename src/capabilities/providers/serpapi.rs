//! The `serpapi` route: SerpApi's search endpoint, keyed by the Provider Credential Pool.
//!
//! This module owns what every SerpApi engine shares: the endpoint, the `api_key` parameter,
//! the HTTP 200 success protocol, and credential redaction. Engine modules own their request
//! parameters, response decoding, and support checks; the account module owns the free
//! per-key quota report.

use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;

use crate::catalog::ProviderId;
use crate::config::KeyedHttpRouteRuntimeConfig;
use crate::credentials::CredentialPool;
use crate::net::{AttemptFailure, ProviderResponseBody, RetryPolicy, read_complete_protocol};
use crate::providers::execution::{ExecutionOutcome, ExecutionSettings, execute_v2};
use crate::providers::shared::redacted_urls_message;
use crate::redact::Secret;
use crate::types::{AttemptErrorKind, AttemptTarget, Deadline, ProviderError};

const ROUTE: ProviderId = ProviderId::Serpapi;
#[path = "serpapi_account.rs"]
mod account;
#[path = "serpapi_scholar.rs"]
mod scholar;
pub(crate) use account::KeyAccount;
pub(crate) use scholar::{cited_by_support, fetch_support, search_support};

pub(crate) struct Serpapi {
    url: String,
    timeout_seconds: u64,
    client: Client,
    credentials: CredentialPool,
    retry_policy: RetryPolicy,
    deadline: Deadline,
}

/// The results of one SerpApi search that passed the HTTP 200 success protocol.
#[derive(Debug, PartialEq)]
enum SearchResults {
    /// A non-empty `organic_results` array, and whether SerpApi offers a next page.
    Found {
        results: Vec<Value>,
        has_next_page: bool,
    },
    /// SerpApi reported the search as `Fully empty`.
    Empty,
}

/// Returns the failure of an HTTP 200 response that the success protocol or an engine refuses.
fn refusal(kind: AttemptErrorKind, message: String) -> AttemptFailure {
    AttemptFailure {
        kind,
        status: Some(200),
        message,
    }
}

impl Serpapi {
    pub(crate) fn new(
        config: KeyedHttpRouteRuntimeConfig,
        client: Client,
        credentials: CredentialPool,
        retry_policy: RetryPolicy,
        deadline: Deadline,
    ) -> Self {
        Self {
            url: config.url,
            timeout_seconds: config.timeout_seconds,
            client,
            credentials,
            retry_policy,
            deadline,
        }
    }

    /// Runs one logical search of the named platform operation with credential claim, rotation,
    /// and retry. `decode` turns the results into the operation's value inside the attempt, so a
    /// refusal is that attempt's failure.
    async fn run<T>(
        &self,
        operation: &'static str,
        parameters: &[(&'static str, String)],
        decode: impl Fn(SearchResults) -> Result<T, AttemptFailure>,
    ) -> Result<ExecutionOutcome<T>, ProviderError> {
        let decode = &decode;
        execute_v2(
            &self.credentials,
            self.settings(AttemptTarget::platform(
                scholar::PLATFORM.as_str(),
                operation,
            )),
            |credential, _| async move {
                let results = self.send_once(parameters, &credential).await?;
                decode(results)
                    .map(|value| (Some(200), value))
                    .map_err(|failure| self.redacted(failure))
            },
        )
        .await
    }

    async fn send_once(
        &self,
        parameters: &[(&'static str, String)],
        credential: &Secret,
    ) -> Result<SearchResults, AttemptFailure> {
        let body = self.get(self.url.as_str(), parameters, credential).await?;
        success_protocol(&body.text).map_err(|failure| self.redacted(failure))
    }

    /// Sends one keyed GET and reads the complete body; any HTTP error status is a failure.
    async fn get(
        &self,
        url: &str,
        parameters: &[(&'static str, String)],
        credential: &Secret,
    ) -> Result<ProviderResponseBody, AttemptFailure> {
        let request = self
            .client
            .get(url)
            .query(parameters)
            .query(&[("api_key", credential.expose())]);
        // The key travels in the URL, so the URL never reaches the message.
        let response = request.send().await.map_err(|error| AttemptFailure {
            kind: AttemptErrorKind::Network,
            status: error.status().map(|status| status.as_u16()),
            message: redacted_urls_message(&error.without_url().to_string(), &self.credentials),
        })?;
        read_complete_protocol(response, &self.credentials, failure_message).await
    }

    fn redacted(&self, mut failure: AttemptFailure) -> AttemptFailure {
        failure.message = redacted_urls_message(&failure.message, &self.credentials);
        failure
    }

    fn settings(&self, target: AttemptTarget) -> ExecutionSettings {
        ExecutionSettings {
            provider: ROUTE.name(),
            target,
            retry_policy: self.retry_policy,
            deadline: self.deadline,
            attempt_timeout: Duration::from_secs(self.timeout_seconds),
            verbose: false,
            timeout_message: "SerpApi request timed out",
            model: None,
            transport: Some("http"),
            endpoint_host: None,
            breaker_event: None,
        }
    }
}

#[derive(Deserialize)]
struct Envelope {
    search_metadata: Option<Metadata>,
    error: Option<String>,
    search_information: Option<Information>,
    organic_results: Option<Vec<Value>>,
    serpapi_pagination: Option<Pagination>,
}

#[derive(Deserialize)]
struct Metadata {
    status: Option<String>,
}

#[derive(Deserialize)]
struct Information {
    organic_results_state: Option<String>,
}

#[derive(Deserialize)]
struct Pagination {
    next: Option<String>,
}

#[derive(Deserialize)]
struct ErrorBody {
    error: String,
}

/// Decides an HTTP 200 response: `Success` with results or exactly `Fully empty` succeeds,
/// `Error` is a Network failure the shared policy retries, and anything else is Runtime.
fn success_protocol(body: &str) -> Result<SearchResults, AttemptFailure> {
    let runtime = |message| refusal(AttemptErrorKind::Runtime, message);
    let envelope = serde_json::from_str::<Envelope>(body)
        .map_err(|error| runtime(format!("invalid SerpApi response: {error}")))?;
    let status = envelope
        .search_metadata
        .and_then(|metadata| metadata.status);
    match status.as_deref() {
        Some("Success") => {}
        Some("Error") => {
            return Err(refusal(
                AttemptErrorKind::Network,
                format!(
                    "SerpApi reported an error: {}",
                    envelope.error.as_deref().unwrap_or("no message")
                ),
            ));
        }
        Some(status) => {
            return Err(runtime(format!(
                "SerpApi returned search status `{status}`"
            )));
        }
        None => {
            return Err(runtime("SerpApi response has no search status".into()));
        }
    }
    let state = envelope
        .search_information
        .and_then(|information| information.organic_results_state);
    match (envelope.organic_results, state.as_deref()) {
        (Some(results), _) if !results.is_empty() => Ok(SearchResults::Found {
            results,
            has_next_page: envelope
                .serpapi_pagination
                .is_some_and(|pagination| pagination.next.is_some()),
        }),
        (None, Some("Fully empty")) => Ok(SearchResults::Empty),
        _ => Err(runtime(format!(
            "SerpApi reported success without results (results state `{}`)",
            state.as_deref().unwrap_or("none")
        ))),
    }
}

fn failure_message(body: &str, status: u16) -> String {
    serde_json::from_str::<ErrorBody>(body).map_or_else(
        |_| format!("SerpApi returned HTTP {status}"),
        |body| format!("SerpApi returned HTTP {status}: {}", body.error),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{SearchResults, success_protocol};
    use crate::types::AttemptErrorKind;

    fn decided(body: &Value) -> Result<SearchResults, (AttemptErrorKind, String)> {
        success_protocol(&body.to_string()).map_err(|failure| (failure.kind, failure.message))
    }

    #[test]
    fn results_with_success_are_found_with_the_next_page_signal() {
        let page = |pagination: Value| {
            json!({
                "search_metadata": {"status": "Success"},
                "organic_results": [{"title": "Time series momentum"}],
                "serpapi_pagination": pagination
            })
        };
        let results = [
            json!({"next": "https://serpapi.com/search.json?start=20"}),
            json!({"current": 1}),
            Value::Null,
        ]
        .map(|pagination| decided(&page(pagination)));

        let found = |has_next_page| {
            Ok(SearchResults::Found {
                results: vec![json!({"title": "Time series momentum"})],
                has_next_page,
            })
        };
        assert_eq!(results, [found(true), found(false), found(false)]);
    }

    #[test]
    fn a_fully_empty_success_without_results_is_empty() {
        let body = json!({
            "search_metadata": {"status": "Success"},
            "search_information": {"organic_results_state": "Fully empty"}
        });

        assert_eq!(decided(&body), Ok(SearchResults::Empty));
    }

    #[test]
    fn an_error_status_is_a_network_failure_with_serpapi_message() {
        let body = json!({
            "search_metadata": {"id": "6ac6613da20802ffe5e7c8be", "status": "Error"},
            "error": "Google hasn't returned any results for this query."
        });

        assert_eq!(
            decided(&body),
            Err((
                AttemptErrorKind::Network,
                "SerpApi reported an error: Google hasn't returned any results for this query."
                    .to_owned()
            ))
        );
    }

    #[test]
    fn any_other_status_or_results_shape_is_a_runtime_failure() {
        let results = [
            json!({
                "search_metadata": {"status": "Success"},
                "search_information": {"organic_results_state": "Results for exact spelling"}
            }),
            json!({"search_metadata": {"status": "Success"}, "organic_results": []}),
            json!({
                "search_metadata": {"status": "Success"},
                "search_information": {"organic_results_state": "Fully empty"},
                "organic_results": []
            }),
            json!({"search_metadata": {}, "organic_results": [{"title": "x"}]}),
            json!({"search_metadata": {"status": "Processing"}, "organic_results": [{"title": "x"}]}),
        ]
        .map(|body| decided(&body));

        let runtime = |message: &str| Err((AttemptErrorKind::Runtime, message.to_owned()));
        assert_eq!(
            results,
            [
                runtime(
                    "SerpApi reported success without results (results state `Results for exact spelling`)"
                ),
                runtime("SerpApi reported success without results (results state `none`)"),
                runtime("SerpApi reported success without results (results state `Fully empty`)"),
                runtime("SerpApi response has no search status"),
                runtime("SerpApi returned search status `Processing`"),
            ]
        );
    }

    #[test]
    fn a_body_that_is_no_serpapi_envelope_is_a_runtime_failure() {
        let failure = decided(&json!({
            "search_metadata": {"status": "Success"},
            "organic_results": {"position": 1}
        }))
        .unwrap_err();

        assert_eq!(
            (
                failure.0,
                failure.1.starts_with("invalid SerpApi response: ")
            ),
            (AttemptErrorKind::Runtime, true),
            "message: {}",
            failure.1
        );
    }
}
