use std::sync::Arc;
use std::time::Duration;

use futures_util::future::join_all;
use serde::Serialize;
use serde_json::Value;

use crate::config::{self, MainSearchProviderConfig, RuntimeConfig};
use crate::net::{self, RetryPolicy};
use crate::providers::{
    self, AnysearchDomainsRequest, DoctorProbe, FetchRequest, MainSearchRequest, ModelBreakers,
    ProviderError, ProviderId,
};
use crate::types::{AttemptDisposition, AttemptErrorKind, Deadline, SearchOutcome};

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
    match providers::registration(provider).probe {
        DoctorProbe::MainSearch(shapes) => {
            probe_main_search(provider, shapes, config, client, retry_policy, deadline).await
        }
        DoctorProbe::WebSearch { name, transport } => {
            let provider_config = providers::catalog::web_config(
                providers::catalog::WEB_SEARCH,
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
        DoctorProbe::WebFetch { name, transport } => {
            let provider_config = providers::catalog::web_config(
                providers::catalog::WEB_FETCH,
                provider,
                &config.tavily,
                &config.firecrawl,
                &config.jina,
            )
            .expect("probe registration belongs to the web-fetch catalog");
            let adapter = providers::build_web_fetch(
                provider,
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
                name,
                transport,
            )
        }
        DoctorProbe::DocsSearch { name, transport } => {
            let provider_config =
                providers::catalog::docs_config(provider, &config.exa, &config.context7)
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

async fn probe_main_search(
    provider: ProviderId,
    shapes: &'static [providers::ProbeShape],
    config: RuntimeConfig,
    client: reqwest::Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Result<Vec<ProbeCheck>, ProbeFailure> {
    let breakers = Arc::new(ModelBreakers::default());
    let mut checks = Vec::new();
    for shape in shapes {
        let mut shape_config =
            providers::catalog::main_config(provider, &config.xai, &config.openai_compatible)
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

fn probe_search_request() -> MainSearchRequest {
    MainSearchRequest {
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
    runtime.provider_runtime(id).endpoint
}

fn status(
    id: ProviderId,
    runtime: &RuntimeConfig,
    effective: &Value,
    reachable: bool,
) -> ProviderStatus {
    let key_count = runtime.provider_runtime(id).keys.len();
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
