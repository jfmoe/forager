//! The `xai` provider: main search through the xAI Responses API, and the X platform route
//! through its `x_search` tool.

use std::future::Future;
use std::pin::Pin;

use reqwest::Client;
use serde_json::json;

use crate::catalog::ProviderId;
use crate::config::XaiRuntimeConfig;
use crate::credentials::CredentialPool;
use crate::net::RetryPolicy;
use crate::providers::constructors::credentials;
use crate::providers::execution::ExecutionSettings;
use crate::providers::main_search_responses::ResponsesMainSearch;
use crate::providers::responses_api::ResponsesEndpoint;
use crate::providers::route_declaration::{
    OperationRoute, PlatformRoute, RouteAdapter, RouteContext, RouteDeclaration, any_request,
};
use crate::providers::{
    MainSearch, MainSearchRequest, MainSearchRequestKind, PlatformFetch, PlatformSearch,
};
use crate::types::{
    AttemptTarget, Deadline, PlatformFetchOutcome, PlatformFetchRequest, PlatformSearchOutcome,
    PlatformSearchRequest, ProviderError, SearchOutcome, XAskOutcome, XAskRequest,
};

#[path = "xai_x.rs"]
mod x;
pub(crate) use x::{fetch_support, search_support};

const SERVICE: &str = "xAI Responses";

/// The declaration of the X platform route: Search and Fetch of the catalog, read through the
/// `x_search` tool.
pub(crate) const DECLARATION: RouteDeclaration<XaiRuntimeConfig> = RouteDeclaration {
    build: |config, context| Box::new(Xai::build(config, context)),
    adapter: RouteAdapter {
        search_support: Some(search_support),
        fetch_support: Some(fetch_support),
        login_page: None,
    },
};

/// Ask on X, the X operation that only this route serves.
pub(crate) const ASK_OPERATION: OperationRoute<
    XaiRuntimeConfig,
    XAskRequest,
    XAskRequest,
    XAskOutcome,
> = OperationRoute {
    route: ProviderId::Xai,
    config: |routes| &routes.xai,
    support: any_request,
    run: |config, context, request| {
        Box::pin(async move { Xai::build(config, context).ask(request).await })
    },
};

impl PlatformRoute for Xai {
    fn into_search(self: Box<Self>) -> Option<Box<dyn PlatformSearch>> {
        Some(self)
    }

    fn into_fetch(self: Box<Self>) -> Option<Box<dyn PlatformFetch>> {
        Some(self)
    }
}

impl PlatformSearch for Xai {
    fn search<'a>(
        &'a self,
        request: &'a PlatformSearchRequest,
    ) -> Pin<Box<dyn Future<Output = Result<PlatformSearchOutcome, ProviderError>> + Send + 'a>>
    {
        Box::pin(Xai::search_posts(self, request))
    }
}

impl MainSearch for Xai {
    fn search(
        &self,
        request: MainSearchRequest,
    ) -> Pin<Box<dyn Future<Output = Result<SearchOutcome, ProviderError>> + Send + '_>> {
        Box::pin(self.search(request))
    }

    fn probe(
        &self,
        request: MainSearchRequest,
    ) -> Pin<Box<dyn Future<Output = Result<SearchOutcome, ProviderError>> + Send + '_>> {
        Box::pin(self.probe(request))
    }
}

impl PlatformFetch for Xai {
    fn fetch<'a>(
        &'a self,
        request: &'a PlatformFetchRequest,
    ) -> Pin<Box<dyn Future<Output = Result<PlatformFetchOutcome, ProviderError>> + Send + 'a>>
    {
        Box::pin(Xai::fetch_post(self, request))
    }
}

pub(crate) struct Xai {
    config: XaiRuntimeConfig,
    client: Client,
    credentials: CredentialPool,
    retry_policy: RetryPolicy,
    deadline: Deadline,
}

impl Xai {
    /// Builds the provider for main search and for the X platform route alike.
    pub(crate) fn build(mut config: XaiRuntimeConfig, context: RouteContext) -> Self {
        Self {
            credentials: credentials(ProviderId::Xai, &mut config.keys),
            config,
            client: context.client,
            retry_policy: context.retry_policy,
            deadline: context.deadline,
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
