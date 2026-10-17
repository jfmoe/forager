use std::sync::Arc;

use reqwest::Client;

use super::constructors::{
    build_chatgpt_browser, build_gemini_browser, credentials, route_limiter,
};
use super::{
    ArxivApi, DocsSearch, GeminiApi, HnApi, KeyAccount, MainSearch, ModelBreakers, PlatformFetch,
    PlatformSearch, ProviderId, Serpapi, SsrnBrowser, SsrnCrossref, SupplementalSearch,
    VerticalSearch, WebFetch, WebSearch, XiaohongshuBrowser, YoutubeApi, YoutubeWeb, arxiv, hn_api,
    opencli, serpapi, ssrn_browser, ssrn_crossref, web_fetch, xai, xiaohongshu_browser,
    youtube_api, youtube_web,
};
use crate::catalog::{ProviderTransport, VERTICAL_SEARCH, WEB_FETCH, WEB_SEARCH, registration};
use crate::config::{
    AnysearchRuntimeConfig, DocsSearchProviderConfig, HnRouteRuntimeConfig, HttpRouteRuntimeConfig,
    KeyedHttpRouteRuntimeConfig, MainSearchProviderConfig, ModelRouteRuntimeConfig,
    PlatformRouteConfig, ProcessRouteRuntimeConfig, RuntimeConfig, WebFetchProviderConfig,
};
use crate::net::RetryPolicy;
use crate::types::{
    Deadline, HnStoriesOutcome, HnStoriesRequest, ModelAnswerOutcome, ModelTranscriptOutcome,
    PlatformFetchRequest, PlatformItem, PlatformSearchOutcome, PlatformSearchRequest,
    ProviderError, ScholarCitedByRequest, XAskOutcome, XAskRequest, XiaohongshuCommentsOutcome,
    XiaohongshuCommentsRequest, YoutubeCommentsOutcome, YoutubeCommentsRequest,
    YoutubeVideosOutcome, YoutubeVideosRequest,
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
        PlatformRouteConfig::XiaohongshuBrowser(config) => {
            Box::new(build_xiaohongshu_browser(config, deadline))
        }
        PlatformRouteConfig::Xai(config) => {
            Box::new(super::build_xai(config, client, retry_policy, deadline))
        }
        PlatformRouteConfig::HnApi(config) => {
            Box::new(build_hn_api(config, client, retry_policy, deadline))
        }
        PlatformRouteConfig::YoutubeApi(config) => {
            Box::new(build_youtube_api(config, client, retry_policy, deadline))
        }
        PlatformRouteConfig::YoutubeWeb(_) => {
            unreachable!("youtube_web serves fetch only, so planning never selects it for search")
        }
        PlatformRouteConfig::GeminiApi(_) => {
            unreachable!(
                "gemini_api serves model operations only, so planning never selects it for search"
            )
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
        PlatformRouteConfig::XiaohongshuBrowser(config) => {
            Box::new(build_xiaohongshu_browser(config, deadline))
        }
        PlatformRouteConfig::Xai(config) => {
            Box::new(super::build_xai(config, client, retry_policy, deadline))
        }
        PlatformRouteConfig::HnApi(config) => {
            Box::new(build_hn_api(config, client, retry_policy, deadline))
        }
        PlatformRouteConfig::YoutubeApi(config) => {
            Box::new(build_youtube_api(config, client, retry_policy, deadline))
        }
        PlatformRouteConfig::YoutubeWeb(config) => {
            Box::new(build_youtube_web(config, client, deadline))
        }
        PlatformRouteConfig::GeminiApi(_) => {
            unreachable!(
                "gemini_api serves model operations only, so planning never selects it for fetch"
            )
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
    super::build_xai(config, client, retry_policy, deadline)
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
    build_xiaohongshu_browser(config, deadline)
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
    build_youtube_api(config, client, retry_policy, deadline)
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
    build_youtube_api(config, client, retry_policy, deadline)
        .videos(request)
        .await
}

/// Answers a question about a YouTube video, which the metadata check read, through an ask
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
    build_gemini_api(config, client, retry_policy, deadline)
        .ask(video, question)
        .await
}

/// Transcribes a YouTube video, which the metadata check read, through a transcribe route.
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
    build_gemini_api(config, client, retry_policy, deadline)
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
    build_hn_api(config, client, retry_policy, deadline)
        .stories(request)
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
    match config {
        PlatformRouteConfig::HnApi(config) => vec![(
            "firebase",
            build_hn_api(config, client, retry_policy, deadline)
                .probe_firebase()
                .await,
        )],
        _ => Vec::new(),
    }
}

/// Checks the account of every key of a route that registers an account probe.
pub(crate) async fn route_accounts(
    config: PlatformRouteConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Vec<KeyAccount> {
    match config {
        PlatformRouteConfig::Serpapi(config) => {
            build_serpapi(config, client, retry_policy, deadline)
                .accounts()
                .await
        }
        PlatformRouteConfig::GeminiApi(config) => {
            build_gemini_api(config, client, retry_policy, deadline)
                .accounts()
                .await
                .into_iter()
                .map(|result| KeyAccount {
                    reports_quota: false,
                    quota: None,
                    failure: result.err(),
                })
                .collect()
        }
        other => unreachable!("{} registers no account probe", other.route().name()),
    }
}

/// Runs the read-only `status` command of a provider that registers an adapter status probe.
pub(crate) async fn adapter_status(
    id: ProviderId,
    config: &RuntimeConfig,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Result<(), ProviderError> {
    match id {
        ProviderId::GeminiBrowser => {
            build_gemini_browser(config.gemini_browser.clone(), retry_policy, deadline)
                .status()
                .await
        }
        ProviderId::ChatgptBrowser => {
            build_chatgpt_browser(config.chatgpt_browser.clone(), retry_policy, deadline)
                .status()
                .await
        }
        other => unreachable!("{} registers no adapter status probe", other.name()),
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

fn build_hn_api(
    config: HnRouteRuntimeConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> HnApi {
    HnApi::new(
        config,
        client,
        route_limiter(ProviderId::HnApi).expect("hn_api registration declares an access policy"),
        retry_policy,
        deadline,
    )
}

fn build_youtube_api(
    mut config: KeyedHttpRouteRuntimeConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> YoutubeApi {
    let credentials = credentials(ProviderId::YoutubeApi, &mut config.keys);
    YoutubeApi::new(
        config,
        client,
        credentials,
        route_limiter(ProviderId::YoutubeApi)
            .expect("youtube_api registration declares an access policy"),
        retry_policy,
        deadline,
    )
}

fn build_gemini_api(
    mut config: ModelRouteRuntimeConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> GeminiApi {
    let credentials = credentials(ProviderId::GeminiApi, &mut config.keys);
    GeminiApi::new(
        config,
        client,
        credentials,
        route_limiter(ProviderId::GeminiApi)
            .expect("gemini_api registration declares an access policy"),
        retry_policy,
        deadline,
    )
}

// The route never retries, so the shared retry policy does not reach it.
fn build_youtube_web(
    config: HttpRouteRuntimeConfig,
    client: Client,
    deadline: Deadline,
) -> YoutubeWeb {
    YoutubeWeb::new(
        config,
        client,
        route_limiter(ProviderId::YoutubeWeb)
            .expect("youtube_web registration declares an access policy"),
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

fn build_xiaohongshu_browser(
    config: ProcessRouteRuntimeConfig,
    deadline: Deadline,
) -> XiaohongshuBrowser {
    XiaohongshuBrowser::new(
        config,
        route_limiter(ProviderId::XiaohongshuBrowser)
            .expect("xiaohongshu_browser registration declares an access policy"),
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
        ProviderId::XiaohongshuBrowser => xiaohongshu_browser::search_support(request),
        ProviderId::Xai => xai::search_support(request),
        ProviderId::HnApi => hn_api::search_support(request),
        ProviderId::YoutubeApi => youtube_api::search_support(request),
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
        ProviderId::XiaohongshuBrowser => xiaohongshu_browser::fetch_support(request),
        ProviderId::Xai => xai::fetch_support(request),
        ProviderId::HnApi => hn_api::fetch_support(request),
        ProviderId::YoutubeApi => youtube_api::fetch_support(request),
        ProviderId::YoutubeWeb => youtube_web::fetch_support(request),
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

/// Returns whether a comments route can run, or `None` for a provider that does not list
/// Xiaohongshu comments. The request needs no check beyond what parsing guarantees.
pub(crate) fn xiaohongshu_comments_support(id: ProviderId) -> Option<Result<(), String>> {
    match id {
        ProviderId::XiaohongshuBrowser => Some(transport_support(id)),
        _ => None,
    }
}

/// Returns whether a comments route can run, or `None` for a provider that does not list
/// YouTube comments. The request needs no check beyond what parsing guarantees.
pub(crate) fn youtube_comments_support(id: ProviderId) -> Option<Result<(), String>> {
    match id {
        ProviderId::YoutubeApi => Some(transport_support(id)),
        _ => None,
    }
}

/// Returns whether a videos route can list the requested page, or `None` for a provider that
/// does not list the videos of a YouTube collection.
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
pub(crate) fn x_ask_support(id: ProviderId) -> Option<Result<(), String>> {
    match id {
        ProviderId::Xai => Some(transport_support(id)),
        _ => None,
    }
}

/// Returns whether an ask route can run, or `None` for a provider that does not answer about
/// YouTube videos. The request needs no check beyond what parsing guarantees.
pub(crate) fn youtube_ask_support(id: ProviderId) -> Option<Result<(), String>> {
    match id {
        ProviderId::GeminiApi => Some(transport_support(id)),
        _ => None,
    }
}

/// Returns whether a transcribe route can run, or `None` for a provider that does not
/// transcribe YouTube videos. The request needs no check beyond what parsing guarantees.
pub(crate) fn youtube_transcribe_support(id: ProviderId) -> Option<Result<(), String>> {
    match id {
        ProviderId::GeminiApi => Some(transport_support(id)),
        _ => None,
    }
}

/// Returns whether a stories route can run, or `None` for a provider that does not list Hacker
/// News stories. The request needs no check beyond what parsing guarantees.
pub(crate) fn hn_stories_support(id: ProviderId) -> Option<Result<(), String>> {
    match id {
        ProviderId::HnApi => Some(transport_support(id)),
        _ => None,
    }
}

/// Returns whether this host can run the route's transport at all.
fn transport_support(id: ProviderId) -> Result<(), String> {
    match registration(id).transport {
        ProviderTransport::Http => Ok(()),
        ProviderTransport::OpenCli(_) => opencli::host_support()
            .map_err(|reason| format!("{} cannot run here: {reason}", id.name())),
    }
}
