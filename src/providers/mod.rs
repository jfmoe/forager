mod context7;
mod exa;

use reqwest::Client;
use thiserror::Error;

use crate::config::{Context7RuntimeConfig, ExaRuntimeConfig};
use crate::credentials::CredentialPool;
use crate::net::RetryPolicy;
use crate::types::{AttemptErrorKind, Deadline, ProviderAttempt};

pub(crate) use context7::{Context7, Context7DocsRequest, Context7LibraryRequest};
pub(crate) use exa::{Exa, ExaSearchRequest, ExaSimilarRequest, SearchType};

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
    pub(crate) credentials_required: bool,
    constructor: ProviderConstructor,
}

#[derive(Clone, Copy)]
enum ProviderConstructor {
    Context7,
    Exa,
    Pending,
}

const REGISTRY: &[ProviderRegistration] = &[
    ProviderRegistration {
        id: ProviderId::Tavily,
        name: "tavily",
        capabilities: &["web_search", "web_fetch"],
        credentials_required: true,
        constructor: ProviderConstructor::Pending,
    },
    ProviderRegistration {
        id: ProviderId::Firecrawl,
        name: "firecrawl",
        capabilities: &["web_search", "web_fetch"],
        credentials_required: true,
        constructor: ProviderConstructor::Pending,
    },
    ProviderRegistration {
        id: ProviderId::Jina,
        name: "jina",
        capabilities: &["web_fetch"],
        credentials_required: true,
        constructor: ProviderConstructor::Pending,
    },
    ProviderRegistration {
        id: ProviderId::Context7,
        name: "context7",
        capabilities: &["docs_search"],
        credentials_required: true,
        constructor: ProviderConstructor::Context7,
    },
    ProviderRegistration {
        id: ProviderId::Exa,
        name: "exa",
        capabilities: &["docs_search"],
        credentials_required: true,
        constructor: ProviderConstructor::Exa,
    },
    ProviderRegistration {
        id: ProviderId::Anysearch,
        name: "anysearch",
        capabilities: &["vertical_search"],
        credentials_required: true,
        constructor: ProviderConstructor::Pending,
    },
];

pub(crate) fn supports(capability: &str, provider: &str) -> bool {
    REGISTRY.iter().any(|registration| {
        registration.name == provider && registration.capabilities.contains(&capability)
    })
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
}
