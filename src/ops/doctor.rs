use std::fs;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::future::join_all;
use serde::Serialize;
use serde_json::Value;

use crate::catalog::{self, DoctorProbe, ProviderId, ProviderRegistration, ProviderTransport};
use crate::config::{self, PlatformRouteConfig, RuntimeConfig};
use crate::net::{self, RetryPolicy};
use crate::providers::{self, FetchRequest, KeyAccount, KeyQuota, MainSearchRequest};
use crate::rate_limit::RateLimiter;
use crate::redact::{Secret, redact_credentials};
use crate::state_file;
use crate::types::{
    AttemptErrorKind, ContentDepth, Deadline, Platform, PlatformFetchRequest, PlatformRef,
    PlatformSearchOptions, PlatformSearchOutcome, PlatformSearchRequest, ProviderError,
};

static PROBE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// The all-provider report, in the shallow mode or the offline mode.
#[derive(Debug, Serialize)]
pub(crate) struct DoctorReport {
    mode: &'static str,
    ok: bool,
    providers: Vec<ProviderStatus>,
    state_directories: StateDirectories,
    permission_warnings: Vec<String>,
    config: Value,
}

#[derive(Debug, Serialize)]
struct StateDirectories {
    state: DirectoryStatus,
    journal: JournalStatus,
}

impl StateDirectories {
    fn writable(&self) -> bool {
        self.state.writable
            && self
                .journal
                .directory
                .as_ref()
                .is_none_or(|directory| directory.writable)
    }
}

#[derive(Debug, Serialize)]
struct DirectoryStatus {
    writable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

/// A disabled journal writes nothing, so its directory is not checked.
#[derive(Debug, Serialize)]
struct JournalStatus {
    enabled: bool,
    #[serde(flatten)]
    directory: Option<DirectoryStatus>,
}

#[derive(Debug, Serialize)]
struct ProviderStatus {
    provider: &'static str,
    configured: bool,
    key_count: usize,
    source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reachable: Option<bool>,
    /// The browser preflight result of an enabled browser route, in the offline mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    preflight: Option<bool>,
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

/// The account report of one key, named by its position in the pool. A service that reports
/// search quotas adds them; a quota it did not report is `null`.
#[derive(Debug, Serialize)]
struct KeyStatus {
    key_index: usize,
    ok: bool,
    #[serde(flatten)]
    quota: Option<QuotaStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_kind: Option<&'static str>,
}

#[derive(Debug, Serialize)]
struct QuotaStatus {
    searches_left: Option<i64>,
    plan_searches_left: Option<i64>,
    this_month_usage: Option<i64>,
    this_hour_searches: Option<i64>,
    hourly_limit: Option<i64>,
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

/// The effective view that `config list` prints, and the runtime configuration.
fn load_configuration() -> Result<(Value, RuntimeConfig), config::ConfigError> {
    let effective = serde_json::to_value(config::effective_view()?)
        .map_err(|error| config::ConfigError::Message(error.to_string()))?;
    Ok((effective, config::runtime_config()?))
}

/// Completes an all-provider report: it fails when a state directory is not writable or
/// `providers_ok` is false.
fn report(
    mode: &'static str,
    providers: Vec<ProviderStatus>,
    providers_ok: bool,
    runtime: &RuntimeConfig,
    effective: Value,
) -> Result<(DoctorReport, u8), config::ConfigError> {
    let state_directories = state_directories(runtime);
    let ok = providers_ok && state_directories.writable();
    Ok((
        DoctorReport {
            mode,
            ok,
            providers,
            state_directories,
            permission_warnings: permission_warnings()?,
            config: effective,
        },
        if ok { 0 } else { 4 },
    ))
}

pub(crate) fn shallow(timeout_seconds: u64) -> Result<(DoctorReport, u8), config::ConfigError> {
    let (effective, runtime_config) = load_configuration()?;
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
    let reachable = providers
        .iter()
        .filter(|provider| provider.configured)
        .all(|provider| provider.reachable == Some(true));
    report("shallow", providers, reachable, &runtime_config, effective)
}

/// Runs the shallow report's checks that send nothing. No provider needs to be configured, so
/// `ok` depends only on the state directories.
pub(crate) fn offline() -> Result<(DoctorReport, u8), config::ConfigError> {
    let (effective, runtime_config) = load_configuration()?;
    let providers = catalog::registrations()
        .iter()
        .map(|registration| {
            let finding = offline_finding(local_check(registration, &runtime_config));
            status(registration.id, &effective, finding, &runtime_config)
        })
        .collect();
    report("offline", providers, true, &runtime_config, effective)
}

pub(crate) fn deep(
    provider: ProviderId,
    timeout_seconds: u64,
) -> Result<(DeepDoctorReport, u8), config::ConfigError> {
    let (effective, runtime_config) = load_configuration()?;
    let unconfigured = deep_unconfigured_reason(provider, &runtime_config);
    let provider_status = status(
        provider,
        &effective,
        ProviderFinding {
            configured: unconfigured.is_none(),
            check: None,
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

async fn probe_platform_fetch(
    route_config: PlatformRouteConfig,
    reference: PlatformRef,
    client: reqwest::Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Result<(), ProviderError> {
    let request = PlatformFetchRequest::new(reference, ContentDepth::Metadata);
    providers::build_platform_fetch(route_config, client, retry_policy, deadline)
        .fetch(&request)
        .await
        .map(drop)
}

async fn run_probe(
    provider: ProviderId,
    config: RuntimeConfig,
    client: reqwest::Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> ProbeOutcome {
    match catalog::registration(provider).probe {
        DoctorProbe::MainSearch { name, transport } => {
            probe_main_search_backend(
                provider,
                (name, transport),
                config,
                client,
                retry_policy,
                deadline,
            )
            .await
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
            let provider_config = config::docs_provider_config(provider, &config.exa)
                .expect("probe registration belongs to the docs-search catalog");
            let adapter =
                providers::build_docs_search(provider_config, client, retry_policy, deadline);
            one_check(adapter.search("forager doctor", 1).await, name, transport)
        }
        DoctorProbe::PlatformSearch {
            platform,
            name,
            transport,
        } => {
            let route_config = config::platform_route_config(provider, &config.platform_routes)
                .expect("probe registration belongs to a platform catalog");
            probe_platform_route(
                platform,
                route_config,
                (name, transport),
                client,
                retry_policy,
                deadline,
            )
            .await
        }
        DoctorProbe::PlatformFetch {
            platform,
            canary,
            name,
            transport,
        } => {
            let route_config = config::platform_route_config(provider, &config.platform_routes)
                .expect("probe registration belongs to a platform catalog");
            let reference =
                PlatformRef::parse(platform, canary).expect("a registered canary ref parses");
            one_check(
                probe_platform_fetch(route_config, reference, client, retry_policy, deadline).await,
                name,
                transport,
            )
        }
        DoctorProbe::ServiceAccount { name, transport } => {
            let route_config = config::platform_route_config(provider, &config.platform_routes)
                .expect("an account probe belongs to a platform route");
            let accounts =
                providers::route_accounts(route_config, client, retry_policy, deadline).await;
            account_outcome(provider, accounts, name, transport)
        }
    }
}

/// Probes a main-search backend, then the platform searches it serves while their orders list
/// it.
async fn probe_main_search_backend(
    provider: ProviderId,
    (name, transport): (&'static str, &'static str),
    config: RuntimeConfig,
    client: reqwest::Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> ProbeOutcome {
    // A main-search backend that a platform order lists also serves that platform, so
    // doctor proves the platform request too (ADR-0026).
    let platform_routes = enabled_platforms(provider, &config)
        .into_iter()
        .filter_map(|platform| {
            config::platform_route_config(provider, &config.platform_routes)
                .map(|route_config| (platform, route_config))
        })
        .collect::<Vec<_>>();
    let provider_config = config::main_provider_config(provider, &config.main_search_providers())
        .expect("probe registration belongs to the main-search catalog");
    let result =
        providers::build_main_search(provider_config, client.clone(), retry_policy, deadline)
            .probe(probe_search_request())
            .await;
    let mut outcome = one_check(result, name, transport);
    for (platform, route_config) in platform_routes {
        if outcome.failure.is_some() {
            break;
        }
        let result = probe_platform_search(
            platform,
            route_config,
            client.clone(),
            retry_policy,
            deadline,
        )
        .await;
        let platform_outcome = one_check(result, platform_probe_name(platform), "sse");
        outcome.checks.extend(platform_outcome.checks);
        outcome.failure = platform_outcome.failure;
    }
    outcome
}

/// Probes a platform route with one search, then each extra endpoint it combines (ADR-0027).
async fn probe_platform_route(
    platform: Platform,
    route_config: PlatformRouteConfig,
    (name, transport): (&'static str, &'static str),
    client: reqwest::Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> ProbeOutcome {
    let result = probe_platform_search(
        platform,
        route_config.clone(),
        client.clone(),
        retry_policy,
        deadline,
    )
    .await;
    let mut outcome = one_check(result, name, transport);
    if outcome.failure.is_some() {
        return outcome;
    }
    for (check, result) in
        providers::platform_route_extra_checks(route_config, client, retry_policy, deadline).await
    {
        let extra = one_check(result, check, transport);
        outcome.checks.extend(extra.checks);
        if extra.failure.is_some() {
            outcome.failure = extra.failure;
            break;
        }
    }
    outcome
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
    let keyed = !provider_config.keys.is_empty();
    let adapter =
        providers::build_web_fetch(provider, provider_config, client, retry_policy, deadline);
    let result = adapter
        .fetch(&FetchRequest {
            url: "https://example.com/".into(),
            verbose: false,
        })
        .await;
    let attempts = match &result {
        Ok(outcome) => &outcome.attempts,
        Err(error) => &error.attempts,
    };
    // The Anonymous Fallback runs only after the keys fail, and doctor checks the keys.
    let key_failure = attempts
        .iter()
        .any(|attempt| attempt.anonymous)
        .then(|| attempts.iter().rfind(|attempt| !attempt.anonymous))
        .flatten()
        .filter(|_| keyed)
        .map(|attempt| ProviderError {
            kind: attempt.error_kind.unwrap_or(AttemptErrorKind::Auth),
            message: attempt.message.clone(),
            attempts: Vec::new(),
            verbose: false,
            diagnostic: None,
        });
    match key_failure {
        Some(error) => Err(error),
        None => result.map(|_| ()),
    }
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
                    quota: quota_status(quota),
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

/// Reports a key's quota: nothing for a service that reports none, and every field `null` when
/// the check could not read it.
fn quota_status(quota: Option<KeyQuota>) -> Option<QuotaStatus> {
    let read = match quota? {
        KeyQuota::Unread => None,
        KeyQuota::Read(quota) => Some(quota),
    };
    Some(QuotaStatus {
        searches_left: read.map(|quota| quota.searches_left),
        plan_searches_left: read.map(|quota| quota.plan_searches_left),
        this_month_usage: read.map(|quota| quota.this_month_usage),
        this_hour_searches: read.map(|quota| quota.this_hour_searches),
        hourly_limit: read.map(|quota| quota.hourly_limit),
    })
}

fn probe_search_request() -> MainSearchRequest {
    MainSearchRequest {
        query: "Reply with exactly: ok".into(),
        model: None,
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

/// What doctor learns about one provider without sending a request.
enum LocalCheck {
    /// A platform route that users must enable, while no order lists it.
    NotEnabled,
    Http {
        configured: bool,
        anonymous: bool,
    },
    /// The browser preflight check, which launches no browser and opens no site.
    Browser {
        preflight: Result<(), String>,
    },
}

/// Checks a provider without sending a request. A platform route that users must enable takes
/// part only when an order enables it.
fn local_check(registration: &ProviderRegistration, runtime: &RuntimeConfig) -> LocalCheck {
    let id = registration.id;
    if is_opt_in_route(id) && !route_enabled(id, runtime) {
        return LocalCheck::NotEnabled;
    }
    match registration.transport {
        ProviderTransport::Http => LocalCheck::Http {
            configured: runtime.provider_configured(id),
            anonymous: registration.lacks_optional_keys(runtime.provider_runtime(id).keys.len()),
        },
        ProviderTransport::Browser(_) => LocalCheck::Browser {
            preflight: providers::browser_preflight(id, &runtime.browser),
        },
    }
}

/// What a report states about one provider.
struct ProviderFinding {
    configured: bool,
    check: Option<ProviderCheck>,
    message: Option<String>,
}

/// The one check a report ran on a provider beyond its configuration.
enum ProviderCheck {
    Reachable(bool),
    Preflight(bool),
}

fn offline_finding(check: LocalCheck) -> ProviderFinding {
    match check {
        LocalCheck::NotEnabled => ProviderFinding {
            configured: false,
            check: None,
            message: None,
        },
        LocalCheck::Http {
            configured,
            anonymous,
        } => ProviderFinding {
            configured,
            check: None,
            message: anonymous_note(anonymous),
        },
        LocalCheck::Browser { preflight } => ProviderFinding {
            configured: true,
            check: Some(ProviderCheck::Preflight(preflight.is_ok())),
            message: preflight.err(),
        },
    }
}

/// Checks an HTTP provider by a GET to its endpoint, and a browser route by its preflight
/// check.
async fn shallow_check(
    registration: &ProviderRegistration,
    runtime: &RuntimeConfig,
    client: &reqwest::Client,
    deadline: Deadline,
) -> ProviderFinding {
    let id = registration.id;
    match local_check(registration, runtime) {
        LocalCheck::NotEnabled => ProviderFinding {
            configured: false,
            check: Some(ProviderCheck::Reachable(false)),
            message: None,
        },
        LocalCheck::Http {
            configured,
            anonymous,
        } => ProviderFinding {
            configured,
            check: Some(ProviderCheck::Reachable(
                probe_reachability(
                    client.clone(),
                    provider_endpoint(id, runtime).to_owned(),
                    providers::route_limiter(id),
                    deadline,
                )
                .await,
            )),
            message: anonymous_note(anonymous),
        },
        // The shallow report has always called a browser route's preflight check `reachable`.
        LocalCheck::Browser { preflight } => ProviderFinding {
            configured: true,
            check: Some(ProviderCheck::Reachable(preflight.is_ok())),
            message: preflight.err(),
        },
    }
}

fn anonymous_note(anonymous: bool) -> Option<String> {
    anonymous.then(|| "runs anonymously without keys".to_owned())
}

/// Returns why the deep probe cannot run the provider, or `None` when it can. A platform route
/// that users must enable (one driving a browser, or one reading undocumented endpoints) runs
/// only when a platform order enables it; every provider also needs its prerequisites.
fn deep_unconfigured_reason(provider: ProviderId, runtime: &RuntimeConfig) -> Option<String> {
    if is_opt_in_route(provider) && !route_enabled(provider, runtime) {
        return Some(format!(
            "no platform order lists `{}`; add it to the order to enable it",
            provider.name()
        ));
    }
    providers::route_prerequisites(provider, runtime).err()
}

/// Returns whether the route serves a platform but no default order lists it, so only the
/// user's order enables it.
fn is_opt_in_route(id: ProviderId) -> bool {
    catalog::PLATFORMS
        .iter()
        .any(|platform| platform.contains(id))
        && !catalog::PLATFORMS
            .iter()
            .any(|platform| platform.default_order.contains(&id))
}

/// Returns whether any platform order lists the route.
fn route_enabled(id: ProviderId, runtime: &RuntimeConfig) -> bool {
    !enabled_platforms(id, runtime).is_empty()
}

/// Names the platform check of a main-search backend after the tool that reads the platform.
fn platform_probe_name(platform: Platform) -> &'static str {
    match platform {
        Platform::X => "x_search",
        _ => "platform_search",
    }
}

/// Returns the platforms whose order lists the route.
fn enabled_platforms(id: ProviderId, runtime: &RuntimeConfig) -> Vec<Platform> {
    catalog::PLATFORMS
        .iter()
        .filter(|platform| platform.contains(id))
        .filter(|platform| {
            runtime
                .platforms
                .get(platform.platform)
                .entries()
                .iter()
                .any(|entry| entry.id() == id)
        })
        .map(|platform| platform.platform)
        .collect()
}

fn provider_endpoint(id: ProviderId, runtime: &RuntimeConfig) -> &str {
    runtime.provider_runtime(id).endpoint
}

fn status(
    id: ProviderId,
    effective: &Value,
    finding: ProviderFinding,
    runtime: &RuntimeConfig,
) -> ProviderStatus {
    let key_count = runtime.provider_runtime(id).keys.len();
    ProviderStatus {
        provider: id.name(),
        configured: finding.configured,
        key_count,
        source: effective["providers"][id.name()]["keys"]["source"]
            .as_str()
            .unwrap_or("default")
            .to_owned(),
        reachable: match finding.check {
            Some(ProviderCheck::Reachable(reachable)) => Some(reachable),
            _ => None,
        },
        preflight: match finding.check {
            Some(ProviderCheck::Preflight(passed)) => Some(passed),
            _ => None,
        },
        message: finding.message,
    }
}

fn endpoint_is_valid(value: &str) -> bool {
    reqwest::Url::parse(value)
        .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
}

/// Checks that each state directory forager writes can be created and written privately.
fn state_directories(runtime: &RuntimeConfig) -> StateDirectories {
    let credentials = &runtime.journal.credentials;
    let state = match state_file::state_directory() {
        Some(path) => directory_status(&path, credentials),
        None => DirectoryStatus {
            writable: false,
            message: Some("XDG_STATE_HOME and HOME do not resolve to an absolute path".into()),
        },
    };
    let journal = JournalStatus {
        enabled: runtime.journal.enabled,
        directory: runtime
            .journal
            .enabled
            .then(|| directory_status(&runtime.journal.dir, credentials)),
    };
    StateDirectories { state, journal }
}

fn directory_status(path: &Path, credentials: &[Secret]) -> DirectoryStatus {
    match probe_private_directory(path) {
        Ok(()) => DirectoryStatus {
            writable: true,
            message: None,
        },
        Err(error) => DirectoryStatus {
            writable: false,
            message: Some(redact_credentials(&error.to_string(), credentials)),
        },
    }
}

/// Creates `path` as a private directory and writes and removes a private file in it.
pub(crate) fn probe_private_directory(path: &Path) -> io::Result<()> {
    config::ensure_private_directory(path)?;
    let sequence = PROBE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let probe = path.join(format!(
        ".forager-doctor-probe-{}-{nanos}-{sequence}",
        std::process::id()
    ));
    let result = config::create_private_file(&probe).and_then(|file| file.sync_all());
    let cleanup = fs::remove_file(&probe);
    result.and(cleanup)
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
