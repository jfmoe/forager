//! Builds the provider behind a seam from its runtime configuration, and reaches a platform
//! route through its route declaration.
//!
//! The no-wildcard rule covers every match in this module: a match on the provider identity, or
//! on a configuration keyed by it, names each case, so a new provider is a compile error at every
//! dispatch site here.
#![deny(clippy::wildcard_enum_match_arm)]

use reqwest::Client;

use super::constructors::credentials;
use super::route_declaration::{OperationRoute, PlatformRoute, RouteAdapter, RouteContext};
use super::{
    DocsSearch, KeyAccount, MainSearch, PlatformFetch, PlatformSearch, ProviderId,
    SupplementalSearch, WebFetch, WebSearch, arxiv, browser, browser_preflight, gemini_api, hn_api,
    polymarket_api, reddit_browser, serpapi, ssrn_crossref, web_fetch, xai, xiaohongshu_browser,
    youtube_api, youtube_web,
};
use crate::catalog::{
    ProviderTransport, WEB_FETCH, WEB_SEARCH, registration, registrations, route_platform,
};
use crate::config::{
    DocsSearchProviderConfig, MainSearchProviderConfig, PlatformRouteConfig, RuntimeConfig,
    WebFetchProviderConfig, provider_keys_key,
};
use crate::net::RetryPolicy;
use crate::providers::browser::BrowserLogin;
use crate::types::{
    Deadline, Platform, PlatformFetchRequest, PlatformSearchRequest, ProviderError,
};

pub(crate) fn build_main_search(
    config: MainSearchProviderConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Box<dyn MainSearch> {
    match config {
        MainSearchProviderConfig::Xai(config) => Box::new(xai::Xai::build(
            config,
            route_context(client, retry_policy, deadline),
        )),
        MainSearchProviderConfig::Openai(config) => {
            Box::new(super::build_openai(config, client, retry_policy, deadline))
        }
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
    config: DocsSearchProviderConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Box<dyn DocsSearch> {
    match config {
        DocsSearchProviderConfig::Exa(config) => {
            Box::new(super::build_exa(config, client, retry_policy, deadline))
        }
    }
}

pub(crate) fn build_platform_search(
    config: PlatformRouteConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Box<dyn PlatformSearch> {
    declared_route(config, route_context(client, retry_policy, deadline))
        .into_search()
        .expect("planning selects a route for search only when its adapter checks search requests")
}

pub(crate) fn build_platform_fetch(
    config: PlatformRouteConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Box<dyn PlatformFetch> {
    declared_route(config, route_context(client, retry_policy, deadline))
        .into_fetch()
        .expect("planning selects a route for fetch only when its adapter checks fetch requests")
}

/// Runs the checks a platform route needs beyond its platform search probe: one per extra
/// endpoint it combines. Each check is named; most routes have none.
pub(crate) async fn platform_route_extra_checks(
    config: PlatformRouteConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Vec<(&'static str, Result<(), ProviderError>)> {
    let route = declared_route(config, route_context(client, retry_policy, deadline));
    route.extra_checks().await
}

/// Checks the account of every key of a route that registers an account probe.
pub(crate) async fn route_accounts(
    config: PlatformRouteConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Vec<KeyAccount> {
    let id = config.route();
    let route = declared_route(config, route_context(client, retry_policy, deadline));
    let Some(accounts) = route.accounts() else {
        unreachable!("{} registers no account probe", id.name())
    };
    accounts.await
}

fn route_context(client: Client, retry_policy: RetryPolicy, deadline: Deadline) -> RouteContext {
    RouteContext {
        client,
        retry_policy,
        deadline,
    }
}

/// Builds a platform route from its route declaration.
fn declared_route(config: PlatformRouteConfig, context: RouteContext) -> Box<dyn PlatformRoute> {
    match config {
        PlatformRouteConfig::ArxivApi(config) => (arxiv::DECLARATION.build)(config, context),
        PlatformRouteConfig::SsrnCrossref(config) => {
            (ssrn_crossref::DECLARATION.build)(config, context)
        }
        PlatformRouteConfig::Serpapi(config) => (serpapi::DECLARATION.build)(config, context),
        PlatformRouteConfig::XiaohongshuBrowser(config) => {
            (xiaohongshu_browser::DECLARATION.build)(config, context)
        }
        PlatformRouteConfig::Xai(config) => (xai::DECLARATION.build)(config, context),
        PlatformRouteConfig::HnApi(config) => (hn_api::DECLARATION.build)(config, context),
        PlatformRouteConfig::YoutubeApi(config) => {
            (youtube_api::DECLARATION.build)(config, context)
        }
        PlatformRouteConfig::YoutubeWeb(config) => {
            (youtube_web::DECLARATION.build)(config, context)
        }
        PlatformRouteConfig::GeminiApi(config) => (gemini_api::DECLARATION.build)(config, context),
        PlatformRouteConfig::PolymarketApi(config) => {
            (polymarket_api::DECLARATION.build)(config, context)
        }
        PlatformRouteConfig::RedditBrowser(config) => {
            (reddit_browser::DECLARATION.build)(config, context)
        }
    }
}

/// Returns the adapter of a platform route, or `None` for a provider that is no platform route.
fn declared_adapter(id: ProviderId) -> Option<RouteAdapter> {
    match id {
        ProviderId::ArxivApi => Some(arxiv::DECLARATION.adapter),
        ProviderId::SsrnCrossref => Some(ssrn_crossref::DECLARATION.adapter),
        ProviderId::Serpapi => Some(serpapi::DECLARATION.adapter),
        ProviderId::XiaohongshuBrowser => Some(xiaohongshu_browser::DECLARATION.adapter),
        ProviderId::Xai => Some(xai::DECLARATION.adapter),
        ProviderId::HnApi => Some(hn_api::DECLARATION.adapter),
        ProviderId::YoutubeApi => Some(youtube_api::DECLARATION.adapter),
        ProviderId::YoutubeWeb => Some(youtube_web::DECLARATION.adapter),
        ProviderId::GeminiApi => Some(gemini_api::DECLARATION.adapter),
        ProviderId::PolymarketApi => Some(polymarket_api::DECLARATION.adapter),
        ProviderId::RedditBrowser => Some(reddit_browser::DECLARATION.adapter),
        ProviderId::Openai
        | ProviderId::Exa
        | ProviderId::Tavily
        | ProviderId::Firecrawl
        | ProviderId::Jina => None,
    }
}

/// For the route `config` configures, and for each of search and fetch: whether its declaration
/// checks the operation's requests, and whether the route built from the declaration hands the
/// operation out. Building a route sends nothing and launches nothing.
#[cfg(test)]
pub(crate) fn declared_and_built_operations(config: PlatformRouteConfig) -> [(bool, bool); 2] {
    let context = || {
        route_context(
            crate::net::build_client(true).expect("build the HTTP client"),
            RetryPolicy::new(1, 1.0, std::time::Duration::ZERO),
            Deadline::new(std::time::Duration::from_secs(1)),
        )
    };
    let adapter = declared_adapter(config.route())
        .expect("a platform route configuration names a platform route");
    [
        (
            adapter.search_support.is_some(),
            declared_route(config.clone(), context())
                .into_search()
                .is_some(),
        ),
        (
            adapter.fetch_support.is_some(),
            declared_route(config, context()).into_fetch().is_some(),
        ),
    ]
}

/// Whether the route `config` configures, built from its declaration, checks the account of each
/// key. Building a route sends nothing and launches nothing.
#[cfg(test)]
pub(crate) fn built_route_checks_accounts(config: PlatformRouteConfig) -> bool {
    let route = declared_route(
        config,
        route_context(
            crate::net::build_client(true).expect("build the HTTP client"),
            RetryPolicy::new(1, 1.0, std::time::Duration::ZERO),
            Deadline::new(std::time::Duration::from_secs(1)),
        ),
    );
    route.accounts().is_some()
}

/// Returns whether a platform search route can run the request with every explicit option, or
/// `None` for a provider that has no platform search adapter.
pub(crate) fn platform_search_support(
    id: ProviderId,
    request: &PlatformSearchRequest,
) -> Option<Result<(), String>> {
    let route = (declared_adapter(id)?.search_support?)(request);
    Some(transport_support(id).and(route))
}

/// Returns whether a platform fetch route can run the request, or `None` for a provider that has
/// no platform fetch adapter.
pub(crate) fn platform_fetch_support(
    id: ProviderId,
    request: &PlatformFetchRequest,
) -> Option<Result<(), String>> {
    let route = (declared_adapter(id)?.fetch_support?)(request);
    Some(transport_support(id).and(route))
}

/// Returns the login into `platform`'s Browser Profile: the platform's browser route, with that
/// route's login page logic. `None` for a platform without one.
pub(crate) fn browser_login(platform: Platform) -> Option<BrowserLogin> {
    registrations().iter().find_map(|registration| {
        let ProviderTransport::Browser(environment) = registration.transport else {
            return None;
        };
        if route_platform(registration.id) != platform {
            return None;
        }
        Some(BrowserLogin {
            route: registration.id,
            profile: platform.as_str(),
            language: environment.language,
            page: declared_adapter(registration.id)?.login_page?,
        })
    })
}

/// Returns whether the route that alone serves an operation can run the request: this host runs
/// its transport, and the route's request check passes.
pub(crate) fn operation_support<C, Q: ?Sized, I: ?Sized, T>(
    operation: &OperationRoute<C, Q, I, T>,
    request: &Q,
) -> Result<(), String> {
    transport_support(operation.route).and((operation.support)(request))
}

/// Returns whether this host can run the route's transport at all.
fn transport_support(id: ProviderId) -> Result<(), String> {
    match registration(id).transport {
        ProviderTransport::Http => Ok(()),
        ProviderTransport::Browser(_) => browser::host_support()
            .map_err(|reason| format!("{} cannot run here: {reason}", id.name())),
    }
}

/// Checks, without launching a browser or sending a request, that this machine meets what the
/// route needs to run: a browser route passes its preflight check, and an HTTP route that
/// requires keys has them. An anonymous HTTP route always meets them.
///
/// # Errors
///
/// Returns the unmet prerequisite with the fix.
pub(crate) fn route_prerequisites(id: ProviderId, runtime: &RuntimeConfig) -> Result<(), String> {
    match registration(id).transport {
        ProviderTransport::Browser(_) => browser_preflight(id, &runtime.browser),
        ProviderTransport::Http if runtime.provider_configured(id) => Ok(()),
        ProviderTransport::Http => Err(format!(
            "{} has no configured credentials",
            provider_keys_key(id.name())
        )),
    }
}
