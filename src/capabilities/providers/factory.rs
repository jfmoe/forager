use std::sync::Arc;

use reqwest::Client;

use super::constructors::{credentials, route_limiter};
use super::{
    ArxivApi, DocsSearch, KeyAccount, MainSearch, ModelBreakers, PlatformFetch, PlatformSearch,
    ProviderId, Serpapi, SsrnBrowser, SsrnCrossref, SupplementalSearch, VerticalSearch, WebFetch,
    WebSearch, arxiv, opencli, serpapi, ssrn_browser, ssrn_crossref, web_fetch,
};
use crate::catalog::{ProviderTransport, VERTICAL_SEARCH, WEB_FETCH, WEB_SEARCH, registration};
use crate::config::{
    AnysearchRuntimeConfig, DocsSearchProviderConfig, HttpRouteRuntimeConfig,
    KeyedHttpRouteRuntimeConfig, MainSearchProviderConfig, PlatformRouteConfig,
    ProcessRouteRuntimeConfig, WebFetchProviderConfig,
};
use crate::net::RetryPolicy;
use crate::types::{
    Deadline, PlatformFetchRequest, PlatformSearchOutcome, PlatformSearchRequest, ProviderError,
    ScholarCitedByRequest,
};

/// The routes that list the works citing a Google Scholar paper. Cited-by has one route, so it
/// is an inherent method of that route and its route set lives here, not in the platform
/// catalog.
pub(crate) const SCHOLAR_CITED_BY_ROUTES: &[ProviderId] = &[ProviderId::Serpapi];

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
        PlatformRouteConfig::SsrnBrowser(config) => Box::new(build_ssrn_browser(config, deadline)),
        PlatformRouteConfig::Serpapi(config) => {
            Box::new(build_serpapi(config, client, retry_policy, deadline))
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
        PlatformRouteConfig::SsrnBrowser(config) => Box::new(build_ssrn_browser(config, deadline)),
        PlatformRouteConfig::Serpapi(config) => {
            Box::new(build_serpapi(config, client, retry_policy, deadline))
        }
    }
}

/// Lists one page of the works citing a Google Scholar paper through a cited-by route.
pub(crate) async fn scholar_cited_by(
    config: PlatformRouteConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    request: &ScholarCitedByRequest,
) -> Result<PlatformSearchOutcome, ProviderError> {
    let PlatformRouteConfig::Serpapi(config) = config else {
        unreachable!("planning selects only cited-by routes")
    };
    build_serpapi(config, client, retry_policy, deadline)
        .cited_by(request)
        .await
}

/// Checks the account of every key of a route that registers an account probe.
pub(crate) async fn route_accounts(
    config: PlatformRouteConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Vec<KeyAccount> {
    let PlatformRouteConfig::Serpapi(config) = config else {
        unreachable!("only serpapi registers an account probe")
    };
    build_serpapi(config, client, retry_policy, deadline)
        .accounts()
        .await
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

fn build_ssrn_browser(config: ProcessRouteRuntimeConfig, deadline: Deadline) -> SsrnBrowser {
    SsrnBrowser::new(
        config,
        route_limiter(ProviderId::SsrnBrowser)
            .expect("ssrn_browser registration declares an access policy"),
        deadline,
    )
}

fn build_serpapi(
    mut config: KeyedHttpRouteRuntimeConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Serpapi {
    let credentials = credentials(ProviderId::Serpapi, &mut config.keys);
    Serpapi::new(config, client, credentials, retry_policy, deadline)
}

/// Returns whether a platform search route can run the request with every explicit option, or
/// `None` for a provider that has no platform search adapter.
pub(crate) fn platform_search_support(
    id: ProviderId,
    request: &PlatformSearchRequest,
) -> Option<Result<(), String>> {
    let route = match id {
        ProviderId::ArxivApi => arxiv::search_support(request),
        ProviderId::SsrnCrossref => ssrn_crossref::search_support(request),
        ProviderId::SsrnBrowser => ssrn_browser::search_support(request),
        ProviderId::Serpapi => serpapi::search_support(request),
        _ => return None,
    };
    Some(transport_support(id).and(route))
}

/// Returns whether a platform fetch route can run the request, or `None` for a provider that has
/// no platform fetch adapter.
pub(crate) fn platform_fetch_support(
    id: ProviderId,
    request: &PlatformFetchRequest,
) -> Option<Result<(), String>> {
    let route = match id {
        ProviderId::ArxivApi => arxiv::fetch_support(request),
        ProviderId::SsrnCrossref => ssrn_crossref::fetch_support(request),
        ProviderId::SsrnBrowser => ssrn_browser::fetch_support(request),
        ProviderId::Serpapi => serpapi::fetch_support(request),
        _ => return None,
    };
    Some(transport_support(id).and(route))
}

/// Returns whether a cited-by route can run the request, or `None` for a provider that does not
/// implement cited-by.
pub(crate) fn scholar_cited_by_support(
    id: ProviderId,
    request: &ScholarCitedByRequest,
) -> Option<Result<(), String>> {
    let route = match id {
        ProviderId::Serpapi => serpapi::cited_by_support(request),
        _ => return None,
    };
    Some(transport_support(id).and(route))
}

/// Returns whether this host can run the route's transport at all.
fn transport_support(id: ProviderId) -> Result<(), String> {
    match registration(id).transport {
        ProviderTransport::Http => Ok(()),
        ProviderTransport::OpenCli(_) => opencli::host_support()
            .map_err(|reason| format!("{} cannot run here: {reason}", id.name())),
    }
}
