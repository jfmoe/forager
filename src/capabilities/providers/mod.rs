mod anysearch;
mod constructors;
mod context7;
mod exa;
pub(crate) mod execution;
mod factory;
mod openai_compatible;
pub(crate) mod shared;
mod supplemental;
mod tavily_map;
mod types;
mod web_fetch;
mod xai;

pub(crate) use crate::catalog::ProviderId;
pub(crate) use anysearch::{Anysearch, AnysearchDomainsRequest, AnysearchSearchRequest};
pub(crate) use constructors::{
    build_anysearch, build_context7, build_exa, build_openai_compatible, build_tavily_map,
    build_xai,
};
pub(crate) use context7::{Context7, Context7DocsRequest, Context7LibraryRequest};
pub(crate) use exa::{Exa, ExaSearchRequest, ExaSimilarRequest, SearchType};
pub(crate) use factory::{
    build_docs_search, build_main_search, build_vertical_search, build_web_fetch, build_web_search,
};
pub(crate) use openai_compatible::{ModelBreakers, OpenAiCompatible};
pub(crate) use supplemental::SupplementalSearch;
pub(crate) use tavily_map::{MapRequest, TavilyMap};
pub use types::ProviderError;
pub(crate) use types::{
    DocsSearch, MainSearch, MainSearchRequest, MainSearchRequestKind, VerticalSearch, WebSearch,
};
pub(crate) use web_fetch::{FetchRequest, WebFetch};
pub(crate) use xai::Xai;
