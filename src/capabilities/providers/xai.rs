//! The `xai` provider: main search through the xAI Responses API, and the X platform route
//! through its `x_search` tool.

use reqwest::Client;
use serde_json::json;

use crate::config::XaiRuntimeConfig;
use crate::credentials::CredentialPool;
use crate::net::RetryPolicy;
use crate::providers::execution::ExecutionSettings;
use crate::providers::main_search_responses::ResponsesMainSearch;
use crate::providers::responses_api::ResponsesEndpoint;
use crate::providers::{MainSearchRequest, MainSearchRequestKind};
use crate::types::{AttemptTarget, Deadline, ProviderError, SearchOutcome};

#[path = "xai_x.rs"]
mod x;
pub(crate) use x::{fetch_support, search_support};

const SERVICE: &str = "xAI Responses";

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
        self.main_search()
            .run(request, MainSearchRequestKind::Search)
            .await
    }

    pub(crate) async fn probe(
        &self,
        request: MainSearchRequest,
    ) -> Result<SearchOutcome, ProviderError> {
        self.main_search()
            .run(request, MainSearchRequestKind::ModelProbe)
            .await
    }

    fn main_search(&self) -> ResponsesMainSearch<'_> {
        ResponsesMainSearch {
            endpoint: self.responses(),
            model: &self.config.model,
            tools: self
                .config
                .tools
                .iter()
                .map(|tool| json!({ "type": tool }))
                .collect(),
            tracking_parameter: None,
            retry_policy: self.retry_policy,
            deadline: self.deadline,
        }
    }

    fn responses(&self) -> ResponsesEndpoint<'_> {
        ResponsesEndpoint {
            provider: "xai",
            service: SERVICE,
            timeout_message: "xAI Responses request timed out",
            url: &self.config.url,
            client: &self.client,
            credentials: &self.credentials,
        }
    }

    fn settings(&self, target: AttemptTarget, model: String) -> ExecutionSettings {
        self.responses()
            .settings(target, model, self.retry_policy, self.deadline)
    }
}
