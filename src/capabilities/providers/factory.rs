//! Builds the provider behind a seam from its runtime configuration, and reaches a platform
//! route through its route declaration.
//!
//! The no-wildcard rule covers every match in this module: a match on the provider identity, or
//! on a configuration keyed by it, names each case, so a new provider is a compile error at every
//! dispatch site here. Only the single-route support checks opt out.
#![deny(clippy::wildcard_enum_match_arm)]

use reqwest::Client;

use super::constructors::credentials;
use super::route_declaration::{PlatformRoute, RouteAdapter, RouteContext};
use super::{
    DocsSearch, KeyAccount, MainSearch, PlatformFetch, PlatformSearch, ProviderId,
    SupplementalSearch, WebFetch, WebSearch, arxiv, browser, browser_preflight, gemini_api, hn_api,
    polymarket_api, reddit_browser, serpapi, ssrn_crossref, web_fetch, xai, xiaohongshu_browser,
    youtube_api, youtube_web,
};
use crate::catalog::{ProviderTransport, WEB_FETCH, WEB_SEARCH, registration, registrations};
use crate::config::{
    DocsSearchProviderConfig, MainSearchProviderConfig, PlatformRouteConfig, RuntimeConfig,
    WebFetchProviderConfig, provider_keys_key,
};
use crate::net::RetryPolicy;
use crate::providers::browser::BrowserLogin;
use crate::types::{
    Deadline, HnStoriesOutcome, HnStoriesRequest, ModelAnswerOutcome, ModelTranscriptOutcome,
    Platform, PlatformFetchRequest, PlatformItem, PlatformSearchOutcome, PlatformSearchRequest,
    PolymarketHistoryOutcome, PolymarketHistoryRequest, ProviderError, ScholarCitedByRequest,
    XAskOutcome, XAskRequest, XiaohongshuCommentsOutcome, XiaohongshuCommentsRequest,
    YoutubeCommentsOutcome, YoutubeCommentsRequest, YoutubeVideosOutcome, YoutubeVideosRequest,
};

/// The routes that list the works citing a Google Scholar paper. Cited-by has one route, so it
/// is an inherent method of that route and its route set lives here, not in the platform
/// catalog.
pub(crate) const SCHOLAR_CITED_BY_ROUTES: &[ProviderId] = &[ProviderId::Serpapi];

/// The routes that list the comments of a Xiaohongshu note; like cited-by, a single-route
/// operation whose route set lives here.
pub(crate) const XIAOHONGSHU_COMMENTS_ROUTES: &[ProviderId] = &[ProviderId::XiaohongshuBrowser];

/// The routes that list the comments of a YouTube video; like cited-by, a single-route
/// operation whose route set lives here.
pub(crate) const YOUTUBE_COMMENTS_ROUTES: &[ProviderId] = &[ProviderId::YoutubeApi];

/// The routes that list the videos of a YouTube playlist or channel; like cited-by, a
/// single-route operation whose route set lives here.
pub(crate) const YOUTUBE_VIDEOS_ROUTES: &[ProviderId] = &[ProviderId::YoutubeApi];

/// The routes that answer a question from X posts; like cited-by, a single-route operation
/// whose route set lives here.
pub(crate) const X_ASK_ROUTES: &[ProviderId] = &[ProviderId::Xai];

/// The routes that answer a question about a YouTube video; like cited-by, a single-route
/// operation whose route set lives here.
pub(crate) const YOUTUBE_ASK_ROUTES: &[ProviderId] = &[ProviderId::GeminiApi];

/// The routes that transcribe a YouTube video with a model; like cited-by, a single-route
/// operation whose route set lives here.
pub(crate) const YOUTUBE_TRANSCRIBE_ROUTES: &[ProviderId] = &[ProviderId::GeminiApi];

/// The routes that list a Hacker News story list; like cited-by, a single-route operation whose
/// route set lives here.
pub(crate) const HN_STORIES_ROUTES: &[ProviderId] = &[ProviderId::HnApi];

/// The routes that read the Price History of a Polymarket market; like cited-by, a single-route
/// operation whose route set lives here.
pub(crate) const POLYMARKET_HISTORY_ROUTES: &[ProviderId] = &[ProviderId::PolymarketApi];

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
    serpapi::Serpapi::build(config, route_context(client, retry_policy, deadline))
        .cited_by(request)
        .await
}

/// Answers a question from X posts through an ask route.
pub(crate) async fn x_ask(
    config: PlatformRouteConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    request: &XAskRequest,
) -> Result<XAskOutcome, ProviderError> {
    let PlatformRouteConfig::Xai(config) = config else {
        unreachable!("planning selects only ask routes")
    };
    xai::Xai::build(config, route_context(client, retry_policy, deadline))
        .ask(request)
        .await
}

/// Lists the comments of a Xiaohongshu note through a comments route.
pub(crate) async fn xiaohongshu_comments(
    config: PlatformRouteConfig,
    deadline: Deadline,
    request: &XiaohongshuCommentsRequest,
) -> Result<XiaohongshuCommentsOutcome, ProviderError> {
    let PlatformRouteConfig::XiaohongshuBrowser(config) = config else {
        unreachable!("planning selects only comments routes")
    };
    xiaohongshu_browser::XiaohongshuBrowser::build(config, deadline)
        .comments(request)
        .await
}

/// Lists the top-level comments of a YouTube video through a comments route.
pub(crate) async fn youtube_comments(
    config: PlatformRouteConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    request: &YoutubeCommentsRequest,
) -> Result<YoutubeCommentsOutcome, ProviderError> {
    let PlatformRouteConfig::YoutubeApi(config) = config else {
        unreachable!("planning selects only comments routes")
    };
    youtube_api::YoutubeApi::build(config, route_context(client, retry_policy, deadline))
        .comments(request)
        .await
}

/// Lists one page of the videos of a YouTube playlist or channel through a videos route.
pub(crate) async fn youtube_videos(
    config: PlatformRouteConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    request: &YoutubeVideosRequest,
) -> Result<YoutubeVideosOutcome, ProviderError> {
    let PlatformRouteConfig::YoutubeApi(config) = config else {
        unreachable!("planning selects only videos routes")
    };
    youtube_api::YoutubeApi::build(config, route_context(client, retry_policy, deadline))
        .videos(request)
        .await
}

/// Answers a question about a YouTube video, which the precheck read, through an ask
/// route.
pub(crate) async fn youtube_ask(
    config: PlatformRouteConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    video: &PlatformItem,
    question: &str,
) -> Result<ModelAnswerOutcome, ProviderError> {
    let PlatformRouteConfig::GeminiApi(config) = config else {
        unreachable!("planning selects only ask routes")
    };
    gemini_api::GeminiApi::build(config, route_context(client, retry_policy, deadline))
        .ask(video, question)
        .await
}

/// Transcribes a YouTube video, which the precheck read, through a transcribe route.
pub(crate) async fn youtube_transcribe(
    config: PlatformRouteConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    video: &PlatformItem,
) -> Result<ModelTranscriptOutcome, ProviderError> {
    let PlatformRouteConfig::GeminiApi(config) = config else {
        unreachable!("planning selects only transcribe routes")
    };
    gemini_api::GeminiApi::build(config, route_context(client, retry_policy, deadline))
        .transcribe(video)
        .await
}

/// Lists the current items of a Hacker News story list through a stories route.
pub(crate) async fn hn_stories(
    config: PlatformRouteConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    request: &HnStoriesRequest,
) -> Result<HnStoriesOutcome, ProviderError> {
    let PlatformRouteConfig::HnApi(config) = config else {
        unreachable!("planning selects only stories routes")
    };
    hn_api::HnApi::build(config, route_context(client, retry_policy, deadline))
        .stories(request)
        .await
}

/// Reads the Price History of one outcome of a Polymarket market through a history route.
pub(crate) async fn polymarket_history(
    config: PlatformRouteConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    request: &PolymarketHistoryRequest,
) -> Result<PolymarketHistoryOutcome, ProviderError> {
    let PlatformRouteConfig::PolymarketApi(config) = config else {
        unreachable!("planning selects only history routes")
    };
    polymarket_api::PolymarketApi::build(config, route_context(client, retry_policy, deadline))
        .history(request)
        .await
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

/// Returns whether a cited-by route can run the request, or `None` for a provider that does not
/// implement cited-by.
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "a single-route support check answers `None` for every provider but its route"
)]
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

/// Returns whether a comments route can run, or `None` for a provider that does not list
/// Xiaohongshu comments. The request needs no check beyond what parsing guarantees.
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "a single-route support check answers `None` for every provider but its route"
)]
pub(crate) fn xiaohongshu_comments_support(id: ProviderId) -> Option<Result<(), String>> {
    match id {
        ProviderId::XiaohongshuBrowser => Some(transport_support(id)),
        _ => None,
    }
}

/// Returns whether a comments route can run, or `None` for a provider that does not list
/// YouTube comments. The request needs no check beyond what parsing guarantees.
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "a single-route support check answers `None` for every provider but its route"
)]
pub(crate) fn youtube_comments_support(id: ProviderId) -> Option<Result<(), String>> {
    match id {
        ProviderId::YoutubeApi => Some(transport_support(id)),
        _ => None,
    }
}

/// Returns whether a videos route can list the requested page, or `None` for a provider that
/// does not list the videos of a YouTube collection.
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "a single-route support check answers `None` for every provider but its route"
)]
pub(crate) fn youtube_videos_support(
    id: ProviderId,
    request: &YoutubeVideosRequest,
) -> Option<Result<(), String>> {
    let route = match id {
        ProviderId::YoutubeApi => youtube_api::videos_support(request),
        _ => return None,
    };
    Some(transport_support(id).and(route))
}

/// Returns whether an ask route can run, or `None` for a provider that does not answer from X
/// posts. The request needs no check beyond what parsing guarantees.
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "a single-route support check answers `None` for every provider but its route"
)]
pub(crate) fn x_ask_support(id: ProviderId) -> Option<Result<(), String>> {
    match id {
        ProviderId::Xai => Some(transport_support(id)),
        _ => None,
    }
}

/// Returns whether an ask route can run, or `None` for a provider that does not answer about
/// YouTube videos. The request needs no check beyond what parsing guarantees.
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "a single-route support check answers `None` for every provider but its route"
)]
pub(crate) fn youtube_ask_support(id: ProviderId) -> Option<Result<(), String>> {
    match id {
        ProviderId::GeminiApi => Some(transport_support(id)),
        _ => None,
    }
}

/// Returns whether a transcribe route can run, or `None` for a provider that does not
/// transcribe YouTube videos. The request needs no check beyond what parsing guarantees.
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "a single-route support check answers `None` for every provider but its route"
)]
pub(crate) fn youtube_transcribe_support(id: ProviderId) -> Option<Result<(), String>> {
    match id {
        ProviderId::GeminiApi => Some(transport_support(id)),
        _ => None,
    }
}

/// Returns whether a stories route can run, or `None` for a provider that does not list Hacker
/// News stories. The request needs no check beyond what parsing guarantees.
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "a single-route support check answers `None` for every provider but its route"
)]
pub(crate) fn hn_stories_support(id: ProviderId) -> Option<Result<(), String>> {
    match id {
        ProviderId::HnApi => Some(transport_support(id)),
        _ => None,
    }
}

/// Returns whether a history route can run, or `None` for a provider that does not read
/// Polymarket price histories. The request needs no check beyond what parsing guarantees.
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "a single-route support check answers `None` for every provider but its route"
)]
pub(crate) fn polymarket_history_support(id: ProviderId) -> Option<Result<(), String>> {
    match id {
        ProviderId::PolymarketApi => Some(transport_support(id)),
        _ => None,
    }
}

/// Returns the login into `platform`'s Browser Profile: the platform's browser route whose
/// environment declares that it needs a login, with that route's login page logic. `None` for a
/// platform without one.
pub(crate) fn browser_login(platform: Platform) -> Option<BrowserLogin> {
    registrations().iter().find_map(|registration| {
        let ProviderTransport::Browser(environment) = registration.transport else {
            return None;
        };
        if environment.profile != platform || !environment.requires_login {
            return None;
        }
        Some(BrowserLogin {
            route: registration.id,
            profile: environment.profile.as_str(),
            language: environment.language,
            page: declared_adapter(registration.id)?.login_page?,
        })
    })
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
        ProviderTransport::Browser(environment) => {
            browser_preflight(&environment, &runtime.browser)
        }
        ProviderTransport::Http if runtime.provider_configured(id) => Ok(()),
        ProviderTransport::Http => Err(format!(
            "{} has no configured credentials",
            provider_keys_key(id.name())
        )),
    }
}
