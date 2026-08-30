use std::sync::Arc;

use reqwest::Client;

use super::{
    DocsSearch, MainSearch, ModelBreakers, ProviderId, SupplementalSearch, VerticalSearch,
    WebFetch, WebSearch, web_fetch,
};
use crate::config::{
    AnysearchRuntimeConfig, Context7RuntimeConfig, DocsSearchProviderConfig, ExaRuntimeConfig,
    MainSearchProviderConfig, OpenAiCompatibleRuntimeConfig, WebFetchProviderConfig,
    XaiRuntimeConfig,
};
use crate::credentials::CredentialPool;
use crate::net::RetryPolicy;
use crate::types::Deadline;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CapabilityCatalog {
    pub(crate) seam: &'static str,
    pub(crate) providers: &'static [ProviderId],
}

pub(crate) const MAIN_SEARCH: CapabilityCatalog = CapabilityCatalog {
    seam: "main_search",
    providers: &[ProviderId::Xai, ProviderId::OpenAiCompatible],
};
pub(crate) const WEB_SEARCH: CapabilityCatalog = CapabilityCatalog {
    seam: "web_search",
    providers: &[ProviderId::Tavily, ProviderId::Firecrawl],
};
pub(crate) const WEB_FETCH: CapabilityCatalog = CapabilityCatalog {
    seam: "web_fetch",
    providers: &[ProviderId::Tavily, ProviderId::Firecrawl, ProviderId::Jina],
};
pub(crate) const DOCS_SEARCH: CapabilityCatalog = CapabilityCatalog {
    seam: "docs_search",
    providers: &[ProviderId::Context7, ProviderId::Exa],
};
pub(crate) const VERTICAL_SEARCH: CapabilityCatalog = CapabilityCatalog {
    seam: "vertical_search",
    providers: &[ProviderId::Anysearch],
};

pub(crate) const CATALOGS: &[CapabilityCatalog] = &[
    MAIN_SEARCH,
    WEB_SEARCH,
    WEB_FETCH,
    DOCS_SEARCH,
    VERTICAL_SEARCH,
];

impl CapabilityCatalog {
    pub(crate) fn contains(self, id: ProviderId) -> bool {
        self.providers.contains(&id)
    }
}

pub(crate) fn by_seam(seam: &str) -> Option<CapabilityCatalog> {
    CATALOGS
        .iter()
        .copied()
        .find(|catalog| catalog.seam == seam)
}

pub(crate) fn main_config(
    id: ProviderId,
    xai: &XaiRuntimeConfig,
    openai: &OpenAiCompatibleRuntimeConfig,
) -> Option<MainSearchProviderConfig> {
    match id {
        ProviderId::Xai => Some(MainSearchProviderConfig::Xai(xai.clone())),
        ProviderId::OpenAiCompatible => {
            Some(MainSearchProviderConfig::OpenAiCompatible(openai.clone()))
        }
        _ => None,
    }
}

pub(crate) fn docs_config(
    id: ProviderId,
    exa: &ExaRuntimeConfig,
    context7: &Context7RuntimeConfig,
) -> Option<DocsSearchProviderConfig> {
    match id {
        ProviderId::Exa => Some(DocsSearchProviderConfig::Exa(exa.clone())),
        ProviderId::Context7 => Some(DocsSearchProviderConfig::Context7(context7.clone())),
        _ => None,
    }
}

pub(crate) fn web_config(
    catalog: CapabilityCatalog,
    id: ProviderId,
    tavily: &WebFetchProviderConfig,
    firecrawl: &WebFetchProviderConfig,
    jina: &WebFetchProviderConfig,
) -> Option<WebFetchProviderConfig> {
    if !catalog.contains(id) {
        return None;
    }
    match id {
        ProviderId::Tavily => Some(tavily.clone()),
        ProviderId::Firecrawl => Some(firecrawl.clone()),
        ProviderId::Jina => Some(jina.clone()),
        _ => None,
    }
}

pub(crate) fn vertical_config(
    id: ProviderId,
    anysearch: &AnysearchRuntimeConfig,
) -> Option<AnysearchRuntimeConfig> {
    VERTICAL_SEARCH.contains(id).then(|| anysearch.clone())
}

pub(crate) fn build_main(
    id: ProviderId,
    config: MainSearchProviderConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    breakers: Arc<ModelBreakers>,
) -> Box<dyn MainSearch> {
    match (id, config) {
        (ProviderId::Xai, MainSearchProviderConfig::Xai(config)) => {
            Box::new(super::build_xai(config, client, retry_policy, deadline))
        }
        (ProviderId::OpenAiCompatible, MainSearchProviderConfig::OpenAiCompatible(config)) => {
            Box::new(super::build_openai_compatible(
                config,
                client,
                retry_policy,
                deadline,
                breakers,
            ))
        }
        _ => unreachable!("validated main-search catalog entry has matching config"),
    }
}

pub(crate) fn build_web_fetch(
    id: ProviderId,
    mut config: WebFetchProviderConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Box<dyn WebFetch> {
    assert!(WEB_FETCH.contains(id), "validated web-fetch provider");
    let credentials = credentials(id, &mut config.keys);
    web_fetch::new(id, config, client, credentials, retry_policy, deadline)
}

pub(crate) fn build_web_search(
    id: ProviderId,
    mut config: WebFetchProviderConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Box<dyn WebSearch> {
    assert!(WEB_SEARCH.contains(id), "validated web-search provider");
    let credentials = credentials(id, &mut config.keys);
    Box::new(SupplementalSearch::new(
        id,
        config,
        client,
        credentials,
        retry_policy,
        deadline,
    ))
}

pub(crate) fn build_docs(
    id: ProviderId,
    config: DocsSearchProviderConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Box<dyn DocsSearch> {
    match (id, config) {
        (ProviderId::Exa, DocsSearchProviderConfig::Exa(config)) => {
            Box::new(super::build_exa(config, client, retry_policy, deadline))
        }
        (ProviderId::Context7, DocsSearchProviderConfig::Context7(config)) => Box::new(
            super::build_context7(config, client, retry_policy, deadline),
        ),
        _ => unreachable!("validated docs-search catalog entry has matching config"),
    }
}

pub(crate) fn build_vertical(
    id: ProviderId,
    config: AnysearchRuntimeConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Box<dyn VerticalSearch> {
    assert!(
        VERTICAL_SEARCH.contains(id),
        "validated vertical-search provider"
    );
    Box::new(super::build_anysearch(
        config,
        client,
        retry_policy,
        deadline,
    ))
}

fn credentials(id: ProviderId, keys: &mut Vec<crate::redact::Secret>) -> CredentialPool {
    let registration = super::registration(id);
    assert!(
        registration.credentials_required,
        "provider uses credentials"
    );
    CredentialPool::new(id.name(), std::mem::take(keys))
}
