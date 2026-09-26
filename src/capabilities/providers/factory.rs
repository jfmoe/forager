use std::sync::Arc;

use reqwest::Client;

use super::constructors::{credentials, route_limiter};
use super::{
    ArxivApi, DocsSearch, MainSearch, ModelBreakers, PlatformFetch, PlatformSearch, ProviderId,
    SsrnCrossref, SupplementalSearch, VerticalSearch, WebFetch, WebSearch, arxiv, ssrn_crossref,
    web_fetch,
};
use crate::catalog::{VERTICAL_SEARCH, WEB_FETCH, WEB_SEARCH};
use crate::config::{
    AnysearchRuntimeConfig, DocsSearchProviderConfig, HttpRouteRuntimeConfig,
    MainSearchProviderConfig, PlatformRouteConfig, WebFetchProviderConfig,
};
use crate::net::RetryPolicy;
use crate::types::{Deadline, PlatformFetchRequest, PlatformSearchRequest};

pub(crate) fn build_main_search(
    id: ProviderId,
    config: MainSearchProviderConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    breakers: Arc<ModelBreakers>,
) -> Box<dyn MainSearch> {
    match (id, config) {
        (ProviderId::Xai, MainSearchProviderConfig::Xai(config)) => {
            Box::new(super::build_xai(config, client, retry_policy, deadline))
        }
        (ProviderId::OpenAiCompatible, MainSearchProviderConfig::OpenAiCompatible(config)) => {
            Box::new(super::build_openai_compatible(
                config,
                client,
                retry_policy,
                deadline,
                breakers,
            ))
        }
        _ => unreachable!("validated main-search catalog entry has matching config"),
    }
}

pub(crate) fn build_web_fetch(
    id: ProviderId,
    mut config: WebFetchProviderConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Box<dyn WebFetch> {
    assert!(WEB_FETCH.contains(id), "validated web-fetch provider");
    let credentials = credentials(id, &mut config.keys);
    web_fetch::new(id, config, client, credentials, retry_policy, deadline)
}

pub(crate) fn build_web_search(
    id: ProviderId,
    mut config: WebFetchProviderConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Box<dyn WebSearch> {
    assert!(WEB_SEARCH.contains(id), "validated web-search provider");
    let credentials = credentials(id, &mut config.keys);
    Box::new(SupplementalSearch::new(
        id,
        config,
        client,
        credentials,
        retry_policy,
        deadline,
    ))
}

pub(crate) fn build_docs_search(
    id: ProviderId,
    config: DocsSearchProviderConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Box<dyn DocsSearch> {
    match (id, config) {
        (ProviderId::Exa, DocsSearchProviderConfig::Exa(config)) => {
            Box::new(super::build_exa(config, client, retry_policy, deadline))
        }
        (ProviderId::Context7, DocsSearchProviderConfig::Context7(config)) => Box::new(
            super::build_context7(config, client, retry_policy, deadline),
        ),
        _ => unreachable!("validated docs-search catalog entry has matching config"),
    }
}

pub(crate) fn build_vertical_search(
    id: ProviderId,
    config: AnysearchRuntimeConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Box<dyn VerticalSearch> {
    assert!(
        VERTICAL_SEARCH.contains(id),
        "validated vertical-search provider"
    );
    Box::new(super::build_anysearch(
        config,
        client,
        retry_policy,
        deadline,
    ))
}

pub(crate) fn build_platform_search(
    config: PlatformRouteConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Box<dyn PlatformSearch> {
    match config {
        PlatformRouteConfig::ArxivApi(config) => {
            Box::new(build_arxiv_api(config, client, retry_policy, deadline))
        }
        PlatformRouteConfig::SsrnCrossref(config) => {
            Box::new(build_ssrn_crossref(config, client, retry_policy, deadline))
        }
    }
}

pub(crate) fn build_platform_fetch(
    config: PlatformRouteConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Box<dyn PlatformFetch> {
    match config {
        PlatformRouteConfig::ArxivApi(config) => {
            Box::new(build_arxiv_api(config, client, retry_policy, deadline))
        }
        PlatformRouteConfig::SsrnCrossref(config) => {
            Box::new(build_ssrn_crossref(config, client, retry_policy, deadline))
        }
    }
}

fn build_arxiv_api(
    config: HttpRouteRuntimeConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> ArxivApi {
    ArxivApi::new(
        config,
        client,
        route_limiter(ProviderId::ArxivApi)
            .expect("arxiv_api registration declares an access policy"),
        retry_policy,
        deadline,
    )
}

fn build_ssrn_crossref(
    config: HttpRouteRuntimeConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> SsrnCrossref {
    SsrnCrossref::new(
        config,
        client,
        route_limiter(ProviderId::SsrnCrossref)
            .expect("ssrn_crossref registration declares an access policy"),
        retry_policy,
        deadline,
    )
}

/// Returns whether a platform search route can run the request with every explicit option, or
/// `None` for a provider that has no platform search adapter.
pub(crate) fn platform_search_support(
    id: ProviderId,
    request: &PlatformSearchRequest,
) -> Option<Result<(), String>> {
    match id {
        ProviderId::ArxivApi => Some(arxiv::search_support(request)),
        ProviderId::SsrnCrossref => Some(ssrn_crossref::search_support(request)),
        _ => None,
    }
}

/// Returns whether a platform fetch route can run the request, or `None` for a provider that has
/// no platform fetch adapter.
pub(crate) fn platform_fetch_support(
    id: ProviderId,
    request: &PlatformFetchRequest,
) -> Option<Result<(), String>> {
    match id {
        ProviderId::ArxivApi => Some(arxiv::fetch_support(request)),
        ProviderId::SsrnCrossref => Some(ssrn_crossref::fetch_support(request)),
        _ => None,
    }
}
