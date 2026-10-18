//! The `xai` provider: main search through the xAI Responses API, and the X platform route
//! through its `x_search` tool.

use reqwest::Client;
use serde_json::json;

use crate::config::XaiRuntimeConfig;
use crate::credentials::CredentialPool;
use crate::net::{AttemptFailure, RetryPolicy};
use crate::providers::execution::{ExecutionSettings, execute_v2};
use crate::providers::shared::{normalize_main_search, redact_and_deduplicate_sources};
use crate::providers::{MainSearchRequest, MainSearchRequestKind};
use crate::redact::Secret;
use crate::types::ProviderError;
use crate::types::{AttemptTarget, Deadline, SearchOutcome, Source};

#[path = "xai_responses.rs"]
mod responses;
#[path = "xai_x.rs"]
mod x;
use responses::{ResponsesRequest, output_text};
pub(crate) use x::{fetch_support, search_support};

pub(crate) struct Xai {
    config: XaiRuntimeConfig,
    client: Client,
    credentials: CredentialPool,
    retry_policy: RetryPolicy,
    deadline: Deadline,
}

impl Xai {
    pub(crate) fn new(
        config: XaiRuntimeConfig,
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
        request: MainSearchRequest,
    ) -> Result<SearchOutcome, ProviderError> {
        self.execute(request, MainSearchRequestKind::Search).await
    }

    pub(crate) async fn probe(
        &self,
        request: MainSearchRequest,
    ) -> Result<SearchOutcome, ProviderError> {
        self.execute(request, MainSearchRequestKind::ModelProbe)
            .await
    }

    async fn execute(
        &self,
        request: MainSearchRequest,
        request_kind: MainSearchRequestKind,
    ) -> Result<SearchOutcome, ProviderError> {
        let model = request
            .model
            .clone()
            .unwrap_or_else(|| self.config.model.clone());
        let query = request.query;
        let execution = execute_v2(
            &self.credentials,
            ExecutionSettings {
                verbose: request.verbose,
                ..self.settings(AttemptTarget::seam("main_search"), model.clone())
            },
            |credential, _| {
                let model = model.clone();
                let query = query.clone();
                async move {
                    self.send_once(&query, &model, &credential, request_kind)
                        .await
                }
            },
        )
        .await?;
        let (answer, sources) = execution.value;
        Ok(SearchOutcome {
            provider: "xai",
            query,
            model,
            answer,
            sources,
            extra_sources: Vec::new(),
            capabilities: Vec::new(),
            capability_gaps: Vec::new(),
            attempts: execution.attempts,
            diagnostic: execution.diagnostic,
        })
    }

    /// Returns the settings of one Responses request: streamed, bounded only by the command
    /// deadline.
    fn settings(&self, target: AttemptTarget, model: String) -> ExecutionSettings {
        ExecutionSettings {
            provider: "xai",
            target,
            retry_policy: self.retry_policy,
            deadline: self.deadline,
            attempt_timeout: self.deadline.remaining().unwrap_or_default(),
            verbose: false,
            timeout_message: "xAI Responses request timed out",
            model: Some(model),
            transport: Some("sse"),
            endpoint_host: reqwest::Url::parse(&self.config.url)
                .ok()
                .and_then(|url| url.host_str().map(ToOwned::to_owned)),
        }
    }

    async fn send_once(
        &self,
        query: &str,
        model: &str,
        credential: &Secret,
        request_kind: MainSearchRequestKind,
    ) -> Result<(Option<u16>, (String, Vec<Source>)), AttemptFailure> {
        let input = request_kind.input(query);
        let tools = if request_kind.uses_search_tools() {
            self.config
                .tools
                .iter()
                .map(|tool| json!({ "type": tool }))
                .collect()
        } else {
            Vec::new()
        };
        let body = ResponsesRequest::new(model, request_kind.instruction(), &input, tools);
        let (status, response) = self.complete(&body, credential).await?;
        let (answer, sources) = output_text(&response)?;
        let outcome = if matches!(request_kind, MainSearchRequestKind::Search) {
            normalize_main_search(&answer, sources, &self.credentials)?
        } else {
            (
                answer,
                redact_and_deduplicate_sources(sources, &self.credentials),
            )
        };
        Ok((Some(status), outcome))
    }
}
