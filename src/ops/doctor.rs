use std::sync::Arc;
use std::time::Duration;

use futures_util::future::join_all;
use serde::Serialize;
use serde_json::Value;

use crate::catalog::{
    self, DoctorProbe, ProbeShape, ProviderId, ProviderRegistration, ProviderTransport,
};
use crate::config::{
    self, MainSearchProviderConfig, MainSearchRuntimeConfig, PlatformRouteConfig, RuntimeConfig,
};
use crate::net::{self, RetryPolicy};
use crate::providers::{
    self, AnysearchDomainsRequest, FetchRequest, FetchSource, KeyAccount, MainSearchRequest,
    ModelBreakers,
};
use crate::rate_limit::RateLimiter;
use crate::types::{
    AttemptDisposition, AttemptErrorKind, Deadline, Platform, PlatformSearchOptions,
    PlatformSearchOutcome, PlatformSearchRequest, ProviderError, SearchOutcome,
};

#[derive(Debug, Serialize)]
pub(crate) struct ShallowDoctorReport {
    mode: &'static str,
    ok: bool,
    providers: Vec<ProviderStatus>,
    permission_warnings: Vec<String>,
    config_warnings: Vec<String>,
    config: Value,
}

#[derive(Debug, Serialize)]
struct ProviderStatus {
    provider: &'static str,
    configured: bool,
    key_count: usize,
    source: String,
    reachable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DeepDoctorReport {
    mode: &'static str,
    ok: bool,
    provider: &'static str,
    configured: bool,
    key_count: usize,
    source: String,
    deadline_seconds: u64,
    checks: Vec<ProbeCheck>,
    /// One entry per configured key, only for a provider whose probe is an account probe.
    #[serde(skip_serializing_if = "Option::is_none")]
    keys: Option<Vec<KeyStatus>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Debug, Serialize)]
struct ProbeCheck {
    name: &'static str,
    transport: &'static str,
    ok: bool,
}

/// The account report of one key, named by its position in the pool; a quota the service did
/// not report is `null`.
#[derive(Debug, Serialize)]
struct KeyStatus {
    key_index: usize,
    ok: bool,
    searches_left: Option<i64>,
    plan_searches_left: Option<i64>,
    this_month_usage: Option<i64>,
    this_hour_searches: Option<i64>,
    hourly_limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_kind: Option<&'static str>,
}

/// What a deep probe found; `failure` is the first failure, which fails the probe.
struct ProbeOutcome {
    checks: Vec<ProbeCheck>,
    keys: Option<Vec<KeyStatus>>,
    failure: Option<ProbeFailure>,
}

struct ProbeFailure {
    kind: AttemptErrorKind,
    message: String,
}

impl From<ProviderError> for ProbeFailure {
    fn from(error: ProviderError) -> Self {
        Self {
            kind: error.kind,
            message: error.message,
        }
    }
}

pub(crate) fn shallow(
    timeout_seconds: u64,
) -> Result<(ShallowDoctorReport, u8), config::ConfigError> {
    let effective = serde_json::to_value(config::effective_view()?)
        .map_err(|error| config::ConfigError::Message(error.to_string()))?;
    let runtime_config = config::runtime_config()?;
    let deadline = Deadline::new(Duration::from_secs(timeout_seconds));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| config::ConfigError::Message(error.to_string()))?;
    let client = runtime
        .block_on(async { net::build_client(runtime_config.ssl_verify) })
        .map_err(|error| config::ConfigError::Message(error.to_string()))?;
    let checks =
        runtime.block_on(async {
            join_all(catalog::registrations().iter().map(|registration| {
                shallow_check(registration, &runtime_config, &client, deadline)
            }))
            .await
        });
    let providers = catalog::registrations()
        .iter()
        .zip(checks)
        .map(|(registration, check)| status(registration.id, &effective, check, &runtime_config))
        .collect::<Vec<_>>();
    let ok = providers
        .iter()
        .filter(|provider| provider.configured)
        .all(|provider| provider.reachable);
    let exit_code = if ok { 0 } else { 4 };
    Ok((
        ShallowDoctorReport {
            mode: "shallow",
            ok,
            providers,
            permission_warnings: permission_warnings()?,
            config_warnings: main_search_fallback_warnings(&runtime_config.main_search),
            config: effective,
        },
        exit_code,
    ))
}

pub(crate) fn deep(
    provider: ProviderId,
    timeout_seconds: u64,
) -> Result<(DeepDoctorReport, u8), config::ConfigError> {
    let effective = serde_json::to_value(config::effective_view()?)
        .map_err(|error| config::ConfigError::Message(error.to_string()))?;
    let runtime_config = config::runtime_config()?;
    let unconfigured = deep_unconfigured_reason(provider, &runtime_config);
    let provider_status = status(
        provider,
        &effective,
        ShallowCheck {
            configured: unconfigured.is_none(),
            reachable: endpoint_is_valid(provider_endpoint(provider, &runtime_config)),
            message: None,
        },
        &runtime_config,
    );
    if let Some(reason) = unconfigured {
        return Ok((
            DeepDoctorReport {
                mode: "deep",
                ok: false,
                provider: provider.name(),
                configured: false,
                key_count: 0,
                source: provider_status.source,
                deadline_seconds: timeout_seconds,
                checks: Vec::new(),
                keys: None,
                error_kind: Some("config"),
                message: Some(reason),
            },
            3,
        ));
    }
    let retry_policy = RetryPolicy::new(
        runtime_config.retry.max_attempts,
        runtime_config.retry.multiplier,
        Duration::from_secs(runtime_config.retry.max_wait_seconds),
    );
    let deadline = Deadline::new(Duration::from_secs(timeout_seconds));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| config::ConfigError::Message(error.to_string()))?;
    let client = runtime
        .block_on(async { net::build_client(runtime_config.ssl_verify) })
        .map_err(|error| config::ConfigError::Message(error.to_string()))?;
    let outcome = runtime.block_on(run_probe(
        provider,
        runtime_config,
        client,
        retry_policy,
        deadline,
    ));
    let ok = outcome.failure.is_none();
    let (error_kind, message) = outcome
        .failure
        .map(|failure| (failure.kind.as_str(), failure.message))
        .unzip();
    Ok((
        DeepDoctorReport {
            mode: "deep",
            ok,
            provider: provider.name(),
            configured: true,
            key_count: provider_status.key_count,
            source: provider_status.source,
            deadline_seconds: timeout_seconds,
            checks: outcome.checks,
            keys: outcome.keys,
            error_kind,
            message,
        },
        if ok { 0 } else { 4 },
    ))
}

async fn run_probe(
    provider: ProviderId,
    config: RuntimeConfig,
    client: reqwest::Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> ProbeOutcome {
    match catalog::registration(provider).probe {
        DoctorProbe::MainSearch(shapes) => {
            probe_main_search(provider, shapes, config, client, retry_policy, deadline).await
        }
        DoctorProbe::WebSearch { name, transport } => {
            let provider_config = config::web_provider_config(
                catalog::WEB_SEARCH,
                provider,
                &config.tavily,
                &config.firecrawl,
                &config.jina,
            )
            .expect("probe registration belongs to the web-search catalog");
            let adapter = providers::build_web_search(
                provider,
                provider_config,
                client,
                retry_policy,
                deadline,
            );
            one_check(adapter.search("forager doctor", 1).await, name, transport)
        }
        DoctorProbe::WebFetch { name, transport } => one_check(
            probe_web_fetch(provider, &config, client, retry_policy, deadline).await,
            name,
            transport,
        ),
        DoctorProbe::DocsSearch { name, transport } => {
            let provider_config =
                config::docs_provider_config(provider, &config.exa, &config.context7)
                    .expect("probe registration belongs to the docs-search catalog");
            let adapter = providers::build_docs_search(
                provider,
                provider_config,
                client,
                retry_policy,
                deadline,
            );
            one_check(adapter.search("forager doctor", 1).await, name, transport)
        }
        DoctorProbe::PlatformSearch {
            platform,
            name,
            transport,
        } => {
            let route_config = config::platform_route_config(provider, &config.platform_routes)
                .expect("probe registration belongs to a platform catalog");
            let result =
                probe_platform_search(platform, route_config, client, retry_policy, deadline).await;
            one_check(result, name, transport)
        }
        DoctorProbe::ServiceAccount { name, transport } => {
            let route_config = config::platform_route_config(provider, &config.platform_routes)
                .expect("an account probe belongs to a platform route");
            let accounts =
                providers::route_accounts(route_config, client, retry_policy, deadline).await;
            account_outcome(provider, accounts, name, transport)
        }
        DoctorProbe::AdapterStatus { name, transport } => one_check(
            providers::adapter_status(provider, &config, retry_policy, deadline).await,
            name,
            transport,
        ),
        DoctorProbe::AnysearchDomains { name, transport } => {
            let adapter =
                providers::build_anysearch(config.anysearch, client, retry_policy, deadline);
            one_check(
                adapter
                    .domains(AnysearchDomainsRequest {
                        domain: "academic".into(),
                        verbose: false,
                    })
                    .await,
                name,
                transport,
            )
        }
    }
}

async fn probe_web_fetch(
    provider: ProviderId,
    config: &RuntimeConfig,
    client: reqwest::Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Result<(), ProviderError> {
    let provider_config = config::web_provider_config(
        catalog::WEB_FETCH,
        provider,
        &config.tavily,
        &config.firecrawl,
        &config.jina,
    )
    .expect("probe registration belongs to the web-fetch catalog");
    let adapter =
        providers::build_web_fetch(provider, provider_config, client, retry_policy, deadline);
    adapter
        .fetch(&FetchRequest {
            source: FetchSource::Url("https://example.com/".into()),
            verbose: false,
        })
        .await
        .map(|_| ())
}

async fn probe_platform_search(
    platform: Platform,
    route_config: PlatformRouteConfig,
    client: reqwest::Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Result<PlatformSearchOutcome, ProviderError> {
    let request = PlatformSearchRequest {
        query: "forager doctor".into(),
        limit: 1,
        options: PlatformSearchOptions::defaults(platform),
        page: None,
    };
    providers::build_platform_search(route_config, client, retry_policy, deadline)
        .search(&request)
        .await
}

async fn probe_main_search(
    provider: ProviderId,
    shapes: &'static [ProbeShape],
    config: RuntimeConfig,
    client: reqwest::Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> ProbeOutcome {
    let breakers = Arc::new(ModelBreakers::default());
    let mut checks = Vec::new();
    for shape in shapes {
        let mut shape_config =
            config::main_provider_config(provider, &config.xai, &config.openai_compatible)
                .expect("probe registration belongs to the main-search catalog");
        if let MainSearchProviderConfig::OpenAiCompatible(config) = &mut shape_config {
            if let Some(stream) = shape.stream {
                config.stream = stream;
            }
            config.fallback_models.clear();
        }
        let adapter = providers::build_main_search(
            provider,
            shape_config,
            client.clone(),
            retry_policy,
            deadline,
            Arc::clone(&breakers),
        );
        match validate_transport(
            adapter.probe(probe_search_request()).await,
            shape.transport,
            shape.name,
        ) {
            Ok(()) => checks.push(ProbeCheck {
                name: shape.name,
                transport: shape.transport,
                ok: true,
            }),
            Err(error) => {
                checks.push(ProbeCheck {
                    name: shape.name,
                    transport: shape.transport,
                    ok: false,
                });
                return ProbeOutcome {
                    checks,
                    keys: None,
                    failure: Some(error.into()),
                };
            }
        }
    }
    ProbeOutcome {
        checks,
        keys: None,
        failure: None,
    }
}

fn validate_transport(
    result: Result<SearchOutcome, ProviderError>,
    expected_transport: &'static str,
    shape: &'static str,
) -> Result<(), ProviderError> {
    let outcome = result?;
    if outcome.attempts.last().is_some_and(|attempt| {
        attempt.disposition == AttemptDisposition::Succeeded
            && attempt.transport == Some(expected_transport)
    }) {
        return Ok(());
    }
    Err(ProviderError {
        kind: AttemptErrorKind::Runtime,
        message: format!("OpenAI-compatible {shape} probe completed through a different transport"),
        attempts: outcome.attempts,
        verbose: false,
        diagnostic: outcome.diagnostic,
        redirected_library_id: None,
    })
}

fn one_check<T>(
    result: Result<T, ProviderError>,
    name: &'static str,
    transport: &'static str,
) -> ProbeOutcome {
    let failure = result.err().map(ProbeFailure::from);
    ProbeOutcome {
        checks: vec![ProbeCheck {
            name,
            transport,
            ok: failure.is_none(),
        }],
        keys: None,
        failure,
    }
}

/// Reports every key's account; the first failing key, named by its config position, fails the
/// probe.
fn account_outcome(
    provider: ProviderId,
    accounts: Vec<KeyAccount>,
    name: &'static str,
    transport: &'static str,
) -> ProbeOutcome {
    let keys_key = config::provider_keys_key(provider.name());
    let mut failure = None;
    let keys = accounts
        .into_iter()
        .enumerate()
        .map(
            |(
                key_index,
                KeyAccount {
                    quota,
                    failure: key_failure,
                },
            )| {
                let error_kind = key_failure.as_ref().map(|error| error.kind.as_str());
                if let Some(error) = key_failure {
                    failure.get_or_insert_with(|| ProbeFailure {
                        kind: error.kind,
                        message: format!("{keys_key}[{key_index}]: {}", error.message),
                    });
                }
                KeyStatus {
                    key_index,
                    ok: error_kind.is_none(),
                    searches_left: quota.map(|quota| quota.searches_left),
                    plan_searches_left: quota.map(|quota| quota.plan_searches_left),
                    this_month_usage: quota.map(|quota| quota.this_month_usage),
                    this_hour_searches: quota.map(|quota| quota.this_hour_searches),
                    hourly_limit: quota.map(|quota| quota.hourly_limit),
                    error_kind,
                }
            },
        )
        .collect();
    ProbeOutcome {
        checks: vec![ProbeCheck {
            name,
            transport,
            ok: failure.is_none(),
        }],
        keys: Some(keys),
        failure,
    }
}

fn probe_search_request() -> MainSearchRequest {
    MainSearchRequest {
        query: "Reply with exactly: ok".into(),
        model: None,
        allow_model_fallback: false,
        verbose: false,
    }
}

async fn probe_reachability(
    client: reqwest::Client,
    url: String,
    limiter: Option<RateLimiter>,
    deadline: Deadline,
) -> bool {
    if !endpoint_is_valid(&url) {
        return false;
    }
    // A paced endpoint counts the probe against its request spacing like any other send.
    let _permit = match &limiter {
        Some(limiter) => match limiter.acquire(deadline).await {
            Ok(permit) => Some(permit),
            Err(_) => return false,
        },
        None => None,
    };
    let Some(remaining) = deadline.remaining() else {
        return false;
    };
    matches!(
        tokio::time::timeout(remaining, client.get(url).send()).await,
        Ok(Ok(_))
    )
}

/// The shallow result of one provider.
struct ShallowCheck {
    configured: bool,
    reachable: bool,
    message: Option<String>,
}

/// Checks an HTTP provider by a GET to its endpoint. A process route takes part only when its
/// platform order enables it; its check runs the adapter's `contract` command. A process
/// provider that only its own commands run never takes part.
async fn shallow_check(
    registration: &ProviderRegistration,
    runtime: &RuntimeConfig,
    client: &reqwest::Client,
    deadline: Deadline,
) -> ShallowCheck {
    let id = registration.id;
    let endpoint = provider_endpoint(id, runtime);
    let limiter = providers::route_limiter(id);
    let ProviderTransport::OpenCli(adapter) = registration.transport else {
        return ShallowCheck {
            configured: runtime.provider_configured(id),
            reachable: probe_reachability(client.clone(), endpoint.to_owned(), limiter, deadline)
                .await,
            message: None,
        };
    };
    if !is_platform_route(id) {
        return ShallowCheck {
            configured: false,
            reachable: false,
            message: Some(format!(
                "checked only by `forager doctor --provider {}`",
                id.name()
            )),
        };
    }
    if !route_enabled(id, runtime) {
        return ShallowCheck {
            configured: false,
            reachable: false,
            message: None,
        };
    }
    let limiter = limiter.expect("a process route declares an access policy");
    let result = providers::check_opencli_contract(endpoint, adapter, &limiter, deadline).await;
    ShallowCheck {
        configured: true,
        reachable: result.is_ok(),
        message: result.err(),
    }
}

/// Returns why the deep probe cannot run the provider, or `None` when it can. A process route
/// drives the user's browser, so doctor runs it only when a platform order enables it; a
/// process provider outside every platform runs because the user named it.
fn deep_unconfigured_reason(provider: ProviderId, runtime: &RuntimeConfig) -> Option<String> {
    let name = provider.name();
    match catalog::registration(provider).transport {
        ProviderTransport::OpenCli(_) => (is_platform_route(provider)
            && !route_enabled(provider, runtime))
        .then(|| format!("no platform order lists `{name}`; add it to the order to enable it")),
        ProviderTransport::Http => (!runtime.provider_configured(provider)).then(|| {
            format!(
                "{} has no configured credentials",
                config::provider_keys_key(name)
            )
        }),
    }
}

fn is_platform_route(id: ProviderId) -> bool {
    catalog::PLATFORMS
        .iter()
        .any(|platform| platform.contains(id))
}

/// Returns whether any platform order lists the route.
fn route_enabled(id: ProviderId, runtime: &RuntimeConfig) -> bool {
    catalog::PLATFORMS
        .iter()
        .filter(|platform| platform.contains(id))
        .any(|platform| {
            runtime
                .platforms
                .get(platform.platform)
                .entries()
                .iter()
                .any(|entry| entry.id() == id)
        })
}

fn provider_endpoint(id: ProviderId, runtime: &RuntimeConfig) -> &str {
    runtime.provider_runtime(id).endpoint
}

fn status(
    id: ProviderId,
    effective: &Value,
    check: ShallowCheck,
    runtime: &RuntimeConfig,
) -> ProviderStatus {
    let key_count = runtime.provider_runtime(id).keys.len();
    ProviderStatus {
        provider: id.name(),
        configured: check.configured,
        key_count,
        source: effective["providers"][id.name()]["keys"]["source"]
            .as_str()
            .unwrap_or("default")
            .to_owned(),
        reachable: check.reachable,
        message: check.message,
    }
}

fn endpoint_is_valid(value: &str) -> bool {
    reqwest::Url::parse(value)
        .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
}

fn main_search_fallback_warnings(main_search: &MainSearchRuntimeConfig) -> Vec<String> {
    let configured = main_search
        .entries()
        .iter()
        .filter(|entry| entry.configured())
        .collect::<Vec<_>>();
    let Some((primary, fallbacks)) = configured.split_first() else {
        return Vec::new();
    };
    fallbacks
        .iter()
        .filter(|fallback| shares_failure_domain(primary.config(), fallback.config()))
        .map(|fallback| {
            format!(
                "main search fallback `{}` uses the same endpoint and model as `{}`, so it cannot recover from failures of that endpoint or model",
                fallback.name(),
                primary.name()
            )
        })
        .collect()
}

fn shares_failure_domain(
    primary: &MainSearchProviderConfig,
    fallback: &MainSearchProviderConfig,
) -> bool {
    primary.url().trim_end_matches('/') == fallback.url().trim_end_matches('/')
        && primary.model() == fallback.model()
}

fn permission_warnings() -> Result<Vec<String>, config::ConfigError> {
    let file = config::ConfigLocation::discover()?.config_file();
    let mut warnings = Vec::new();
    check_permissions(
        file.parent().expect("configuration file has a parent"),
        0o700,
        "config directory",
        &mut warnings,
    );
    if file.exists() {
        check_permissions(&file, 0o600, "config file", &mut warnings);
    }
    Ok(warnings)
}

fn check_permissions(
    path: &std::path::Path,
    expected: u32,
    label: &str,
    warnings: &mut Vec<String>,
) {
    match config::has_private_permissions(path, expected) {
        Ok(true) => {}
        Ok(false) => warnings.push(format!(
            "{label} permissions are too broad; expected {expected:04o}"
        )),
        Err(error) => warnings.push(format!("{label} permissions cannot be inspected: {error}")),
    }
}
