use std::sync::Arc;
use std::time::Duration;

use futures_util::future::join_all;
use serde::Serialize;
use serde_json::Value;

use crate::config::{self, MainSearchProviderConfig, OpenAiCompatibleRuntimeConfig, RuntimeConfig};
use crate::net::{self, RetryPolicy};
use crate::providers::{
    self, AnysearchDomainsRequest, Context7LibraryRequest, DoctorProbe, ExaSearchRequest,
    FetchRequest, ModelBreakers, ProviderError, ProviderId, SearchRequest, SearchType,
};
use crate::types::{AttemptErrorKind, Deadline, SearchOutcome};

#[derive(Debug, Serialize)]
pub(crate) struct ShallowDoctorReport {
    mode: &'static str,
    ok: bool,
    providers: Vec<ProviderStatus>,
    permission_warnings: Vec<String>,
    config: Value,
}

#[derive(Debug, Serialize)]
struct ProviderStatus {
    provider: &'static str,
    configured: bool,
    key_count: usize,
    source: String,
    reachable: bool,
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

struct ProbeFailure {
    error: Box<ProviderError>,
    checks: Vec<ProbeCheck>,
}

pub(crate) fn shallow(timeout_seconds: u64) -> Result<ShallowDoctorReport, config::ConfigError> {
    let effective = serde_json::to_value(config::effective_view()?)
        .map_err(|error| config::ConfigError::Message(error.to_string()))?;
    let runtime_config = config::runtime_config()?;
    let client = net::build_client(runtime_config.ssl_verify)
        .map_err(|error| config::ConfigError::Message(error.to_string()))?;
    let deadline = Deadline::new(Duration::from_secs(timeout_seconds));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| config::ConfigError::Message(error.to_string()))?;
    let reachability = runtime.block_on(async {
        join_all(providers::registrations().iter().map(|registration| {
            probe_reachability(
                client.clone(),
                provider_endpoint(registration.id, &runtime_config).to_owned(),
                deadline,
            )
        }))
        .await
    });
    let providers = providers::registrations()
        .iter()
        .zip(reachability)
        .map(|(registration, reachable)| {
            status(registration.id, &runtime_config, &effective, reachable)
        })
        .collect();
    Ok(ShallowDoctorReport {
        mode: "shallow",
        ok: true,
        providers,
        permission_warnings: permission_warnings()?,
        config: effective,
    })
}

pub(crate) fn deep(
    provider: ProviderId,
    timeout_seconds: u64,
) -> Result<(DeepDoctorReport, u8), config::ConfigError> {
    let effective = serde_json::to_value(config::effective_view()?)
        .map_err(|error| config::ConfigError::Message(error.to_string()))?;
    let runtime_config = config::runtime_config()?;
    let provider_status = status(
        provider,
        &runtime_config,
        &effective,
        endpoint_is_valid(provider_endpoint(provider, &runtime_config)),
    );
    if !provider_status.configured {
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
                error_kind: Some("config"),
                message: Some(format!(
                    "providers.{}.keys has no configured credentials",
                    provider.name()
                )),
            },
            3,
        ));
    }
    let client = net::build_client(runtime_config.ssl_verify)
        .map_err(|error| config::ConfigError::Message(error.to_string()))?;
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
    let result = runtime.block_on(run_probe(
        provider,
        runtime_config,
        client,
        retry_policy,
        deadline,
    ));
    match result {
        Ok(checks) => Ok((
            DeepDoctorReport {
                mode: "deep",
                ok: true,
                provider: provider.name(),
                configured: true,
                key_count: provider_status.key_count,
                source: provider_status.source,
                deadline_seconds: timeout_seconds,
                checks,
                error_kind: None,
                message: None,
            },
            0,
        )),
        Err(failure) => Ok((
            DeepDoctorReport {
                mode: "deep",
                ok: false,
                provider: provider.name(),
                configured: true,
                key_count: provider_status.key_count,
                source: provider_status.source,
                deadline_seconds: timeout_seconds,
                checks: failure.checks,
                error_kind: Some(failure.error.kind.as_str()),
                message: Some(failure.error.message),
            },
            4,
        )),
    }
}

async fn run_probe(
    provider: ProviderId,
    config: RuntimeConfig,
    client: reqwest::Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Result<Vec<ProbeCheck>, ProbeFailure> {
    let registration = providers::registration(provider);
    match registration.doctor_probe {
        DoctorProbe::XaiResponses => {
            let MainSearchProviderConfig::Xai(provider_config) = config
                .main_search
                .provider(provider.name())
                .expect("xai registry entry has runtime configuration")
                .clone()
            else {
                unreachable!("xai registry entry uses xai configuration")
            };
            let adapter = providers::build_xai(provider_config, client, retry_policy, deadline);
            one_check(
                adapter.probe(probe_search_request()).await,
                "responses",
                "sse",
            )
        }
        DoctorProbe::OpenAiCompatibleShapes => {
            let MainSearchProviderConfig::OpenAiCompatible(provider_config) = config
                .main_search
                .provider(provider.name())
                .expect("openai-compatible registry entry has runtime configuration")
                .clone()
            else {
                unreachable!(
                    "openai-compatible registry entry uses openai-compatible configuration"
                )
            };
            probe_openai_shapes(provider_config, client, retry_policy, deadline).await
        }
        DoctorProbe::ExaSearch => {
            let adapter = providers::build_exa(config.exa, client, retry_policy, deadline);
            one_check(
                adapter
                    .search(ExaSearchRequest {
                        query: "forager doctor".into(),
                        num_results: 1,
                        search_type: SearchType::Auto,
                        include_text: false,
                        include_highlights: false,
                        start_published_date: None,
                        include_domains: Vec::new(),
                        exclude_domains: Vec::new(),
                        category: None,
                        verbose: false,
                    })
                    .await,
                "search",
                "http",
            )
        }
        DoctorProbe::TavilySearch | DoctorProbe::FirecrawlSearch => {
            let provider_config = config
                .web_search
                .provider(provider.name())
                .expect("web-search registry entry has runtime configuration")
                .clone();
            let adapter = providers::build_web_search(
                provider.name(),
                provider_config,
                client,
                retry_policy,
                deadline,
            );
            one_check(
                adapter.search("forager doctor".into(), 1).await,
                "search",
                "http",
            )
        }
        DoctorProbe::JinaFetch => {
            let provider_config = config
                .web_fetch
                .provider(provider.name())
                .expect("jina registry entry has runtime configuration")
                .clone();
            let adapter = providers::build_web_fetch(
                provider.name(),
                provider_config,
                client,
                retry_policy,
                deadline,
            );
            one_check(
                adapter
                    .fetch(&FetchRequest {
                        url: "https://example.com/".into(),
                        verbose: false,
                    })
                    .await,
                "fetch",
                "http",
            )
        }
        DoctorProbe::Context7Library => {
            let adapter =
                providers::build_context7(config.context7, client, retry_policy, deadline);
            one_check(
                adapter
                    .library(Context7LibraryRequest {
                        name: "rust".into(),
                        query: "forager doctor".into(),
                        verbose: false,
                    })
                    .await,
                "library",
                "mcp",
            )
        }
        DoctorProbe::AnysearchDomains => {
            let adapter =
                providers::build_anysearch(config.anysearch, client, retry_policy, deadline);
            one_check(
                adapter
                    .domains(AnysearchDomainsRequest {
                        domain: "academic".into(),
                        verbose: false,
                    })
                    .await,
                "domains",
                "mcp",
            )
        }
    }
}

async fn probe_openai_shapes(
    config: OpenAiCompatibleRuntimeConfig,
    client: reqwest::Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Result<Vec<ProbeCheck>, ProbeFailure> {
    let breakers = Arc::new(ModelBreakers::default());
    let mut checks = Vec::new();
    for (stream, name, transport) in [(false, "non_stream", "http"), (true, "stream", "sse")] {
        let mut shape_config = config.clone();
        shape_config.stream = stream;
        shape_config.fallback_models.clear();
        let adapter = providers::build_openai_compatible(
            shape_config,
            client.clone(),
            retry_policy,
            deadline,
            Arc::clone(&breakers),
        );
        match validate_transport(adapter.probe(probe_search_request()).await, transport, name) {
            Ok(_) => checks.push(ProbeCheck {
                name,
                transport,
                ok: true,
            }),
            Err(error) => {
                checks.push(ProbeCheck {
                    name,
                    transport,
                    ok: false,
                });
                return Err(ProbeFailure {
                    error: Box::new(error),
                    checks,
                });
            }
        }
    }
    Ok(checks)
}

fn validate_transport(
    result: Result<SearchOutcome, ProviderError>,
    expected_transport: &'static str,
    shape: &'static str,
) -> Result<(), ProviderError> {
    let outcome = result?;
    if outcome.attempts.last().is_some_and(|attempt| {
        attempt.error_kind.is_none() && attempt.transport == Some(expected_transport)
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
) -> Result<Vec<ProbeCheck>, ProbeFailure> {
    match result {
        Ok(_) => Ok(vec![ProbeCheck {
            name,
            transport,
            ok: true,
        }]),
        Err(error) => Err(ProbeFailure {
            error: Box::new(error),
            checks: vec![ProbeCheck {
                name,
                transport,
                ok: false,
            }],
        }),
    }
}

fn probe_search_request() -> SearchRequest {
    SearchRequest {
        query: "Reply with exactly: ok".into(),
        model: None,
        allow_model_fallback: false,
        verbose: false,
    }
}

async fn probe_reachability(client: reqwest::Client, url: String, deadline: Deadline) -> bool {
    if !endpoint_is_valid(&url) {
        return false;
    }
    let Some(remaining) = deadline.remaining() else {
        return false;
    };
    matches!(
        tokio::time::timeout(remaining, client.head(url).send()).await,
        Ok(Ok(_))
    )
}

fn provider_endpoint(id: ProviderId, runtime: &RuntimeConfig) -> &str {
    match id {
        ProviderId::Xai | ProviderId::OpenAiCompatible => runtime
            .main_search
            .provider(id.name())
            .expect("registry main provider has runtime configuration")
            .url(),
        ProviderId::Exa => runtime.exa.url.as_str(),
        ProviderId::Tavily => runtime.tavily.url.as_str(),
        ProviderId::Firecrawl | ProviderId::Jina => runtime
            .web_fetch
            .provider(id.name())
            .expect("registry web-fetch provider has runtime configuration")
            .url
            .as_str(),
        ProviderId::Context7 => runtime.context7.url.as_str(),
        ProviderId::Anysearch => runtime.anysearch.url.as_str(),
    }
}

fn status(
    id: ProviderId,
    runtime: &RuntimeConfig,
    effective: &Value,
    reachable: bool,
) -> ProviderStatus {
    let key_count = match id {
        ProviderId::Xai | ProviderId::OpenAiCompatible => runtime
            .main_search
            .provider(id.name())
            .expect("registry main provider has runtime configuration")
            .keys()
            .len(),
        ProviderId::Exa => runtime.exa.keys.len(),
        ProviderId::Tavily => runtime.tavily.keys.len(),
        ProviderId::Firecrawl | ProviderId::Jina => runtime
            .web_fetch
            .provider(id.name())
            .expect("registry web-fetch provider has runtime configuration")
            .keys
            .len(),
        ProviderId::Context7 => runtime.context7.keys.len(),
        ProviderId::Anysearch => runtime.anysearch.keys.len(),
    };
    ProviderStatus {
        provider: id.name(),
        configured: key_count > 0,
        key_count,
        source: effective["providers"][id.name()]["keys"]["source"]
            .as_str()
            .unwrap_or("default")
            .to_owned(),
        reachable,
    }
}

fn endpoint_is_valid(value: &str) -> bool {
    reqwest::Url::parse(value)
        .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
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

#[cfg(unix)]
fn check_permissions(
    path: &std::path::Path,
    expected: u32,
    label: &str,
    warnings: &mut Vec<String>,
) {
    use std::os::unix::fs::PermissionsExt;

    if let Ok(metadata) = std::fs::metadata(path) {
        let actual = metadata.permissions().mode() & 0o777;
        if actual & !expected != 0 {
            warnings.push(format!(
                "{label} permissions are too broad: {actual:04o}; expected {expected:04o}"
            ));
        }
    }
}

#[cfg(not(unix))]
fn check_permissions(
    _path: &std::path::Path,
    _expected: u32,
    _label: &str,
    _warnings: &mut Vec<String>,
) {
}
