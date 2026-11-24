mod arxiv_api;
mod browser;
mod browser_profile;
mod browser_route;
mod cdp;
mod constructors;
mod exa;
pub(crate) mod execution;
mod factory;
mod gemini_api;
mod hn_api;
mod key_account;
mod main_search_responses;
pub(crate) mod managed_browser;
mod markup;
mod openai;
mod polymarket_api;
mod reddit_browser;
mod responses_api;
mod route_declaration;
mod serpapi;
pub(crate) mod shared;
mod ssrn_crossref;
mod supplemental;
mod tavily_map;
#[cfg(test)]
mod test_projection;
mod transcript;
mod types;
mod web_fetch;
mod xai;
mod xiaohongshu_browser;
mod youtube_api;
mod youtube_transcript;
mod youtube_web;

pub(crate) use crate::catalog::ProviderId;
#[cfg(all(test, unix))]
pub(crate) use browser::real_tests as real_browser;
pub(crate) use browser::{
    BrowserLogin, BrowserSession, Page as BrowserPage, WindowMode, run as run_browser,
};
pub(crate) use browser_profile::{BrowserProfile, Screen as BrowserScreen};
pub(crate) use browser_route::preflight as browser_preflight;
pub(crate) use constructors::{build_exa, build_openai, build_tavily_map, route_limiter};
pub(crate) use exa::{Exa, ExaSearchRequest, ExaSimilarRequest, SearchType};
pub(crate) use factory::{
    browser_login, build_docs_search, build_main_search, build_platform_fetch,
    build_platform_search, build_web_fetch, build_web_search, operation_support,
    platform_fetch_support, platform_search_support, route_accounts, route_prerequisites,
};
#[cfg(test)]
pub(crate) use factory::{built_route_checks_accounts, declared_and_built_operations};
pub(crate) use gemini_api::{
    ASK_OPERATION as YOUTUBE_ASK, TRANSCRIBE_OPERATION as YOUTUBE_TRANSCRIBE,
};
pub(crate) use hn_api::STORIES_OPERATION as HN_STORIES;
pub(crate) use key_account::KeyAccount;
pub(crate) use openai::Openai;
pub(crate) use polymarket_api::HISTORY_OPERATION as POLYMARKET_HISTORY;
pub(crate) use route_declaration::{OperationRoute, RouteContext};
pub(crate) use serpapi::CITED_BY_OPERATION as SCHOLAR_CITED_BY;
pub(crate) use serpapi::KeyQuota;
pub(crate) use supplemental::SupplementalSearch;
pub(crate) use tavily_map::{MapRequest, TavilyMap};
pub(crate) use types::{
    DocsSearch, MainSearch, MainSearchRequest, PlatformFetch, PlatformSearch, WebSearch,
};
pub(crate) use web_fetch::{FetchRequest, WebFetch, is_pdf_url};
pub(crate) use xai::ASK_OPERATION as X_ASK;
pub(crate) use xiaohongshu_browser::COMMENTS_OPERATION as XIAOHONGSHU_COMMENTS;
pub(crate) use youtube_api::{
    COMMENTS_OPERATION as YOUTUBE_COMMENTS, VIDEOS_OPERATION as YOUTUBE_VIDEOS,
};
