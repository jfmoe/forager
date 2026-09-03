use std::future::Future;
use std::pin::Pin;
use std::sync::LazyLock;

use chrono::{Datelike, Local, Weekday};

mod anysearch;
pub(crate) mod catalog;
mod context7;
mod exa;
pub(crate) mod execution;
mod openai_compatible;
pub(crate) mod shared;
mod supplemental;
mod tavily_map;
mod web_fetch;
mod xai;

use reqwest::Client;
use thiserror::Error;

use crate::config::{
    AnysearchRuntimeConfig, Context7RuntimeConfig, DocsSearchProviderConfig, ExaRuntimeConfig,
    MainSearchProviderConfig, OpenAiCompatibleRuntimeConfig, ProviderRuntime, RuntimeConfig,
    WebFetchProviderConfig, XaiRuntimeConfig,
};
use crate::credentials::CredentialPool;
use crate::net::RetryPolicy;
use crate::redact::redact_url;
use crate::types::{
    AnysearchOutcome, Context7Outcome, DocumentationEvidence, DocumentationSearchOutcome,
    EvidenceLocator, ExaOutcome, SearchCandidate, Source, SupplementalSearchOutcome,
    VerticalSearchOutcome,
};
use crate::types::{AttemptErrorKind, Deadline, ProviderAttempt};

pub(crate) use anysearch::{Anysearch, AnysearchDomainsRequest, AnysearchSearchRequest};
pub(crate) use context7::{Context7, Context7DocsRequest, Context7LibraryRequest};
pub(crate) use exa::{Exa, ExaSearchRequest, ExaSimilarRequest, SearchType};
pub(crate) use openai_compatible::{ModelBreakers, OpenAiCompatible};
pub(crate) use supplemental::SupplementalSearch;
pub(crate) use tavily_map::{MapRequest, TavilyMap};
pub(crate) use web_fetch::{FetchRequest, WebFetch};
pub(crate) use xai::Xai;

#[derive(Clone, Debug)]
pub(crate) struct MainSearchRequest {
    pub(crate) query: String,
    pub(crate) model: Option<String>,
    pub(crate) allow_model_fallback: bool,
    pub(crate) verbose: bool,
}

const MAIN_SEARCH_INSTRUCTION: &str = "You are a helpful research assistant. Answer the user's question thoroughly using web search results.\n\nGuidelines:\n- Infer the user's true intent even when the question is vague. Consider multiple angles.\n- Search broadly first (5+ perspectives), then go deep on the 2-3 most relevant ones.\n- Prioritize authoritative sources: official docs, Wikipedia, academic papers, reputable journalism.\n- Search in English first for breadth, switch to Chinese when the topic demands it.\n- Every factual claim should cite its source. More credible sources strengthen the answer.\n- Lead with the most likely answer, then provide supporting analysis.\n- Define technical terms in plain language. Use real-world analogies for complex concepts.\n- Format output in clean Markdown. Use LaTeX for formulas, code blocks for scripts.\n- Be direct and concise. No filler or unnecessary follow-up questions.\n";

#[derive(Clone, Copy)]
pub(crate) enum MainSearchRequestKind {
    Search,
    ModelProbe,
}

impl MainSearchRequestKind {
    fn instruction(self) -> Option<&'static str> {
        match self {
            Self::Search => Some(MAIN_SEARCH_INSTRUCTION),
            Self::ModelProbe => None,
        }
    }

    fn input(self, query: &str) -> String {
        match self {
            Self::Search => main_search_input(query),
            Self::ModelProbe => query.to_owned(),
        }
    }

    fn uses_search_tools(self) -> bool {
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
                    include_text: false,
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

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum ProviderId {
    Xai,
    OpenAiCompatible,
    Exa,
    Tavily,
    Firecrawl,
    Jina,
    Context7,
    Anysearch,
}

impl ProviderId {
    pub(crate) const ALL: [Self; 8] = [
        Self::Xai,
        Self::OpenAiCompatible,
        Self::Exa,
        Self::Tavily,
        Self::Firecrawl,
        Self::Jina,
        Self::Context7,
        Self::Anysearch,
    ];

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "xai" => Some(Self::Xai),
            "openai_compatible" => Some(Self::OpenAiCompatible),
            "exa" => Some(Self::Exa),
            "tavily" => Some(Self::Tavily),
            "firecrawl" => Some(Self::Firecrawl),
            "jina" => Some(Self::Jina),
            "context7" => Some(Self::Context7),
            "anysearch" => Some(Self::Anysearch),
            _ => None,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Xai => "xai",
            Self::OpenAiCompatible => "openai_compatible",
            Self::Exa => "exa",
            Self::Tavily => "tavily",
            Self::Firecrawl => "firecrawl",
            Self::Jina => "jina",
            Self::Context7 => "context7",
            Self::Anysearch => "anysearch",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ProbeShape {
    pub(crate) name: &'static str,
    pub(crate) transport: &'static str,
    pub(crate) stream: Option<bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DoctorProbe {
    MainSearch(&'static [ProbeShape]),
    WebSearch {
        name: &'static str,
        transport: &'static str,
    },
    WebFetch {
        name: &'static str,
        transport: &'static str,
    },
    DocsSearch {
        name: &'static str,
        transport: &'static str,
    },
    AnysearchDomains {
        name: &'static str,
        transport: &'static str,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ProviderSmokeCase {
    pub(crate) id: &'static str,
    pub(crate) operation: &'static str,
    pub(crate) transport: &'static str,
}

#[derive(Clone, Copy)]
pub(crate) struct ProviderRegistration {
    pub(crate) id: ProviderId,
    pub(crate) operations: &'static [&'static str],
    pub(crate) credentials_required: bool,
    pub(crate) probe: DoctorProbe,
    pub(crate) smoke_cases: &'static [ProviderSmokeCase],
    runtime: for<'a> fn(&'a RuntimeConfig) -> ProviderRuntime<'a>,
}

impl ProviderRegistration {
    pub(crate) fn runtime<'a>(&self, config: &'a RuntimeConfig) -> ProviderRuntime<'a> {
        (self.runtime)(config)
    }
}

const XAI_PROBES: &[ProbeShape] = &[ProbeShape {
    name: "responses",
    transport: "sse",
    stream: None,
}];
const OPENAI_PROBES: &[ProbeShape] = &[
    ProbeShape {
        name: "non_stream",
        transport: "http",
        stream: Some(false),
    },
    ProbeShape {
        name: "stream",
        transport: "sse",
        stream: Some(true),
    },
];

fn xai_runtime(config: &RuntimeConfig) -> ProviderRuntime<'_> {
    ProviderRuntime {
        endpoint: &config.xai.url,
        keys: &config.xai.keys,
    }
}

fn openai_runtime(config: &RuntimeConfig) -> ProviderRuntime<'_> {
    ProviderRuntime {
        endpoint: &config.openai_compatible.url,
        keys: &config.openai_compatible.keys,
    }
}

fn exa_runtime(config: &RuntimeConfig) -> ProviderRuntime<'_> {
    ProviderRuntime {
        endpoint: &config.exa.url,
        keys: &config.exa.keys,
    }
}

fn tavily_runtime(config: &RuntimeConfig) -> ProviderRuntime<'_> {
    ProviderRuntime {
        endpoint: &config.tavily.url,
        keys: &config.tavily.keys,
    }
}

fn firecrawl_runtime(config: &RuntimeConfig) -> ProviderRuntime<'_> {
    ProviderRuntime {
        endpoint: &config.firecrawl.url,
        keys: &config.firecrawl.keys,
    }
}

fn jina_runtime(config: &RuntimeConfig) -> ProviderRuntime<'_> {
    ProviderRuntime {
        endpoint: &config.jina.url,
        keys: &config.jina.keys,
    }
}

fn context7_runtime(config: &RuntimeConfig) -> ProviderRuntime<'_> {
    ProviderRuntime {
        endpoint: &config.context7.url,
        keys: &config.context7.keys,
    }
}

fn anysearch_runtime(config: &RuntimeConfig) -> ProviderRuntime<'_> {
    ProviderRuntime {
        endpoint: &config.anysearch.url,
        keys: &config.anysearch.keys,
    }
}

const XAI_SMOKE: &[ProviderSmokeCase] = &[ProviderSmokeCase {
    id: "C01",
    operation: "main_search",
    transport: "sse",
}];
const OPENAI_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C02",
        operation: "main_search_stream_false",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C03",
        operation: "main_search_stream_true",
        transport: "sse",
    },
];
const TAVILY_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C05",
        operation: "web_search",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C08",
        operation: "web_fetch",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C17",
        operation: "site_map",
        transport: "http",
    },
];
const FIRECRAWL_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C06",
        operation: "web_search",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C09",
        operation: "web_fetch",
        transport: "http",
    },
];
const JINA_SMOKE: &[ProviderSmokeCase] = &[ProviderSmokeCase {
    id: "C07",
    operation: "web_fetch",
    transport: "http",
}];
const CONTEXT7_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C10",
        operation: "library_resolve",
        transport: "mcp",
    },
    ProviderSmokeCase {
        id: "C11",
        operation: "docs",
        transport: "mcp",
    },
];
const EXA_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C12",
        operation: "docs_search",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C13",
        operation: "similar",
        transport: "http",
    },
];
const ANYSEARCH_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C14",
        operation: "academic.search",
        transport: "mcp",
    },
    ProviderSmokeCase {
        id: "C15",
        operation: "vertical_discovery",
        transport: "mcp",
    },
    ProviderSmokeCase {
        id: "C16",
        operation: "domains",
        transport: "mcp",
    },
];

const REGISTRY: &[ProviderRegistration] = &[
    ProviderRegistration {
        id: ProviderId::Xai,
        operations: &[],
        credentials_required: true,
        probe: DoctorProbe::MainSearch(XAI_PROBES),
        smoke_cases: XAI_SMOKE,
        runtime: xai_runtime,
    },
    ProviderRegistration {
        id: ProviderId::OpenAiCompatible,
        operations: &[],
        credentials_required: true,
        probe: DoctorProbe::MainSearch(OPENAI_PROBES),
        smoke_cases: OPENAI_SMOKE,
        runtime: openai_runtime,
    },
    ProviderRegistration {
        id: ProviderId::Tavily,
        operations: &["site_map"],
        credentials_required: true,
        probe: DoctorProbe::WebSearch {
            name: "search",
            transport: "http",
        },
        smoke_cases: TAVILY_SMOKE,
        runtime: tavily_runtime,
    },
    ProviderRegistration {
        id: ProviderId::Firecrawl,
        operations: &[],
        credentials_required: true,
        probe: DoctorProbe::WebSearch {
            name: "search",
            transport: "http",
        },
        smoke_cases: FIRECRAWL_SMOKE,
        runtime: firecrawl_runtime,
    },
    ProviderRegistration {
        id: ProviderId::Jina,
        operations: &[],
        credentials_required: true,
        probe: DoctorProbe::WebFetch {
            name: "fetch",
            transport: "http",
        },
        smoke_cases: JINA_SMOKE,
        runtime: jina_runtime,
    },
    ProviderRegistration {
        id: ProviderId::Context7,
        operations: &[],
        credentials_required: true,
        probe: DoctorProbe::DocsSearch {
            name: "library",
            transport: "mcp",
        },
        smoke_cases: CONTEXT7_SMOKE,
        runtime: context7_runtime,
    },
    ProviderRegistration {
        id: ProviderId::Exa,
        operations: &["similar"],
        credentials_required: true,
        probe: DoctorProbe::DocsSearch {
            name: "search",
            transport: "http",
        },
        smoke_cases: EXA_SMOKE,
        runtime: exa_runtime,
    },
    ProviderRegistration {
        id: ProviderId::Anysearch,
        operations: &["search", "domains"],
        credentials_required: true,
        probe: DoctorProbe::AnysearchDomains {
            name: "domains",
            transport: "mcp",
        },
        smoke_cases: ANYSEARCH_SMOKE,
        runtime: anysearch_runtime,
    },
];

static VALIDATED_REGISTRY: LazyLock<()> = LazyLock::new(|| {
    if let Err(error) = validate_registrations(REGISTRY) {
        panic!("invalid provider registry: {error}");
    }
});

pub(crate) fn registrations() -> &'static [ProviderRegistration] {
    LazyLock::force(&VALIDATED_REGISTRY);
    REGISTRY
}

fn validate_registrations(registry: &[ProviderRegistration]) -> Result<(), String> {
    let ids = registry
        .iter()
        .map(|registration| registration.id)
        .collect::<std::collections::BTreeSet<_>>();
    let expected = ProviderId::ALL
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    if registry.len() != expected.len() || ids != expected {
        return Err("provider IDs must appear exactly once".into());
    }
    let catalog_ids = catalog::CATALOGS
        .iter()
        .flat_map(|catalog| catalog.providers.iter().copied())
        .collect::<std::collections::BTreeSet<_>>();
    for registration in registry {
        if !catalog_ids.contains(&registration.id) && registration.operations.is_empty() {
            return Err(format!(
                "{} has neither a capability nor an operation",
                registration.id.name()
            ));
        }
    }
    let smoke_ids = registry
        .iter()
        .flat_map(|registration| registration.smoke_cases.iter().map(|case| case.id))
        .collect::<Vec<_>>();
    let unique_smoke_ids = smoke_ids
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    if smoke_ids.len() != unique_smoke_ids.len() {
        return Err("provider smoke case IDs must be unique".into());
    }
    Ok(())
}

pub(crate) fn build_xai(
    mut config: XaiRuntimeConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Xai {
    let registration = registration(ProviderId::Xai);
    debug_assert!(registration.credentials_required);
    let credentials = CredentialPool::new(registration.id.name(), std::mem::take(&mut config.keys));
    Xai::new(config, client, credentials, retry_policy, deadline)
}

pub(crate) fn build_openai_compatible(
    mut config: OpenAiCompatibleRuntimeConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    breakers: std::sync::Arc<ModelBreakers>,
) -> OpenAiCompatible {
    let registration = registration(ProviderId::OpenAiCompatible);
    debug_assert!(registration.credentials_required);
    let credentials = CredentialPool::new(registration.id.name(), std::mem::take(&mut config.keys));
    OpenAiCompatible::new(
        config,
        client,
        credentials,
        retry_policy,
        deadline,
        breakers,
    )
}

pub(crate) fn build_main_search(
    id: ProviderId,
    config: MainSearchProviderConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    breakers: std::sync::Arc<ModelBreakers>,
) -> Box<dyn MainSearch> {
    catalog::build_main(id, config, client, retry_policy, deadline, breakers)
}

pub(crate) fn supports(capability: &str, provider: &str) -> bool {
    let Some(id) = ProviderId::parse(provider) else {
        return false;
    };
    catalog::by_seam(capability).is_some_and(|catalog| catalog.contains(id))
}

pub(crate) fn build_web_fetch(
    id: ProviderId,
    config: WebFetchProviderConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Box<dyn WebFetch> {
    catalog::build_web_fetch(id, config, client, retry_policy, deadline)
}

pub(crate) fn build_web_search(
    id: ProviderId,
    config: WebFetchProviderConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Box<dyn WebSearch> {
    catalog::build_web_search(id, config, client, retry_policy, deadline)
}

pub(crate) fn build_docs_search(
    id: ProviderId,
    config: DocsSearchProviderConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Box<dyn DocsSearch> {
    catalog::build_docs(id, config, client, retry_policy, deadline)
}

pub(crate) fn build_vertical_search(
    id: ProviderId,
    config: AnysearchRuntimeConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Box<dyn VerticalSearch> {
    catalog::build_vertical(id, config, client, retry_policy, deadline)
}

pub(crate) fn registration(id: ProviderId) -> &'static ProviderRegistration {
    registrations()
        .iter()
        .find(|registration| registration.id == id)
        .expect("validated registry contains every provider ID")
}

pub(crate) fn build_exa(
    mut config: ExaRuntimeConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> exa::Exa {
    let registration = registration(ProviderId::Exa);
    debug_assert!(registration.credentials_required);
    let credentials = CredentialPool::new(registration.id.name(), std::mem::take(&mut config.keys));
    exa::Exa::new(config, client, credentials, retry_policy, deadline)
}

pub(crate) fn build_tavily_map(
    mut config: WebFetchProviderConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> TavilyMap {
    let registration = registration(ProviderId::Tavily);
    debug_assert!(registration.credentials_required);
    debug_assert!(registration.operations.contains(&"site_map"));
    let credentials = CredentialPool::new(registration.id.name(), std::mem::take(&mut config.keys));
    TavilyMap::new(config, client, credentials, retry_policy, deadline)
}

pub(crate) fn build_context7(
    mut config: Context7RuntimeConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Context7 {
    let registration = registration(ProviderId::Context7);
    debug_assert!(registration.credentials_required);
    let credentials = CredentialPool::new(registration.id.name(), std::mem::take(&mut config.keys));
    Context7::new(config, client, credentials, retry_policy, deadline)
}

pub(crate) fn build_anysearch(
    mut config: AnysearchRuntimeConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Anysearch {
    let registration = registration(ProviderId::Anysearch);
    debug_assert!(registration.credentials_required);
    let credentials = CredentialPool::new(registration.id.name(), std::mem::take(&mut config.keys));
    Anysearch::new(config, client, credentials, retry_policy, deadline)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde::Deserialize;

    use super::{
        DoctorProbe, ProviderId, REGISTRY, catalog, registrations, validate_registrations,
    };

    #[derive(Deserialize)]
    struct AcceptanceManifest {
        transport_fixtures: Vec<TransportFixture>,
    }

    #[derive(Deserialize)]
    struct TransportFixture {
        provider: String,
        seam: String,
        test: String,
    }

    #[test]
    fn provider_fixture_projection_matches_transport_manifest() {
        let manifest: AcceptanceManifest = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/acceptance-manifest.json"
        )))
        .expect("acceptance manifest");
        let registry = catalog::CATALOGS
            .iter()
            .flat_map(|catalog| {
                catalog
                    .providers
                    .iter()
                    .map(move |provider| (provider.name(), catalog.seam))
            })
            .collect::<BTreeSet<_>>();
        let fixture_projection = manifest
            .transport_fixtures
            .iter()
            .map(|fixture| (fixture.provider.as_str(), fixture.seam.as_str()))
            .collect::<BTreeSet<_>>();

        assert_eq!(fixture_projection, registry);
        for fixture in manifest.transport_fixtures {
            assert!(
                !fixture.test.trim().is_empty(),
                "{} / {} lacks a fixture test",
                fixture.provider,
                fixture.seam
            );
        }
    }

    #[test]
    fn registration_lookup_is_identifier_based_and_rejects_missing_or_duplicate_ids() {
        let mut reordered = REGISTRY.to_vec();
        reordered.swap(0, 6);
        assert!(validate_registrations(&reordered).is_ok());
        assert_eq!(
            reordered
                .iter()
                .find(|registration| registration.id == ProviderId::Xai)
                .map(|registration| registration.id),
            Some(ProviderId::Xai)
        );

        let mut misaligned = reordered;
        misaligned[0] = misaligned[1];

        assert!(validate_registrations(&misaligned).is_err());
        for id in ProviderId::ALL {
            assert_eq!(super::registration(id).id, id);
        }
    }

    #[test]
    fn catalogs_project_every_registration_probe_and_smoke_case_consistently() {
        let catalog_ids = catalog::CATALOGS
            .iter()
            .flat_map(|catalog| catalog.providers.iter().copied())
            .collect::<BTreeSet<_>>();
        let registration_ids = registrations()
            .iter()
            .map(|registration| registration.id)
            .collect::<BTreeSet<_>>();
        assert_eq!(catalog_ids, registration_ids);

        for registration in registrations() {
            let probe_is_supported = match registration.probe {
                DoctorProbe::MainSearch(_) => catalog::MAIN_SEARCH.contains(registration.id),
                DoctorProbe::WebSearch { .. } => catalog::WEB_SEARCH.contains(registration.id),
                DoctorProbe::WebFetch { .. } => catalog::WEB_FETCH.contains(registration.id),
                DoctorProbe::DocsSearch { .. } => catalog::DOCS_SEARCH.contains(registration.id),
                DoctorProbe::AnysearchDomains { .. } => registration.id == ProviderId::Anysearch,
            };
            assert!(probe_is_supported, "{} probe", registration.id.name());
            assert!(
                !registration.smoke_cases.is_empty(),
                "{} smoke",
                registration.id.name()
            );
        }
    }
}
