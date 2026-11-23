use std::future::Future;
use std::pin::Pin;

use chrono::{Datelike, Local, Weekday};

use super::{Exa, ExaSearchRequest, Openai, SearchType, SupplementalSearch};
use crate::types::{
    DocumentationSearchOutcome, ExaOutcome, PlatformFetchOutcome, PlatformFetchRequest,
    PlatformSearchOutcome, PlatformSearchRequest, ProviderError, SearchCandidate,
    SupplementalSearchOutcome,
};

#[derive(Clone, Debug)]
pub(crate) struct MainSearchRequest {
    pub(crate) query: String,
    pub(crate) model: Option<String>,
    pub(crate) verbose: bool,
}

const MAIN_SEARCH_INSTRUCTION: &str = "You are a web research assistant. Another AI agent reads your answer and verifies key claims against the cited sources, so optimize for accurate, verifiable facts per word.\n\nGuidelines:\n- Infer the user's true intent even when the question is vague. If the question has several plausible readings, answer the most likely one and name the others in one line.\n- Search before answering. Match search breadth to the question: a single fact needs one or two targeted searches; a comparison, a contested topic, or a multi-part question needs several independent perspectives.\n- Prioritize primary and authoritative sources: official docs, standards, filings, academic papers, reputable journalism.\n- Search in English for breadth; also search in the question's language when the topic is local to it.\n- Cite a source for every factual claim. Keep versions, dates, numbers, and names exactly as the source states them. State when sources disagree or when no source supports a claim.\n- Lead with the direct answer, then the supporting details.\n- Write compact Markdown. Use code blocks for code and LaTeX for formulas.\n- Do not explain common terms, use analogies, restate the question, add a closing summary, or offer further help.\n";

#[derive(Clone, Copy)]
pub(crate) enum MainSearchRequestKind {
    Search,
    ModelProbe,
}

impl MainSearchRequestKind {
    pub(super) fn instruction(self) -> Option<&'static str> {
        match self {
            Self::Search => Some(MAIN_SEARCH_INSTRUCTION),
            Self::ModelProbe => None,
        }
    }

    pub(super) fn input(self, query: &str) -> String {
        match self {
            Self::Search => with_time_context(query),
            Self::ModelProbe => query.to_owned(),
        }
    }

    pub(super) fn uses_search_tools(self) -> bool {
        matches!(self, Self::Search)
    }
}

/// Prefixes `query` with the local date and time, so a model can resolve words like "recent".
pub(super) fn with_time_context(query: &str) -> String {
    let now = Local::now();
    let weekday = match now.weekday() {
        Weekday::Mon => "星期一",
        Weekday::Tue => "星期二",
        Weekday::Wed => "星期三",
        Weekday::Thu => "星期四",
        Weekday::Fri => "星期五",
        Weekday::Sat => "星期六",
        Weekday::Sun => "星期日",
    };
    format!(
        "[Current Time Context]\n- Date: {} ({weekday})\n- Time: {}\n- Timezone: {}\n\n{query}",
        now.format("%Y-%m-%d"),
        now.format("%H:%M:%S"),
        now.format("%:z"),
    )
}

pub(crate) trait MainSearch: Send + Sync {
    fn search(
        &self,
        request: MainSearchRequest,
    ) -> Pin<Box<dyn Future<Output = Result<crate::types::SearchOutcome, ProviderError>> + Send + '_>>;

    fn probe(
        &self,
        request: MainSearchRequest,
    ) -> Pin<Box<dyn Future<Output = Result<crate::types::SearchOutcome, ProviderError>> + Send + '_>>;
}

pub(crate) trait WebSearch: Send + Sync {
    fn search<'a>(
        &'a self,
        query: &'a str,
        limit: u16,
    ) -> Pin<Box<dyn Future<Output = Result<SupplementalSearchOutcome, ProviderError>> + Send + 'a>>;
}

impl WebSearch for SupplementalSearch {
    fn search<'a>(
        &'a self,
        query: &'a str,
        limit: u16,
    ) -> Pin<Box<dyn Future<Output = Result<SupplementalSearchOutcome, ProviderError>> + Send + 'a>>
    {
        Box::pin(SupplementalSearch::search(self, query, limit))
    }
}

pub(crate) trait DocsSearch: Send + Sync {
    fn search<'a>(
        &'a self,
        query: &'a str,
        limit: u16,
    ) -> Pin<Box<dyn Future<Output = Result<DocumentationSearchOutcome, ProviderError>> + Send + 'a>>;
}

/// The search seam of a platform; each route of the platform implements it.
pub(crate) trait PlatformSearch: Send + Sync {
    fn search<'a>(
        &'a self,
        request: &'a PlatformSearchRequest,
    ) -> Pin<Box<dyn Future<Output = Result<PlatformSearchOutcome, ProviderError>> + Send + 'a>>;
}

/// The fetch seam of a platform; each route of the platform implements it.
pub(crate) trait PlatformFetch: Send + Sync {
    fn fetch<'a>(
        &'a self,
        request: &'a PlatformFetchRequest,
    ) -> Pin<Box<dyn Future<Output = Result<PlatformFetchOutcome, ProviderError>> + Send + 'a>>;
}

impl DocsSearch for Exa {
    fn search<'a>(
        &'a self,
        query: &'a str,
        limit: u16,
    ) -> Pin<Box<dyn Future<Output = Result<DocumentationSearchOutcome, ProviderError>> + Send + 'a>>
    {
        Box::pin(async move {
            let ExaOutcome {
                results,
                attempts,
                diagnostic,
                ..
            } = self
                .search(ExaSearchRequest {
                    query: query.to_owned(),
                    num_results: limit,
                    search_type: SearchType::Auto,
                    text_max_characters: None,
                    include_highlights: true,
                    start_published_date: None,
                    include_domains: Vec::new(),
                    exclude_domains: Vec::new(),
                    category: None,
                    verbose: true,
                })
                .await?;
            let candidate_sources = results
                .into_iter()
                .filter_map(SearchCandidate::from_exa_source)
                .collect();
            Ok(DocumentationSearchOutcome {
                candidate_sources,
                attempts,
                diagnostic,
            })
        })
    }
}

impl MainSearch for Openai {
    fn search(
        &self,
        request: MainSearchRequest,
    ) -> Pin<Box<dyn Future<Output = Result<crate::types::SearchOutcome, ProviderError>> + Send + '_>>
    {
        Box::pin(self.search(request))
    }

    fn probe(
        &self,
        request: MainSearchRequest,
    ) -> Pin<Box<dyn Future<Output = Result<crate::types::SearchOutcome, ProviderError>> + Send + '_>>
    {
        Box::pin(self.probe(request))
    }
}
