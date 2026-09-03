use reqwest::Client;

use super::{Anysearch, Context7, Exa, ModelBreakers, OpenAiCompatible, TavilyMap, Xai};
use crate::catalog::{ProviderId, registration};
use crate::config::{
    AnysearchRuntimeConfig, Context7RuntimeConfig, ExaRuntimeConfig, OpenAiCompatibleRuntimeConfig,
    WebFetchProviderConfig, XaiRuntimeConfig,
};
use crate::credentials::CredentialPool;
use crate::net::RetryPolicy;
use crate::types::Deadline;

pub(crate) fn build_xai(
    mut config: XaiRuntimeConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Xai {
    let credentials = credentials(ProviderId::Xai, &mut config.keys);
    Xai::new(config, client, credentials, retry_policy, deadline)
}

pub(crate) fn build_openai_compatible(
    mut config: OpenAiCompatibleRuntimeConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    breakers: std::sync::Arc<ModelBreakers>,
) -> OpenAiCompatible {
    let credentials = credentials(ProviderId::OpenAiCompatible, &mut config.keys);
    OpenAiCompatible::new(
        config,
        client,
        credentials,
        retry_policy,
        deadline,
        breakers,
    )
}

pub(crate) fn build_exa(
    mut config: ExaRuntimeConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Exa {
    let credentials = credentials(ProviderId::Exa, &mut config.keys);
    Exa::new(config, client, credentials, retry_policy, deadline)
}

pub(crate) fn build_tavily_map(
    mut config: WebFetchProviderConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> TavilyMap {
    let registration = registration(ProviderId::Tavily);
    debug_assert!(registration.operations.contains(&"site_map"));
    let credentials = credentials(ProviderId::Tavily, &mut config.keys);
    TavilyMap::new(config, client, credentials, retry_policy, deadline)
}

pub(crate) fn build_context7(
    mut config: Context7RuntimeConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Context7 {
    let credentials = credentials(ProviderId::Context7, &mut config.keys);
    Context7::new(config, client, credentials, retry_policy, deadline)
}

pub(crate) fn build_anysearch(
    mut config: AnysearchRuntimeConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Anysearch {
    let credentials = credentials(ProviderId::Anysearch, &mut config.keys);
    Anysearch::new(config, client, credentials, retry_policy, deadline)
}

pub(super) fn credentials(id: ProviderId, keys: &mut Vec<crate::redact::Secret>) -> CredentialPool {
    let registration = registration(id);
    assert!(
        registration.credentials_required,
        "provider uses credentials"
    );
    CredentialPool::new(id.name(), std::mem::take(keys))
}
