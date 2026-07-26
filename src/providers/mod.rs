use std::future::Future;
use std::pin::Pin;

mod anysearch;
mod context7;
mod exa;
pub(crate) mod execution;
mod openai_compatible;
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
use crate::net::RetryPolicy;
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

pub(crate) trait MainSearch: Send + Sync {
    fn search(
        &self,
        request: SearchRequest,
    ) -> Pin<Box<dyn Future<Output = Result<crate::types::SearchOutcome, ProviderError>> + Send + '_>>;
}

pub(crate) trait WebSearch: Send + Sync {
    fn search(
        &self,
        query: String,
        limit: u16,
    ) -> Pin<Box<dyn Future<Output = Result<SupplementalSearchOutcome, ProviderError>> + Send + '_>>;
}

impl WebSearch for SupplementalSearch {
    fn search(
        &self,
        query: String,
        limit: u16,
    ) -> Pin<Box<dyn Future<Output = Result<SupplementalSearchOutcome, ProviderError>> + Send + '_>>
    {
        Box::pin(async move { SupplementalSearch::search(self, &query, limit).await })
    }
}

pub(crate) trait DocsSearch: Send + Sync {
    fn search(
        &self,
        query: String,
        limit: u16,
    ) -> Pin<Box<dyn Future<Output = Result<SupplementalSearchOutcome, ProviderError>> + Send + '_>>;
}

pub(crate) trait VerticalSearch: Send + Sync {
    fn search(
        &self,
        query: String,
        limit: u16,
    ) -> Pin<Box<dyn Future<Output = Result<VerticalSearchOutcome, ProviderError>> + Send + '_>>;
}

impl VerticalSearch for Anysearch {
    fn search(
        &self,
        query: String,
        limit: u16,
    ) -> Pin<Box<dyn Future<Output = Result<VerticalSearchOutcome, ProviderError>> + Send + '_>>
    {
        Box::pin(async move {
            let AnysearchOutcome::Search(outcome) = self
                .search(AnysearchSearchRequest {
                    query,
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
                    url: crate::config::redact_url(&result.url),
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
    fn search(
        &self,
        query: String,
        limit: u16,
    ) -> Pin<Box<dyn Future<Output = Result<SupplementalSearchOutcome, ProviderError>> + Send + '_>>
    {
        Box::pin(async move {
            let ExaOutcome {
                results,
                attempts,
                diagnostic,
                ..
            } = self
                .search(ExaSearchRequest {
                    query,
                    num_results: limit,
                    search_type: SearchType::Auto,
                    include_text: false,
                    include_highlights: false,
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
    fn search(
        &self,
        query: String,
        limit: u16,
    ) -> Pin<Box<dyn Future<Output = Result<SupplementalSearchOutcome, ProviderError>> + Send + '_>>
    {
        Box::pin(async move {
            let Context7Outcome::Library(library) = self
                .library(Context7LibraryRequest {
                    name: query.clone(),
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
                    query,
                    verbose: true,
                })
                .await;
            let Context7Outcome::Docs(mut docs) = (match docs_result {
                Ok(outcome) => outcome,
                Err(mut error) => {
                    error.attempts.splice(0..0, library.attempts);
                    error.diagnostic = merge_diagnostic(library.diagnostic, error.diagnostic);
                    return Err(error);
                }
            }) else {
                unreachable!("docs request returns docs outcome");
            };
            let mut attempts = library.attempts;
            attempts.append(&mut docs.attempts);
            let mut sources = docs
                .results
                .iter()
                .filter_map(context7_source)
                .take(usize::from(limit))
                .collect::<Vec<_>>();
            if let Some(source) = sources.first_mut()
                && source.text.is_none()
                && !docs.content.trim().is_empty()
            {
                source.text = Some(docs.content);
            }
            Ok(SupplementalSearchOutcome {
                sources,
                attempts,
                diagnostic: merge_diagnostic(library.diagnostic, docs.diagnostic),
            })
        })
    }
}

fn merge_diagnostic(first: Option<String>, second: Option<String>) -> Option<String> {
    match (first, second) {
        (Some(first), Some(second)) => Some(format!("{first}\n{second}")),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn context7_source(value: &serde_json::Value) -> Option<Source> {
    let fields = value.as_object()?;
    let url = fields.get("url")?.as_str()?;
    Some(Source {
        title: fields
            .get("title")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Context7 documentation")
            .to_owned(),
        url: crate::config::redact_url(url),
        published_date: None,
        author: None,
        text: fields
            .get("text")
            .or_else(|| fields.get("content"))
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        highlights: Vec::new(),
    })
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
        registration_by_name(value).map(|registration| registration.id)
    }

    pub(crate) fn name(self) -> &'static str {
        registration(self).name
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ProviderRegistration {
    pub(crate) id: ProviderId,
    pub(crate) name: &'static str,
    pub(crate) capabilities: &'static [&'static str],
    pub(crate) operations: &'static [&'static str],
    pub(crate) credentials_required: bool,
    pub(crate) doctor_probe: DoctorProbe,
    constructor: ProviderConstructor,
}

#[derive(Clone, Copy)]
pub(crate) enum DoctorProbe {
    XaiResponses,
    OpenAiCompatibleShapes,
    ExaSearch,
    TavilySearch,
    FirecrawlSearch,
    JinaFetch,
    Context7Library,
    AnysearchDomains,
}

#[derive(Clone, Copy)]
enum ProviderConstructor {
    Xai,
    OpenAiCompatible,
    Anysearch,
    Context7,
    Exa,
    Jina,
    Tavily,
    Firecrawl,
}

const REGISTRY: &[ProviderRegistration] = &[
    ProviderRegistration {
        id: ProviderId::Xai,
        name: "xai",
        capabilities: &["main_search"],
        operations: &[],
        credentials_required: true,
        doctor_probe: DoctorProbe::XaiResponses,
        constructor: ProviderConstructor::Xai,
    },
    ProviderRegistration {
        id: ProviderId::OpenAiCompatible,
        name: "openai_compatible",
        capabilities: &["main_search"],
        operations: &[],
        credentials_required: true,
        doctor_probe: DoctorProbe::OpenAiCompatibleShapes,
        constructor: ProviderConstructor::OpenAiCompatible,
    },
    ProviderRegistration {
        id: ProviderId::Tavily,
        name: "tavily",
        capabilities: &["web_search", "web_fetch"],
        operations: &["site_map"],
        credentials_required: true,
        doctor_probe: DoctorProbe::TavilySearch,
        constructor: ProviderConstructor::Tavily,
    },
    ProviderRegistration {
        id: ProviderId::Firecrawl,
        name: "firecrawl",
        capabilities: &["web_search", "web_fetch"],
        operations: &[],
        credentials_required: true,
        doctor_probe: DoctorProbe::FirecrawlSearch,
        constructor: ProviderConstructor::Firecrawl,
    },
    ProviderRegistration {
        id: ProviderId::Jina,
        name: "jina",
        capabilities: &["web_fetch"],
        operations: &[],
        credentials_required: true,
        doctor_probe: DoctorProbe::JinaFetch,
        constructor: ProviderConstructor::Jina,
    },
    ProviderRegistration {
        id: ProviderId::Context7,
        name: "context7",
        capabilities: &["docs_search"],
        operations: &[],
        credentials_required: true,
        doctor_probe: DoctorProbe::Context7Library,
        constructor: ProviderConstructor::Context7,
    },
    ProviderRegistration {
        id: ProviderId::Exa,
        name: "exa",
        capabilities: &["docs_search"],
        operations: &[],
        credentials_required: true,
        doctor_probe: DoctorProbe::ExaSearch,
        constructor: ProviderConstructor::Exa,
    },
    ProviderRegistration {
        id: ProviderId::Anysearch,
        name: "anysearch",
        capabilities: &["vertical_search"],
        operations: &[],
        credentials_required: true,
        doctor_probe: DoctorProbe::AnysearchDomains,
        constructor: ProviderConstructor::Anysearch,
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
    debug_assert!(matches!(registration.constructor, ProviderConstructor::Xai));
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
    debug_assert!(matches!(
        registration.constructor,
        ProviderConstructor::OpenAiCompatible
    ));
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
    name: &str,
    config: MainSearchProviderConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    breakers: std::sync::Arc<ModelBreakers>,
) -> Box<dyn MainSearch> {
    match config {
        MainSearchProviderConfig::Xai(config) => {
            debug_assert_eq!(name, registration(ProviderId::Xai).name);
            Box::new(build_xai(config, client, retry_policy, deadline))
        }
        MainSearchProviderConfig::OpenAiCompatible(config) => {
            debug_assert_eq!(name, registration(ProviderId::OpenAiCompatible).name);
            Box::new(build_openai_compatible(
                config,
                client,
                retry_policy,
                deadline,
                breakers,
            ))
        }
    }
}

pub(crate) fn supports(capability: &str, provider: &str) -> bool {
    REGISTRY.iter().any(|registration| {
        registration.name == provider && registration.capabilities.contains(&capability)
    })
}

pub(crate) fn registration_by_name(name: &str) -> Option<&'static ProviderRegistration> {
    REGISTRY
        .iter()
        .find(|registration| registration.name == name)
}

pub(crate) fn build_web_fetch(
    provider: &str,
    mut config: WebFetchProviderConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Box<dyn WebFetch> {
    let registration = registration_by_name(provider)
        .expect("validated web_fetch order contains registered providers");
    debug_assert!(registration.credentials_required);
    debug_assert!(registration.capabilities.contains(&"web_fetch"));
    let credentials = CredentialPool::new(registration.name, std::mem::take(&mut config.keys));
    match registration.constructor {
        ProviderConstructor::Jina => {
            web_fetch::jina(config, client, credentials, retry_policy, deadline)
        }
        ProviderConstructor::Tavily => {
            web_fetch::tavily(config, client, credentials, retry_policy, deadline)
        }
        ProviderConstructor::Firecrawl => {
            web_fetch::firecrawl(config, client, credentials, retry_policy, deadline)
        }
        _ => unreachable!("web_fetch capability only has web fetch constructors"),
    }
}

pub(crate) fn build_web_search(
    provider: &str,
    mut config: WebFetchProviderConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Box<dyn WebSearch> {
    let registration = registration_by_name(provider)
        .expect("validated web_search order contains registered providers");
    debug_assert!(registration.credentials_required);
    debug_assert!(registration.capabilities.contains(&"web_search"));
    let credentials = CredentialPool::new(registration.name, std::mem::take(&mut config.keys));
    match registration.constructor {
        ProviderConstructor::Tavily | ProviderConstructor::Firecrawl => {
            Box::new(SupplementalSearch::new(
                registration.name,
                config,
                client,
                credentials,
                retry_policy,
                deadline,
            ))
        }
        _ => unreachable!("web_search capability only has web search constructors"),
    }
}

pub(crate) fn build_docs_search(
    provider: &str,
    config: DocsSearchProviderConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Box<dyn DocsSearch> {
    let registration = registration_by_name(provider)
        .expect("validated docs_search order contains registered providers");
    debug_assert!(registration.capabilities.contains(&"docs_search"));
    match (registration.constructor, config) {
        (ProviderConstructor::Exa, DocsSearchProviderConfig::Exa(config)) => {
            Box::new(build_exa(config, client, retry_policy, deadline))
        }
        (ProviderConstructor::Context7, DocsSearchProviderConfig::Context7(config)) => {
            Box::new(build_context7(config, client, retry_policy, deadline))
        }
        _ => unreachable!("docs_search capability only has docs search constructors"),
    }
}

pub(crate) fn build_vertical_search(
    provider: &str,
    config: AnysearchRuntimeConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Box<dyn VerticalSearch> {
    let registration = registration_by_name(provider)
        .expect("validated vertical_search order contains registered providers");
    debug_assert!(registration.capabilities.contains(&"vertical_search"));
    match registration.constructor {
        ProviderConstructor::Anysearch => {
            Box::new(build_anysearch(config, client, retry_policy, deadline))
        }
        _ => unreachable!("vertical_search capability only has vertical search constructors"),
    }
}

pub(crate) fn registration(id: ProviderId) -> &'static ProviderRegistration {
    REGISTRY
        .iter()
        .find(|registration| registration.id == id)
        .expect("every ProviderId has one registry entry")
}

pub(crate) fn build_exa(
    mut config: ExaRuntimeConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> exa::Exa {
    let registration = registration(ProviderId::Exa);
    debug_assert!(registration.credentials_required);
    debug_assert!(matches!(registration.constructor, ProviderConstructor::Exa));
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
    debug_assert!(matches!(
        registration.constructor,
        ProviderConstructor::Context7
    ));
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
    debug_assert!(matches!(
        registration.constructor,
        ProviderConstructor::Anysearch
    ));
    let credentials = CredentialPool::new(registration.name, std::mem::take(&mut config.keys));
    Anysearch::new(config, client, credentials, retry_policy, deadline)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde::Deserialize;

    use super::{ProviderConstructor, ProviderId, registration, registrations};

    #[derive(Deserialize)]
    struct TransportFixture {
        provider: String,
        seam: String,
        test: String,
    }

    #[test]
    fn provider_fixture_projection_matches_transport_manifest() {
        let fixtures: Vec<TransportFixture> = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/transport-fixtures.json"
        )))
        .expect("transport fixture manifest");
        let registry = registrations()
            .iter()
            .flat_map(|registration| {
                registration
                    .capabilities
                    .iter()
                    .map(move |seam| (registration.name, *seam))
            })
            .collect::<BTreeSet<_>>();
        let fixture_projection = fixtures
            .iter()
            .map(|fixture| (fixture.provider.as_str(), fixture.seam.as_str()))
            .collect::<BTreeSet<_>>();

        assert_eq!(fixture_projection, registry);
        for fixture in fixtures {
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
            (
                exa.name,
                exa.capabilities,
                exa.credentials_required,
                matches!(exa.constructor, ProviderConstructor::Exa),
            ),
            ("exa", &["docs_search"][..], true, true)
        );
    }

    #[test]
    fn xai_has_one_complete_registry_description() {
        let xai = registration(ProviderId::Xai);

        assert_eq!(
            (
                xai.name,
                xai.capabilities,
                xai.credentials_required,
                matches!(xai.constructor, ProviderConstructor::Xai),
            ),
            ("xai", &["main_search"][..], true, true)
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
                matches!(openai.constructor, ProviderConstructor::OpenAiCompatible),
            ),
            ("openai_compatible", &["main_search"][..], true, true)
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
                matches!(context7.constructor, ProviderConstructor::Context7),
            ),
            ("context7", &["docs_search"][..], true, true)
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
                matches!(anysearch.constructor, ProviderConstructor::Anysearch),
            ),
            ("anysearch", &["vertical_search"][..], true, true)
        );
    }

    #[test]
    fn every_web_fetch_provider_has_one_complete_registry_description() {
        for (id, name, constructor) in [
            (ProviderId::Jina, "jina", ProviderConstructor::Jina),
            (ProviderId::Tavily, "tavily", ProviderConstructor::Tavily),
            (
                ProviderId::Firecrawl,
                "firecrawl",
                ProviderConstructor::Firecrawl,
            ),
        ] {
            let registration = registration(id);
            assert_eq!(
                (
                    registration.name,
                    registration.capabilities.contains(&"web_fetch"),
                    registration.credentials_required,
                    std::mem::discriminant(&registration.constructor)
                        == std::mem::discriminant(&constructor),
                ),
                (name, true, true, true)
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
