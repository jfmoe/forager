use std::sync::Arc;

use reqwest::Client;

use super::constructors::credentials;
use super::{
    DocsSearch, MainSearch, ModelBreakers, ProviderId, SupplementalSearch, VerticalSearch,
    WebFetch, WebSearch, web_fetch,
};
use crate::catalog::{VERTICAL_SEARCH, WEB_FETCH, WEB_SEARCH};
use crate::config::{
    AnysearchRuntimeConfig, DocsSearchProviderConfig, MainSearchProviderConfig,
    WebFetchProviderConfig,
};
use crate::net::RetryPolicy;
use crate::types::Deadline;

pub(crate) fn build_main_search(
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

pub(crate) fn build_docs_search(
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

pub(crate) fn build_vertical_search(
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
