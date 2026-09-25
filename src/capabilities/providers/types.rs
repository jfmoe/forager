use std::future::Future;
use std::pin::Pin;

use chrono::{Datelike, Local, Weekday};
use thiserror::Error;

use super::{
    Anysearch, AnysearchSearchRequest, Context7, Context7DocsRequest, Context7LibraryRequest, Exa,
    ExaSearchRequest, OpenAiCompatible, SearchType, SupplementalSearch, Xai,
};
use crate::redact::redact_url;
use crate::types::{
    AnysearchOutcome, AttemptErrorKind, Context7Outcome, DocumentationEvidence,
    DocumentationSearchOutcome, EvidenceLocator, ExaOutcome, ProviderAttempt, SearchCandidate,
    Source, SupplementalSearchOutcome, VerticalSearchOutcome,
};

#[derive(Clone, Debug)]
pub(crate) struct MainSearchRequest {
    pub(crate) query: String,
    pub(crate) model: Option<String>,
    pub(crate) allow_model_fallback: bool,
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
            Self::Search => main_search_input(query),
            Self::ModelProbe => query.to_owned(),
        }
    }

    pub(super) fn uses_search_tools(self) -> bool {
        matches!(self, Self::Search)
    }
}

fn main_search_input(query: &str) -> String {
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

type DocumentationReadFuture<'a> =
    Pin<Box<dyn Future<Output = Result<DocumentationEvidence, ProviderError>> + Send + 'a>>;

pub(crate) trait DocsSearch: Send + Sync {
    fn search<'a>(
        &'a self,
        query: &'a str,
        limit: u16,
    ) -> Pin<Box<dyn Future<Output = Result<DocumentationSearchOutcome, ProviderError>> + Send + 'a>>;

    fn read<'a>(
        &'a self,
        _locator: &'a EvidenceLocator,
        _query: &'a str,
    ) -> Option<DocumentationReadFuture<'a>> {
        None
    }
}

pub(crate) trait VerticalSearch: Send + Sync {
    fn search<'a>(
        &'a self,
        query: &'a str,
        limit: u16,
    ) -> Pin<Box<dyn Future<Output = Result<VerticalSearchOutcome, ProviderError>> + Send + 'a>>;
}

impl VerticalSearch for Anysearch {
    fn search<'a>(
        &'a self,
        query: &'a str,
        limit: u16,
    ) -> Pin<Box<dyn Future<Output = Result<VerticalSearchOutcome, ProviderError>> + Send + 'a>>
    {
        Box::pin(async move {
            let AnysearchOutcome::Search(outcome) = self
                .search(AnysearchSearchRequest {
                    query: query.to_owned(),
                    domain: None,
                    sub_domain: None,
                    sub_domain_params: serde_json::Map::new(),
                    max_results: limit,
                    verbose: true,
                })
                .await?
            else {
                unreachable!("search request returns search outcome");
            };
            let sources = outcome
                .results
                .iter()
                .filter(|result| !result.url.is_empty())
                .map(|result| Source {
                    title: result.title.clone(),
                    url: redact_url(&result.url),
                    published_date: None,
                    author: None,
                    text: (!result.description.is_empty()).then(|| result.description.clone()),
                    highlights: Vec::new(),
                    id: None,
                    image: None,
                    favicon: None,
                })
                .collect();
            Ok(VerticalSearchOutcome {
                results: outcome.results,
                sources,
                attempts: outcome.attempts,
                diagnostic: outcome.diagnostic,
            })
        })
    }
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

impl DocsSearch for Context7 {
    fn search<'a>(
        &'a self,
        query: &'a str,
        limit: u16,
    ) -> Pin<Box<dyn Future<Output = Result<DocumentationSearchOutcome, ProviderError>> + Send + 'a>>
    {
        Box::pin(async move {
            let Context7Outcome::Library(library) = self
                .library(Context7LibraryRequest {
                    name: query.to_owned(),
                    query: String::new(),
                    verbose: true,
                })
                .await?
            else {
                unreachable!("library request returns library outcome");
            };
            Ok(DocumentationSearchOutcome {
                candidate_sources: library
                    .results
                    .into_iter()
                    .filter_map(SearchCandidate::from_context7_library)
                    .take(usize::from(limit))
                    .collect(),
                attempts: library.attempts,
                diagnostic: library.diagnostic,
            })
        })
    }

    fn read<'a>(
        &'a self,
        locator: &'a EvidenceLocator,
        query: &'a str,
    ) -> Option<DocumentationReadFuture<'a>> {
        let EvidenceLocator::Context7Library(library_id) = locator else {
            return None;
        };
        Some(Box::pin(async move {
            let Context7Outcome::Docs(docs) = self
                .docs(Context7DocsRequest {
                    library_id: library_id.clone(),
                    query: query.to_owned(),
                    verbose: true,
                })
                .await?
            else {
                unreachable!("docs request returns docs outcome");
            };
            Ok(DocumentationEvidence {
                locator: EvidenceLocator::Context7Library(docs.library_id),
                provider: docs.provider,
                content: docs.content,
                attempts: docs.attempts,
                diagnostic: docs.diagnostic,
            })
        }))
    }
}

impl MainSearch for Xai {
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

impl MainSearch for OpenAiCompatible {
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

#[derive(Debug, Error)]
#[error("{message}")]
pub struct ProviderError {
    pub kind: AttemptErrorKind,
    pub message: String,
    pub attempts: Vec<ProviderAttempt>,
    pub verbose: bool,
    pub diagnostic: Option<String>,
    pub redirected_library_id: Option<String>,
}
