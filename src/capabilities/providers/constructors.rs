use reqwest::Client;

use super::{Exa, Openai, TavilyMap};
use crate::catalog::{ProviderId, registration};
use crate::config::{ExaRuntimeConfig, OpenaiRuntimeConfig, WebFetchProviderConfig};
use crate::credentials::CredentialPool;
use crate::net::RetryPolicy;
use crate::rate_limit::RateLimiter;
use crate::types::Deadline;

pub(crate) fn build_openai(
    mut config: OpenaiRuntimeConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Openai {
    let credentials = credentials(ProviderId::Openai, &mut config.keys);
    Openai::new(config, client, credentials, retry_policy, deadline)
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

/// Returns a limiter for a provider whose registration declares an access policy.
///
/// Each call builds a new in-process concurrency limit; sends still share the cross-process
/// state, so every limiter for one route keeps the same spacing.
pub(crate) fn route_limiter(id: ProviderId) -> Option<RateLimiter> {
    registration(id)
        .access_policy
        .map(|policy| RateLimiter::new(id.name(), policy))
}

pub(super) fn credentials(id: ProviderId, keys: &mut Vec<crate::redact::Secret>) -> CredentialPool {
    CredentialPool::new(id.name(), std::mem::take(keys))
}
