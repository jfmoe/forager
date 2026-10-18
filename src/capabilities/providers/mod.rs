mod arxiv;
mod chatgpt_browser;
mod constructors;
mod context7;
mod exa;
pub(crate) mod execution;
mod factory;
mod gemini_api;
mod gemini_browser;
mod hn_api;
mod key_account;
mod markup;
mod opencli;
mod serpapi;
pub(crate) mod shared;
mod ssrn_browser;
mod ssrn_crossref;
mod supplemental;
mod tavily_map;
mod transcript;
mod types;
mod web_fetch;
mod xai;
mod xiaohongshu_browser;
mod youtube_api;
mod youtube_transcript;
mod youtube_web;

pub(crate) use crate::catalog::ProviderId;
pub(crate) use arxiv::ArxivApi;
pub(crate) use chatgpt_browser::ChatgptBrowser;
pub(crate) use constructors::{
    build_chatgpt_browser, build_context7, build_exa, build_gemini_browser, build_tavily_map,
    build_xai, route_limiter,
};
pub(crate) use context7::{Context7, Context7DocsRequest, Context7LibraryRequest};
pub(crate) use exa::{Exa, ExaSearchRequest, ExaSimilarRequest, SearchType};
pub(crate) use factory::{
    HN_STORIES_ROUTES, SCHOLAR_CITED_BY_ROUTES, X_ASK_ROUTES, XIAOHONGSHU_COMMENTS_ROUTES,
    YOUTUBE_ASK_ROUTES, YOUTUBE_COMMENTS_ROUTES, YOUTUBE_TRANSCRIBE_ROUTES, YOUTUBE_VIDEOS_ROUTES,
    adapter_status, build_docs_search, build_main_search, build_platform_fetch,
    build_platform_search, build_web_fetch, build_web_search, hn_stories, hn_stories_support,
    platform_fetch_support, platform_route_extra_checks, platform_search_support, route_accounts,
    scholar_cited_by, scholar_cited_by_support, x_ask, x_ask_support, xiaohongshu_comments,
    xiaohongshu_comments_support, youtube_ask, youtube_ask_support, youtube_comments,
    youtube_comments_support, youtube_transcribe, youtube_transcribe_support, youtube_videos,
    youtube_videos_support,
};
pub(crate) use gemini_api::GeminiApi;
pub(crate) use gemini_browser::GeminiBrowser;
pub(crate) use hn_api::HnApi;
pub(crate) use key_account::{KeyAccount, KeyQuota};
pub(crate) use opencli::check_contract as check_opencli_contract;
pub(crate) use serpapi::Serpapi;
pub(crate) use ssrn_browser::SsrnBrowser;
pub(crate) use ssrn_crossref::SsrnCrossref;
pub(crate) use supplemental::SupplementalSearch;
pub(crate) use tavily_map::{MapRequest, TavilyMap};
pub(crate) use types::{
    DocsSearch, MainSearch, MainSearchRequest, MainSearchRequestKind, PlatformFetch,
    PlatformSearch, WebSearch,
};
pub(crate) use web_fetch::{FetchRequest, FetchSource, WebFetch, is_pdf_url, source_support};
pub(crate) use xai::Xai;
pub(crate) use xiaohongshu_browser::XiaohongshuBrowser;
pub(crate) use youtube_api::YoutubeApi;
pub(crate) use youtube_web::YoutubeWeb;
