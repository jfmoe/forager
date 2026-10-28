//! Main search through a Responses endpoint, the path `xai` and `openai` share: the request of
//! each kind, normalization, and the outcome. Each provider supplies only its endpoint, its
//! model, the tools a search sends, and the tracking parameter it appends to cited URLs.

use serde_json::Value;

use crate::net::{AttemptFailure, RetryPolicy};
use crate::providers::execution::{ExecutionSettings, execute_v2};
use crate::providers::responses_api::{ResponsesEndpoint, ResponsesRequest, output_text};
use crate::providers::shared::{
    normalize_main_search, redact_and_deduplicate_sources, without_query_parameter,
};
use crate::providers::{MainSearchRequest, MainSearchRequestKind};
use crate::redact::Secret;
use crate::types::{AttemptTarget, Deadline, ProviderError, SearchOutcome, Source};

/// One provider's main search through its Responses endpoint.
pub(super) struct ResponsesMainSearch<'a> {
    pub(super) endpoint: ResponsesEndpoint<'a>,
    /// The model of a request that names none.
    pub(super) model: &'a str,
    /// The tools a search sends; a model probe sends none.
    pub(super) tools: Vec<Value>,
    /// The exact `name=value` query parameter the provider appends to the URLs it cites.
    pub(super) tracking_parameter: Option<&'static str>,
    pub(super) retry_policy: RetryPolicy,
    pub(super) deadline: Deadline,
}

impl ResponsesMainSearch<'_> {
    pub(super) async fn run(
        &self,
        request: MainSearchRequest,
        request_kind: MainSearchRequestKind,
    ) -> Result<SearchOutcome, ProviderError> {
        let model = request.model.unwrap_or_else(|| self.model.to_owned());
        let query = request.query;
        let settings = ExecutionSettings {
            verbose: request.verbose,
            ..self.endpoint.settings(
                AttemptTarget::seam("main_search"),
                model.clone(),
                self.retry_policy,
                self.deadline,
            )
        };
        let execution = execute_v2(self.endpoint.credentials, settings, |credential, _| {
            let model = model.clone();
            let query = query.clone();
            async move {
                self.send_once(&query, &model, &credential, request_kind)
                    .await
            }
        })
        .await?;
        let (answer, sources) = execution.value;
        Ok(SearchOutcome {
            provider: self.endpoint.provider,
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

    async fn send_once(
        &self,
        query: &str,
        model: &str,
        credential: &Secret,
        request_kind: MainSearchRequestKind,
    ) -> Result<(Option<u16>, (String, Vec<Source>)), AttemptFailure> {
        let input = request_kind.input(query);
        let tools = if request_kind.uses_search_tools() {
            self.tools.clone()
        } else {
            Vec::new()
        };
        let body = ResponsesRequest::new(model, request_kind.instruction(), &input, tools);
        let (status, response) = self.endpoint.complete(&body, credential).await?;
        let (answer, sources) = output_text(self.endpoint.service, &response)?;
        let (answer, sources) = if matches!(request_kind, MainSearchRequestKind::Search) {
            normalize_main_search(&answer, sources, self.endpoint.credentials)?
        } else {
            (
                answer,
                redact_and_deduplicate_sources(sources, self.endpoint.credentials),
            )
        };
        Ok((Some(status), (answer, self.untracked(sources))))
    }

    /// Drops the tracking parameter from every source, then deduplicates the sources again,
    /// since two URLs that differ only in it are one page.
    fn untracked(&self, sources: Vec<Source>) -> Vec<Source> {
        let Some(parameter) = self.tracking_parameter else {
            return sources;
        };
        let sources = sources
            .into_iter()
            .map(|mut source| {
                source.url = without_query_parameter(&source.url, parameter);
                source
            })
            .collect();
        redact_and_deduplicate_sources(sources, self.endpoint.credentials)
    }
}
