mod anysearch;
mod context7;
mod exa;
mod execution;
mod tavily_map;
mod web_fetch;

use reqwest::Client;
use thiserror::Error;

use crate::config::{
    AnysearchRuntimeConfig, Context7RuntimeConfig, ExaRuntimeConfig, WebFetchProviderConfig,
};
use crate::credentials::CredentialPool;
use crate::net::RetryPolicy;
use crate::types::{AttemptErrorKind, Deadline, ProviderAttempt};

pub(crate) use anysearch::{Anysearch, AnysearchDomainsRequest, AnysearchSearchRequest};
pub(crate) use context7::{Context7, Context7DocsRequest, Context7LibraryRequest};
pub(crate) use exa::{Exa, ExaSearchRequest, ExaSimilarRequest, SearchType};
pub(crate) use tavily_map::{MapRequest, TavilyMap};
pub(crate) use web_fetch::{FetchRequest, WebFetch};

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
    Exa,
    Tavily,
    Firecrawl,
    Jina,
    Context7,
    Anysearch,
}

#[derive(Clone, Copy)]
pub(crate) struct ProviderRegistration {
    pub(crate) id: ProviderId,
    pub(crate) name: &'static str,
    pub(crate) capabilities: &'static [&'static str],
    pub(crate) operations: &'static [&'static str],
    pub(crate) credentials_required: bool,
    constructor: ProviderConstructor,
}

#[derive(Clone, Copy)]
enum ProviderConstructor {
    Anysearch,
    Context7,
    Exa,
    Jina,
    Tavily,
    Firecrawl,
}

const REGISTRY: &[ProviderRegistration] = &[
    ProviderRegistration {
        id: ProviderId::Tavily,
        name: "tavily",
        capabilities: &["web_search", "web_fetch"],
        operations: &["site_map"],
        credentials_required: true,
        constructor: ProviderConstructor::Tavily,
    },
    ProviderRegistration {
        id: ProviderId::Firecrawl,
        name: "firecrawl",
        capabilities: &["web_search", "web_fetch"],
        operations: &[],
        credentials_required: true,
        constructor: ProviderConstructor::Firecrawl,
    },
    ProviderRegistration {
        id: ProviderId::Jina,
        name: "jina",
        capabilities: &["web_fetch"],
        operations: &[],
        credentials_required: true,
        constructor: ProviderConstructor::Jina,
    },
    ProviderRegistration {
        id: ProviderId::Context7,
        name: "context7",
        capabilities: &["docs_search"],
        operations: &[],
        credentials_required: true,
        constructor: ProviderConstructor::Context7,
    },
    ProviderRegistration {
        id: ProviderId::Exa,
        name: "exa",
        capabilities: &["docs_search"],
        operations: &[],
        credentials_required: true,
        constructor: ProviderConstructor::Exa,
    },
    ProviderRegistration {
        id: ProviderId::Anysearch,
        name: "anysearch",
        capabilities: &["vertical_search"],
        operations: &[],
        credentials_required: true,
        constructor: ProviderConstructor::Anysearch,
    },
];

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
    use super::{ProviderConstructor, ProviderId, registration};

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
