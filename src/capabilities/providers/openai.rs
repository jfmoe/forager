//! The `openai` provider: main search through an OpenAI Responses endpoint and its `web_search`
//! tool, the official API or a gateway that speaks the protocol.

use reqwest::Client;
use serde_json::json;

use crate::config::OpenaiRuntimeConfig;
use crate::credentials::CredentialPool;
use crate::net::RetryPolicy;
use crate::providers::main_search_responses::ResponsesMainSearch;
use crate::providers::responses_api::ResponsesEndpoint;
use crate::providers::{MainSearchRequest, MainSearchRequestKind};
use crate::types::{Deadline, ProviderError, SearchOutcome};

// The Responses web_search tool appends it to every URL it cites (2026-10-28).
const TRACKING_PARAMETER: &str = "utm_source=openai";

pub(crate) struct Openai {
    config: OpenaiRuntimeConfig,
    client: Client,
    credentials: CredentialPool,
    retry_policy: RetryPolicy,
    deadline: Deadline,
}

impl Openai {
    pub(crate) fn new(
        config: OpenaiRuntimeConfig,
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
            endpoint: ResponsesEndpoint {
                provider: "openai",
                service: "OpenAI Responses",
                timeout_message: "OpenAI Responses request timed out",
                url: &self.config.url,
                client: &self.client,
                credentials: &self.credentials,
            },
            model: &self.config.model,
            tools: vec![json!({ "type": "web_search" })],
            tracking_parameter: Some(TRACKING_PARAMETER),
            retry_policy: self.retry_policy,
            deadline: self.deadline,
        }
    }
}
