use std::future::Future;
use std::pin::Pin;

use chrono::{Datelike, Local, Weekday};

mod anysearch;
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
    MainSearchProviderConfig, OpenAiCompatibleRuntimeConfig, WebFetchProviderConfig,
    XaiRuntimeConfig,
};
use crate::credentials::CredentialPool;
use crate::net::{RetryPolicy, combine_diagnostics};
use crate::redact::redact_url;
use crate::types::{
    AnysearchOutcome, Context7Outcome, ExaOutcome, Source, SupplementalSearchOutcome,
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
pub(crate) use xai::{SearchRequest, Xai};

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
        request: SearchRequest,
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
    ) -> Pin<Box<dyn Future<Output = Result<SupplementalSearchOutcome, ProviderError>> + Send + 'a>>;
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
    ) -> Pin<Box<dyn Future<Output = Result<SupplementalSearchOutcome, ProviderError>> + Send + 'a>>
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
            Ok(SupplementalSearchOutcome {
                sources: results,
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
        _limit: u16,
    ) -> Pin<Box<dyn Future<Output = Result<SupplementalSearchOutcome, ProviderError>> + Send + 'a>>
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
            let Some(candidate) = library.results.into_iter().next() else {
                return Ok(SupplementalSearchOutcome {
                    sources: Vec::new(),
                    attempts: library.attempts,
                    diagnostic: library.diagnostic,
                });
            };
            let docs_result = self
                .docs(Context7DocsRequest {
                    library_id: candidate.id,
                    query: query.to_owned(),
                    verbose: true,
                })
                .await;
            let Context7Outcome::Docs(mut docs) = (match docs_result {
                Ok(outcome) => outcome,
                Err(mut error) => {
                    error.attempts.splice(0..0, library.attempts);
                    error.diagnostic = combine_diagnostics(
                        [library.diagnostic, error.diagnostic].into_iter().flatten(),
                    );
                    return Err(error);
                }
            }) else {
                unreachable!("docs request returns docs outcome");
            };
            let mut attempts = library.attempts;
            attempts.append(&mut docs.attempts);
            Ok(SupplementalSearchOutcome {
                sources: Vec::new(),
                attempts,
                diagnostic: combine_diagnostics(
                    [library.diagnostic, docs.diagnostic].into_iter().flatten(),
                ),
            })
        })
    }
}

impl MainSearch for Xai {
    fn search(
        &self,
        request: SearchRequest,
    ) -> Pin<Box<dyn Future<Output = Result<crate::types::SearchOutcome, ProviderError>> + Send + '_>>
    {
        Box::pin(self.search(request))
    }
}

impl MainSearch for OpenAiCompatible {
    fn search(
        &self,
        request: SearchRequest,
    ) -> Pin<Box<dyn Future<Output = Result<crate::types::SearchOutcome, ProviderError>> + Send + '_>>
    {
        Box::pin(self.search(request))
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

#[derive(Clone, Copy)]
pub(crate) struct ProviderRegistration {
    pub(crate) id: ProviderId,
    pub(crate) name: &'static str,
    pub(crate) capabilities: &'static [&'static str],
    pub(crate) operations: &'static [&'static str],
    pub(crate) credentials_required: bool,
}

const REGISTRY: &[ProviderRegistration] = &[
    ProviderRegistration {
        id: ProviderId::Xai,
        name: "xai",
        capabilities: &["main_search"],
        operations: &[],
        credentials_required: true,
    },
    ProviderRegistration {
        id: ProviderId::OpenAiCompatible,
        name: "openai_compatible",
        capabilities: &["main_search"],
        operations: &[],
        credentials_required: true,
    },
    ProviderRegistration {
        id: ProviderId::Tavily,
        name: "tavily",
        capabilities: &["web_search", "web_fetch"],
        operations: &["site_map"],
        credentials_required: true,
    },
    ProviderRegistration {
        id: ProviderId::Firecrawl,
        name: "firecrawl",
        capabilities: &["web_search", "web_fetch"],
        operations: &[],
        credentials_required: true,
    },
    ProviderRegistration {
        id: ProviderId::Jina,
        name: "jina",
        capabilities: &["web_fetch"],
        operations: &[],
        credentials_required: true,
    },
    ProviderRegistration {
        id: ProviderId::Context7,
        name: "context7",
        capabilities: &["docs_search"],
        operations: &[],
        credentials_required: true,
    },
    ProviderRegistration {
        id: ProviderId::Exa,
        name: "exa",
        capabilities: &["docs_search"],
        operations: &[],
        credentials_required: true,
    },
    ProviderRegistration {
        id: ProviderId::Anysearch,
        name: "anysearch",
        capabilities: &["vertical_search"],
        operations: &[],
        credentials_required: true,
    },
];

pub(crate) fn registrations() -> &'static [ProviderRegistration] {
    REGISTRY
}

pub(crate) fn build_xai(
    mut config: XaiRuntimeConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Xai {
    let registration = registration(ProviderId::Xai);
    debug_assert!(registration.credentials_required);
    let credentials = CredentialPool::new(registration.name, std::mem::take(&mut config.keys));
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
    let credentials = CredentialPool::new(registration.name, std::mem::take(&mut config.keys));
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
    match (id, config) {
        (ProviderId::Xai, MainSearchProviderConfig::Xai(config)) => {
            Box::new(build_xai(config, client, retry_policy, deadline))
        }
        (ProviderId::OpenAiCompatible, MainSearchProviderConfig::OpenAiCompatible(config)) => {
            Box::new(build_openai_compatible(
                config,
                client,
                retry_policy,
                deadline,
                breakers,
            ))
        }
        _ => unreachable!("main search entry pairs provider id with its configuration"),
    }
}

pub(crate) fn supports(capability: &str, provider: &str) -> bool {
    REGISTRY.iter().any(|registration| {
        registration.name == provider && registration.capabilities.contains(&capability)
    })
}

pub(crate) fn build_web_fetch(
    id: ProviderId,
    mut config: WebFetchProviderConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Box<dyn WebFetch> {
    let registration = registration(id);
    debug_assert!(registration.credentials_required);
    debug_assert!(registration.capabilities.contains(&"web_fetch"));
    let credentials = CredentialPool::new(registration.name, std::mem::take(&mut config.keys));
    match id {
        ProviderId::Jina => web_fetch::jina(config, client, credentials, retry_policy, deadline),
        ProviderId::Tavily => {
            web_fetch::tavily(config, client, credentials, retry_policy, deadline)
        }
        ProviderId::Firecrawl => {
            web_fetch::firecrawl(config, client, credentials, retry_policy, deadline)
        }
        _ => unreachable!("web_fetch capability only has web fetch constructors"),
    }
}

pub(crate) fn build_web_search(
    id: ProviderId,
    mut config: WebFetchProviderConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Box<dyn WebSearch> {
    let registration = registration(id);
    debug_assert!(registration.credentials_required);
    debug_assert!(registration.capabilities.contains(&"web_search"));
    let credentials = CredentialPool::new(registration.name, std::mem::take(&mut config.keys));
    match id {
        ProviderId::Tavily | ProviderId::Firecrawl => Box::new(SupplementalSearch::new(
            registration.name,
            config,
            client,
            credentials,
            retry_policy,
            deadline,
        )),
        _ => unreachable!("web_search capability only has web search constructors"),
    }
}

pub(crate) fn build_docs_search(
    id: ProviderId,
    config: DocsSearchProviderConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Box<dyn DocsSearch> {
    let registration = registration(id);
    debug_assert!(registration.capabilities.contains(&"docs_search"));
    match (id, config) {
        (ProviderId::Exa, DocsSearchProviderConfig::Exa(config)) => {
            Box::new(build_exa(config, client, retry_policy, deadline))
        }
        (ProviderId::Context7, DocsSearchProviderConfig::Context7(config)) => {
            Box::new(build_context7(config, client, retry_policy, deadline))
        }
        _ => unreachable!("docs_search capability only has docs search constructors"),
    }
}

pub(crate) fn build_vertical_search(
    id: ProviderId,
    config: AnysearchRuntimeConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Box<dyn VerticalSearch> {
    let registration = registration(id);
    debug_assert!(registration.capabilities.contains(&"vertical_search"));
    match id {
        ProviderId::Anysearch => Box::new(build_anysearch(config, client, retry_policy, deadline)),
        _ => unreachable!("vertical_search capability only has vertical search constructors"),
    }
}

pub(crate) fn registration(id: ProviderId) -> &'static ProviderRegistration {
    match id {
        ProviderId::Xai => &REGISTRY[0],
        ProviderId::OpenAiCompatible => &REGISTRY[1],
        ProviderId::Tavily => &REGISTRY[2],
        ProviderId::Firecrawl => &REGISTRY[3],
        ProviderId::Jina => &REGISTRY[4],
        ProviderId::Context7 => &REGISTRY[5],
        ProviderId::Exa => &REGISTRY[6],
        ProviderId::Anysearch => &REGISTRY[7],
    }
}

pub(crate) fn build_exa(
    mut config: ExaRuntimeConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> exa::Exa {
    let registration = registration(ProviderId::Exa);
    debug_assert!(registration.credentials_required);
    let credentials = CredentialPool::new(registration.name, std::mem::take(&mut config.keys));
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
    let credentials = CredentialPool::new(registration.name, std::mem::take(&mut config.keys));
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
    let credentials = CredentialPool::new(registration.name, std::mem::take(&mut config.keys));
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
    let credentials = CredentialPool::new(registration.name, std::mem::take(&mut config.keys));
    Anysearch::new(config, client, credentials, retry_policy, deadline)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde::Deserialize;

    use super::{ProviderId, registration, registrations};

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
        let registry = registrations()
            .iter()
            .flat_map(|registration| {
                registration
                    .capabilities
                    .iter()
                    .map(move |seam| (registration.name, *seam))
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
    fn exa_has_one_complete_registry_description() {
        let exa = registration(ProviderId::Exa);

        assert_eq!(
            (exa.name, exa.capabilities, exa.credentials_required,),
            ("exa", &["docs_search"][..], true)
        );
    }

    #[test]
    fn xai_has_one_complete_registry_description() {
        let xai = registration(ProviderId::Xai);

        assert_eq!(
            (xai.name, xai.capabilities, xai.credentials_required,),
            ("xai", &["main_search"][..], true)
        );
    }

    #[test]
    fn openai_compatible_has_one_complete_registry_description() {
        let openai = registration(ProviderId::OpenAiCompatible);

        assert_eq!(
            (
                openai.name,
                openai.capabilities,
                openai.credentials_required,
            ),
            ("openai_compatible", &["main_search"][..], true)
        );
    }

    #[test]
    fn context7_has_one_complete_registry_description() {
        let context7 = registration(ProviderId::Context7);

        assert_eq!(
            (
                context7.name,
                context7.capabilities,
                context7.credentials_required,
            ),
            ("context7", &["docs_search"][..], true)
        );
    }

    #[test]
    fn anysearch_has_one_complete_registry_description() {
        let anysearch = registration(ProviderId::Anysearch);

        assert_eq!(
            (
                anysearch.name,
                anysearch.capabilities,
                anysearch.credentials_required,
            ),
            ("anysearch", &["vertical_search"][..], true)
        );
    }

    #[test]
    fn every_web_fetch_provider_has_one_complete_registry_description() {
        for (id, name) in [
            (ProviderId::Jina, "jina"),
            (ProviderId::Tavily, "tavily"),
            (ProviderId::Firecrawl, "firecrawl"),
        ] {
            let registration = registration(id);
            assert_eq!(
                (
                    registration.name,
                    registration.capabilities.contains(&"web_fetch"),
                    registration.credentials_required,
                ),
                (name, true, true)
            );
        }
    }

    #[test]
    fn tavily_registers_site_map_without_changing_its_capability_support() {
        let tavily = registration(ProviderId::Tavily);

        assert_eq!(
            (tavily.capabilities, tavily.operations),
            (&["web_search", "web_fetch"][..], &["site_map"][..])
        );
    }
}
