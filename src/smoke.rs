use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;
use shared_child::SharedChild;

use crate::config::{self, RuntimeConfig};
use crate::credentials;
use crate::providers::{self, ProviderId};
use crate::types::Deadline;

static PROBE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
pub(crate) const MAIN_CANARY_QUERY: &str = "What is the latest stable Rust release?";
pub(crate) const PIPELINE_CANARY_QUERY: &str =
    "Find current Rust release news and official documentation.";
pub(crate) const RESEARCH_CANARY_QUERY: &str = "What is the current status of async drop in Rust?";
const FETCH_CANARY_URL: &str = "https://www.rust-lang.org/";
const ANYSEARCH_CANARY_QUERY: &str = "retrieval augmented generation";
const EXPECTED_PROVIDERS: [&str; 8] = [
    "anysearch",
    "context7",
    "exa",
    "firecrawl",
    "jina",
    "openai_compatible",
    "tavily",
    "xai",
];
const SPECIFICATION_CASE_IDS: [&str; 19] = [
    "P1", "P2", "C01", "C02", "C03", "C04", "C05", "C06", "C07", "C08", "C09", "C10", "C11", "C12",
    "C13", "C14", "C15", "C16", "C17",
];
const LIVE_CASES: [LiveCaseDefinition; 19] = [
    LiveCaseDefinition::pipeline("P1", "search"),
    LiveCaseDefinition::pipeline("P2", "research"),
    LiveCaseDefinition::provider("C01", "xai", "main_search", "sse"),
    LiveCaseDefinition::provider(
        "C02",
        "openai_compatible",
        "main_search_stream_false",
        "http",
    ),
    LiveCaseDefinition::provider("C03", "openai_compatible", "main_search_stream_true", "sse"),
    LiveCaseDefinition::provider("C04", "classifier", "capability_and_plan", "http"),
    LiveCaseDefinition::provider("C05", "tavily", "web_search", "http"),
    LiveCaseDefinition::provider("C06", "firecrawl", "web_search", "http"),
    LiveCaseDefinition::provider("C07", "jina", "web_fetch", "http"),
    LiveCaseDefinition::provider("C08", "tavily", "web_fetch", "http"),
    LiveCaseDefinition::provider("C09", "firecrawl", "web_fetch", "http"),
    LiveCaseDefinition::provider("C10", "context7", "library_resolve", "mcp"),
    LiveCaseDefinition::provider("C11", "context7", "docs", "mcp"),
    LiveCaseDefinition::provider("C12", "exa", "docs_search", "http"),
    LiveCaseDefinition::provider("C13", "exa", "similar", "http"),
    LiveCaseDefinition::provider("C14", "anysearch", "academic.search", "mcp"),
    LiveCaseDefinition::provider("C15", "anysearch", "vertical_discovery", "mcp"),
    LiveCaseDefinition::provider("C16", "anysearch", "domains", "mcp"),
    LiveCaseDefinition::provider("C17", "tavily", "site_map", "http"),
];

#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) struct LiveCaseDefinition {
    id: &'static str,
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<&'static str>,
    operation: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    transport: Option<&'static str>,
}

impl LiveCaseDefinition {
    const fn pipeline(id: &'static str, operation: &'static str) -> Self {
        Self {
            id,
            kind: "pipeline",
            provider: None,
            operation,
            transport: None,
        }
    }

    const fn provider(
        id: &'static str,
        provider: &'static str,
        operation: &'static str,
        transport: &'static str,
    ) -> Self {
        Self {
            id,
            kind: "provider_contract",
            provider: Some(provider),
            operation,
            transport: Some(transport),
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct LiveRegistry {
    mode: &'static str,
    registered_case_ids: Vec<&'static str>,
    specification_case_ids: Vec<&'static str>,
    cases: &'static [LiveCaseDefinition],
}

#[derive(Debug, Serialize)]
pub(crate) struct LiveReport {
    mode: &'static str,
    ok: bool,
    registered_case_ids: Vec<&'static str>,
    specification_case_ids: Vec<&'static str>,
    summary: LiveSummary,
    cases: Vec<LiveCaseResult>,
}

#[derive(Debug, Default, Serialize)]
struct LiveSummary {
    passed: usize,
    failed: usize,
    deferred: usize,
    unconfigured: usize,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum LiveCaseStatus {
    Passed,
    Failed,
    Deferred,
    Unconfigured,
}

#[derive(Debug, Serialize)]
struct LiveCaseResult {
    #[serde(flatten)]
    definition: LiveCaseDefinition,
    status: LiveCaseStatus,
    attempts: usize,
    checked_at_unix_seconds: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    outage_evidence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<&'static str>,
}

#[derive(Clone, Copy)]
enum ResultShape {
    MainSearch,
    PipelineSearch,
    Research,
    Classifier,
    SupplementalSearch,
    Fetch,
    Context7Library,
    Context7Docs,
    Exa,
    Anysearch,
    Map,
}

pub(crate) enum ProbeKind {
    Classifier,
    Supplemental(ProviderId),
    Outage,
}

pub(crate) fn probe_kind(case_id: &str) -> Result<ProbeKind, String> {
    match case_id {
        "C04" => Ok(ProbeKind::Classifier),
        "C05" => Ok(ProbeKind::Supplemental(ProviderId::Tavily)),
        "C06" => Ok(ProbeKind::Supplemental(ProviderId::Firecrawl)),
        "OUTAGE" => Ok(ProbeKind::Outage),
        _ => Err("unknown internal smoke probe".into()),
    }
}

struct CommandSpec {
    arguments: Vec<String>,
    environment: Vec<(&'static str, &'static str)>,
    shape: ResultShape,
}

#[derive(Debug, Serialize)]
pub(crate) struct SmokeReport {
    mode: &'static str,
    ok: bool,
    registry: RegistryStatus,
    providers: Vec<CredentialStatus>,
    classifier: CredentialStatus,
    journal: DirectoryStatus,
    credential_cursor: DirectoryStatus,
    permissions: PermissionStatus,
}

#[derive(Debug, Serialize)]
struct RegistryStatus {
    ok: bool,
    provider_count: usize,
}

#[derive(Debug, Serialize)]
struct CredentialStatus {
    provider: &'static str,
    configured: bool,
    key_count: usize,
    keys: Vec<&'static str>,
    source: String,
}

#[derive(Debug, Serialize)]
struct DirectoryStatus {
    #[serde(skip_serializing_if = "Option::is_none")]
    enabled: Option<bool>,
    writable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Debug, Serialize)]
struct PermissionStatus {
    ok: bool,
    issues: Vec<String>,
}

pub(crate) fn live_registry() -> LiveRegistry {
    LiveRegistry {
        mode: "live_registry",
        registered_case_ids: LIVE_CASES.iter().map(|case| case.id).collect(),
        specification_case_ids: SPECIFICATION_CASE_IDS.to_vec(),
        cases: &LIVE_CASES,
    }
}

pub(crate) fn parse_outage_evidence(values: &[String]) -> Result<BTreeMap<String, String>, String> {
    let registered = LIVE_CASES
        .iter()
        .map(|case| case.id)
        .collect::<BTreeSet<_>>();
    let mut parsed = BTreeMap::new();
    for value in values {
        let (case_id, evidence) = value
            .split_once('=')
            .ok_or_else(|| "outage evidence must use CASE_ID=EVIDENCE_URL".to_owned())?;
        if !registered.contains(case_id) {
            return Err(format!(
                "outage evidence references unknown live case `{case_id}`"
            ));
        }
        if !(evidence.starts_with("https://") || evidence.starts_with("http://")) {
            return Err(format!(
                "outage evidence for `{case_id}` must be an official status or independent probe URL"
            ));
        }
        if parsed.insert(case_id.into(), evidence.into()).is_some() {
            return Err(format!(
                "duplicate outage evidence for live case `{case_id}`"
            ));
        }
    }
    Ok(parsed)
}

fn verify_outage_evidence(
    case_id: &str,
    evidence: &str,
    runtime: &RuntimeConfig,
    deadline: Deadline,
) -> Option<String> {
    if !evidence_matches_case_endpoint(case_id, evidence, runtime)
        && !official_status_page_matches_case(case_id, evidence, runtime)
    {
        return None;
    }
    let timeout = remaining_seconds(deadline)?.min(5);
    let executable = std::env::current_exe().ok()?;
    let mut command = Command::new(executable);
    command.args([
        "smoke",
        "--probe",
        "OUTAGE",
        "--probe-url",
        evidence,
        "--probe-timeout",
        &timeout.to_string(),
    ]);
    let output = execute_with_deadline(command, deadline).ok()?;
    let payload: Value = serde_json::from_slice(&output.stdout).ok()?;
    (output.status.code() == Some(4) && payload["outage"] == true)
        .then(|| config::redact_url(evidence))
}

fn official_status_page_matches_case(case_id: &str, url: &str, runtime: &RuntimeConfig) -> bool {
    let Some(host) = official_status_host(url) else {
        return false;
    };
    match case_id {
        "P1" => pipeline_status_matches(runtime, host, false),
        "P2" => pipeline_status_matches(runtime, host, true),
        "C04" => status_host_matches_endpoint(host, &runtime.classifier.url),
        _ => provider_for_case(case_id)
            .and_then(|provider| provider_endpoint(runtime, provider))
            .is_some_and(|endpoint| status_host_matches_endpoint(host, endpoint)),
    }
}

fn provider_for_case(case_id: &str) -> Option<ProviderId> {
    match case_id {
        "C01" => Some(ProviderId::Xai),
        "C02" | "C03" => Some(ProviderId::OpenAiCompatible),
        "C05" | "C08" | "C17" => Some(ProviderId::Tavily),
        "C06" | "C09" => Some(ProviderId::Firecrawl),
        "C07" => Some(ProviderId::Jina),
        "C10" | "C11" => Some(ProviderId::Context7),
        "C12" | "C13" => Some(ProviderId::Exa),
        "C14" | "C15" | "C16" => Some(ProviderId::Anysearch),
        _ => None,
    }
}

fn pipeline_status_matches(runtime: &RuntimeConfig, status_host: &str, research: bool) -> bool {
    let classifier_matches = runtime.classifier.configured()
        && status_host_matches_endpoint(status_host, &runtime.classifier.url);
    let main_matches = runtime.main_search.backends.iter().any(|provider| {
        runtime
            .main_search
            .provider(provider)
            .filter(|config| config.configured())
            .is_some_and(|config| status_host_matches_endpoint(status_host, config.url()))
    });
    let web_search_matches = runtime.web_search.order.iter().any(|provider| {
        runtime
            .web_search
            .provider(provider)
            .filter(|config| !config.keys.is_empty())
            .is_some_and(|config| status_host_matches_endpoint(status_host, &config.url))
    });
    classifier_matches
        || (!research && main_matches)
        || web_search_matches
        || (research && research_status_matches(runtime, status_host))
}

fn research_status_matches(runtime: &RuntimeConfig, status_host: &str) -> bool {
    [
        (ProviderId::Exa, &runtime.docs_search.order),
        (ProviderId::Context7, &runtime.docs_search.order),
        (ProviderId::Anysearch, &runtime.vertical_search.order),
        (ProviderId::Tavily, &runtime.web_fetch.order),
        (ProviderId::Jina, &runtime.web_fetch.order),
        (ProviderId::Firecrawl, &runtime.web_fetch.order),
    ]
    .into_iter()
    .any(|(provider, order)| {
        order.iter().any(|name| name == provider.name())
            && provider_is_configured(runtime, provider)
            && provider_endpoint(runtime, provider)
                .is_some_and(|endpoint| status_host_matches_endpoint(status_host, endpoint))
    })
}

fn status_host_matches_endpoint(status_host: &str, endpoint: &str) -> bool {
    let Ok(endpoint) = reqwest::Url::parse(endpoint) else {
        return false;
    };
    let Some(endpoint_host) = endpoint.host_str() else {
        return false;
    };
    match status_host {
        "status.x.ai" => endpoint_host == "x.ai" || endpoint_host.ends_with(".x.ai"),
        "status.openai.com" => {
            endpoint_host == "openai.com" || endpoint_host.ends_with(".openai.com")
        }
        "status.tavily.com" => {
            endpoint_host == "tavily.com" || endpoint_host.ends_with(".tavily.com")
        }
        "status.firecrawl.dev" => {
            endpoint_host == "firecrawl.dev" || endpoint_host.ends_with(".firecrawl.dev")
        }
        "status.jina.ai" => endpoint_host == "jina.ai" || endpoint_host.ends_with(".jina.ai"),
        "status.upstash.com" => {
            endpoint_host == "context7.com"
                || endpoint_host.ends_with(".context7.com")
                || endpoint_host == "upstash.com"
                || endpoint_host.ends_with(".upstash.com")
        }
        "status.exa.ai" => endpoint_host == "exa.ai" || endpoint_host.ends_with(".exa.ai"),
        "status.anysearch.com" => {
            endpoint_host == "anysearch.com" || endpoint_host.ends_with(".anysearch.com")
        }
        _ => false,
    }
}

fn official_status_host(url: &str) -> Option<&'static str> {
    let Ok(url) = reqwest::Url::parse(url) else {
        return None;
    };
    if url.scheme() != "https" {
        return None;
    }
    match url.host_str()? {
        "status.x.ai" => Some("status.x.ai"),
        "status.openai.com" => Some("status.openai.com"),
        "status.tavily.com" => Some("status.tavily.com"),
        "status.firecrawl.dev" => Some("status.firecrawl.dev"),
        "status.jina.ai" => Some("status.jina.ai"),
        "status.upstash.com" => Some("status.upstash.com"),
        "status.exa.ai" => Some("status.exa.ai"),
        "status.anysearch.com" => Some("status.anysearch.com"),
        _ => None,
    }
}

pub(crate) fn is_official_status_url(url: &str) -> bool {
    official_status_host(url).is_some()
}

pub(crate) fn status_page_reports_outage(body: &str) -> bool {
    if let Ok(payload) = serde_json::from_str::<Value>(body) {
        if payload
            .pointer("/status/indicator")
            .and_then(Value::as_str)
            .is_some_and(|indicator| !matches!(indicator, "none" | "operational"))
        {
            return true;
        }
        if payload["incidents"].as_array().is_some_and(|incidents| {
            incidents.iter().any(|incident| {
                incident["status"].as_str().is_some_and(|status| {
                    !matches!(status, "resolved" | "postmortem" | "completed")
                })
            })
        }) {
            return true;
        }
    }
    let body = body.to_ascii_lowercase();
    let current = ["past incidents", "incident history", "uptime history"]
        .iter()
        .filter_map(|marker| body.find(marker))
        .min()
        .map_or(body.as_str(), |index| &body[..index]);
    if current.contains("all systems operational")
        || current.contains("no incidents declared")
        || current.contains("no active incidents")
        || (current.contains("resolved")
            && !["investigating", "identified", "monitoring"]
                .iter()
                .any(|marker| current.contains(marker)))
    {
        return false;
    }
    [
        "major outage",
        "partial outage",
        "degraded performance",
        "service disruption",
        "status-investigating",
        "\"status\":\"investigating\"",
        "\"indicator\":\"major\"",
        "\"indicator\":\"critical\"",
    ]
    .iter()
    .any(|marker| current.contains(marker))
}

fn evidence_matches_case_endpoint(case_id: &str, evidence: &str, runtime: &RuntimeConfig) -> bool {
    let matches = |endpoint: &str| same_origin(evidence, endpoint);
    match case_id {
        "P1" | "P2" => {
            matches(&runtime.classifier.url)
                || providers::registrations().iter().any(|registration| {
                    provider_endpoint(runtime, registration.id).is_some_and(&matches)
                })
        }
        "C01" => provider_endpoint(runtime, ProviderId::Xai).is_some_and(matches),
        "C02" | "C03" => {
            provider_endpoint(runtime, ProviderId::OpenAiCompatible).is_some_and(matches)
        }
        "C04" => matches(&runtime.classifier.url),
        "C05" | "C08" | "C17" => {
            provider_endpoint(runtime, ProviderId::Tavily).is_some_and(matches)
        }
        "C06" | "C09" => provider_endpoint(runtime, ProviderId::Firecrawl).is_some_and(matches),
        "C07" => provider_endpoint(runtime, ProviderId::Jina).is_some_and(matches),
        "C10" | "C11" => provider_endpoint(runtime, ProviderId::Context7).is_some_and(matches),
        "C12" | "C13" => provider_endpoint(runtime, ProviderId::Exa).is_some_and(matches),
        "C14" | "C15" | "C16" => {
            provider_endpoint(runtime, ProviderId::Anysearch).is_some_and(matches)
        }
        _ => false,
    }
}

fn provider_endpoint(runtime: &RuntimeConfig, provider: ProviderId) -> Option<&str> {
    match provider {
        ProviderId::Xai | ProviderId::OpenAiCompatible => runtime
            .main_search
            .provider(provider.name())
            .map(|config| config.url()),
        ProviderId::Exa => Some(&runtime.exa.url),
        ProviderId::Tavily => Some(&runtime.tavily.url),
        ProviderId::Firecrawl | ProviderId::Jina => runtime
            .web_fetch
            .provider(provider.name())
            .map(|config| config.url.as_str()),
        ProviderId::Context7 => Some(&runtime.context7.url),
        ProviderId::Anysearch => Some(&runtime.anysearch.url),
    }
}

fn same_origin(first: &str, second: &str) -> bool {
    let (Ok(first), Ok(second)) = (reqwest::Url::parse(first), reqwest::Url::parse(second)) else {
        return false;
    };
    first.scheme() == second.scheme()
        && first.host_str() == second.host_str()
        && first.port_or_known_default() == second.port_or_known_default()
}

pub(crate) fn run_live(
    timeout_seconds: u64,
    outage_evidence: &BTreeMap<String, String>,
) -> Result<(LiveReport, u8), config::ConfigError> {
    let runtime = config::runtime_config()?;
    let deadline = Deadline::new(Duration::from_secs(timeout_seconds));
    let mut results = Vec::with_capacity(LIVE_CASES.len());
    let mut summary = LiveSummary::default();

    for definition in LIVE_CASES {
        let result = if !case_is_configured(definition.id, &runtime) {
            LiveCaseResult {
                definition,
                status: LiveCaseStatus::Unconfigured,
                attempts: 0,
                checked_at_unix_seconds: unix_timestamp(),
                outage_evidence: None,
                message: Some("required unified credentials are not configured"),
            }
        } else {
            run_configured_case(definition, &runtime, deadline, outage_evidence)
        };
        match result.status {
            LiveCaseStatus::Passed => summary.passed += 1,
            LiveCaseStatus::Failed => summary.failed += 1,
            LiveCaseStatus::Deferred => summary.deferred += 1,
            LiveCaseStatus::Unconfigured => summary.unconfigured += 1,
        }
        results.push(result);
    }

    let ok = summary.passed == LIVE_CASES.len();
    Ok((
        LiveReport {
            mode: "live",
            ok,
            registered_case_ids: LIVE_CASES.iter().map(|case| case.id).collect(),
            specification_case_ids: SPECIFICATION_CASE_IDS.to_vec(),
            summary,
            cases: results,
        },
        if ok { 0 } else { 4 },
    ))
}

fn run_configured_case(
    definition: LiveCaseDefinition,
    runtime: &RuntimeConfig,
    deadline: Deadline,
    outage_evidence: &BTreeMap<String, String>,
) -> LiveCaseResult {
    let mut failure = "live case failed";
    let mut attempts = 0;
    for attempt in 1..=3 {
        if deadline.remaining().is_none() {
            failure = "live smoke hard deadline elapsed";
            break;
        }
        attempts = attempt;
        match run_case_once(definition.id, deadline) {
            Ok(()) => {
                return LiveCaseResult {
                    definition,
                    status: LiveCaseStatus::Passed,
                    attempts: attempt,
                    checked_at_unix_seconds: unix_timestamp(),
                    outage_evidence: None,
                    message: None,
                };
            }
            Err(message) => failure = message,
        }
    }
    let evidence = outage_evidence
        .get(definition.id)
        .and_then(|evidence| verify_outage_evidence(definition.id, evidence, runtime, deadline));
    LiveCaseResult {
        definition,
        status: if evidence.is_some() {
            LiveCaseStatus::Deferred
        } else {
            LiveCaseStatus::Failed
        },
        attempts,
        checked_at_unix_seconds: unix_timestamp(),
        outage_evidence: evidence,
        message: Some(failure),
    }
}

fn run_case_once(case_id: &str, deadline: Deadline) -> Result<(), &'static str> {
    let timeout_seconds = remaining_seconds(deadline).ok_or("live smoke hard deadline elapsed")?;
    let executable = std::env::current_exe().map_err(|_| "cannot resolve forager executable")?;
    let p2_evidence = (case_id == "P2")
        .then(tempfile::tempdir)
        .transpose()
        .map_err(|_| "cannot create temporary P2 evidence directory")?;
    let commands = command_specs(
        case_id,
        timeout_seconds,
        p2_evidence.as_ref().map(tempfile::TempDir::path),
    )
    .ok_or("registered live case has no execution mapping")?;
    for mut command in commands {
        if command.arguments.first().map(String::as_str) != Some("smoke") {
            command.arguments.push("--verbose".into());
        }
        let mut process = Command::new(&executable);
        process.args(&command.arguments);
        process.env("FORAGER_RETRY__MAX_ATTEMPTS", "1");
        for (name, value) in command.environment {
            process.env(name, value);
        }
        let output = execute_with_deadline(process, deadline)?;
        if output.status.code() != Some(0) {
            return Err("live case command returned a nonzero terminal");
        }
        if matches!(case_id, "P1" | "P2")
            && String::from_utf8_lossy(&output.stderr).contains("Classifier warning")
        {
            return Err("pipeline used classifier degradation instead of the live contract");
        }
        let payload: Value = serde_json::from_slice(&output.stdout)
            .map_err(|_| "live case returned invalid JSON")?;
        if !result_shape_is_nonempty(command.shape, &payload) {
            return Err("live case returned an empty or unexpected result shape");
        }
        if contains_runtime_error(&payload) {
            return Err("live case included a runtime-class provider attempt");
        }
    }
    Ok(())
}

fn execute_with_deadline(mut command: Command, deadline: Deadline) -> Result<Output, &'static str> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let child =
        Arc::new(SharedChild::spawn(&mut command).map_err(|_| "cannot execute live case command")?);
    let mut stdout = child
        .take_stdout()
        .ok_or("cannot capture live case stdout")?;
    let mut stderr = child
        .take_stderr()
        .ok_or("cannot capture live case stderr")?;
    let stdout_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    let stderr_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).map(|_| bytes)
    });
    let (status_sender, status_receiver) = mpsc::sync_channel(1);
    let waiting_child = Arc::clone(&child);
    thread::spawn(move || {
        let _ = status_sender.send(waiting_child.wait());
    });
    let remaining = deadline.remaining().unwrap_or_default();
    let status = match status_receiver.recv_timeout(remaining) {
        Ok(status) => status.map_err(|_| "cannot wait for live case command")?,
        Err(RecvTimeoutError::Timeout) => {
            let _ = child.kill();
            let _ = status_receiver.recv();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err("live smoke hard deadline elapsed");
        }
        Err(RecvTimeoutError::Disconnected) => return Err("cannot wait for live case command"),
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| "cannot collect live case stdout")?
        .map_err(|_| "cannot collect live case stdout")?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| "cannot collect live case stderr")?
        .map_err(|_| "cannot collect live case stderr")?;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn remaining_seconds(deadline: Deadline) -> Option<u64> {
    deadline.remaining().map(|remaining| {
        remaining
            .as_secs()
            .saturating_add(u64::from(remaining.subsec_nanos() > 0))
            .max(1)
    })
}

fn command_specs(
    case_id: &str,
    timeout_seconds: u64,
    p2_evidence_dir: Option<&Path>,
) -> Option<Vec<CommandSpec>> {
    let timeout = timeout_seconds.to_string();
    let one = |arguments: &[&str], shape| {
        vec![CommandSpec {
            arguments: arguments.iter().map(|value| (*value).into()).collect(),
            environment: Vec::new(),
            shape,
        }]
    };
    Some(match case_id {
        "P1" => one(
            &[
                "search",
                PIPELINE_CANARY_QUERY,
                "--extra-sources",
                "2",
                "--timeout",
                &timeout,
            ],
            ResultShape::PipelineSearch,
        ),
        "P2" => {
            let evidence_dir = p2_evidence_dir?.display().to_string();
            one(
                &[
                    "research",
                    RESEARCH_CANARY_QUERY,
                    "--budget",
                    "standard",
                    "--evidence-dir",
                    &evidence_dir,
                    "--timeout",
                    &timeout,
                ],
                ResultShape::Research,
            )
        }
        "C01" => with_environment(
            one(
                &[
                    "search",
                    MAIN_CANARY_QUERY,
                    "--capabilities",
                    "none",
                    "--timeout",
                    &timeout,
                ],
                ResultShape::MainSearch,
            ),
            &[
                ("FORAGER_SEARCH__BACKENDS", "[\"xai\"]"),
                ("FORAGER_SEARCH__FALLBACK", "off"),
            ],
        ),
        "C02" => with_environment(
            one(
                &[
                    "search",
                    MAIN_CANARY_QUERY,
                    "--capabilities",
                    "none",
                    "--timeout",
                    &timeout,
                ],
                ResultShape::MainSearch,
            ),
            &[
                ("FORAGER_SEARCH__BACKENDS", "[\"openai_compatible\"]"),
                ("FORAGER_SEARCH__FALLBACK", "off"),
                ("FORAGER_PROVIDERS__OPENAI_COMPATIBLE__STREAM", "false"),
            ],
        ),
        "C03" => with_environment(
            one(
                &[
                    "search",
                    MAIN_CANARY_QUERY,
                    "--capabilities",
                    "none",
                    "--timeout",
                    &timeout,
                ],
                ResultShape::MainSearch,
            ),
            &[
                ("FORAGER_SEARCH__BACKENDS", "[\"openai_compatible\"]"),
                ("FORAGER_SEARCH__FALLBACK", "off"),
                ("FORAGER_PROVIDERS__OPENAI_COMPATIBLE__STREAM", "true"),
            ],
        ),
        "C04" => one(
            &["smoke", "--probe", "C04", "--probe-timeout", &timeout],
            ResultShape::Classifier,
        ),
        "C05" => one(
            &["smoke", "--probe", "C05", "--probe-timeout", &timeout],
            ResultShape::SupplementalSearch,
        ),
        "C06" => one(
            &["smoke", "--probe", "C06", "--probe-timeout", &timeout],
            ResultShape::SupplementalSearch,
        ),
        "C07" | "C08" | "C09" => {
            let order = match case_id {
                "C07" => "[\"jina\"]",
                "C08" => "[\"tavily\"]",
                _ => "[\"firecrawl\"]",
            };
            with_environment(
                one(
                    &["fetch", FETCH_CANARY_URL, "--timeout", &timeout],
                    ResultShape::Fetch,
                ),
                &[("FORAGER_CAPABILITIES__WEB_FETCH__ORDER", order)],
            )
        }
        "C10" => one(
            &[
                "context7",
                "library",
                "tokio",
                "Tokio runtime",
                "--timeout",
                &timeout,
            ],
            ResultShape::Context7Library,
        ),
        "C11" => one(
            &[
                "context7",
                "docs",
                "/tokio-rs/tokio",
                "How do I spawn tasks?",
                "--timeout",
                &timeout,
            ],
            ResultShape::Context7Docs,
        ),
        "C12" => one(
            &[
                "exa",
                "search",
                "Rust async drop proposal",
                "--num-results",
                "3",
                "--timeout",
                &timeout,
            ],
            ResultShape::Exa,
        ),
        "C13" => one(
            &[
                "exa",
                "similar",
                FETCH_CANARY_URL,
                "--num-results",
                "3",
                "--timeout",
                &timeout,
            ],
            ResultShape::Exa,
        ),
        "C14" => one(
            &[
                "anysearch",
                "search",
                ANYSEARCH_CANARY_QUERY,
                "--domain",
                "academic",
                "--sub-domain",
                "search",
                "--max-results",
                "3",
                "--timeout",
                &timeout,
            ],
            ResultShape::Anysearch,
        ),
        "C15" => one(
            &[
                "anysearch",
                "search",
                ANYSEARCH_CANARY_QUERY,
                "--max-results",
                "3",
                "--timeout",
                &timeout,
            ],
            ResultShape::Anysearch,
        ),
        "C16" => one(
            &["anysearch", "domains", "academic", "--timeout", &timeout],
            ResultShape::Anysearch,
        ),
        "C17" => one(
            &[
                "map",
                FETCH_CANARY_URL,
                "--limit",
                "10",
                "--timeout",
                &timeout,
            ],
            ResultShape::Map,
        ),
        _ => return None,
    })
}

fn with_environment(
    mut commands: Vec<CommandSpec>,
    environment: &[(&'static str, &'static str)],
) -> Vec<CommandSpec> {
    for command in &mut commands {
        command.environment.extend_from_slice(environment);
    }
    commands
}

fn result_shape_is_nonempty(shape: ResultShape, payload: &Value) -> bool {
    match shape {
        ResultShape::MainSearch => nonempty_string(payload, "answer"),
        ResultShape::PipelineSearch => {
            nonempty_string(payload, "answer") && nonempty_array(payload, "extra_sources")
        }
        ResultShape::Research => {
            nonempty_string(payload, "final_answer")
                && nonempty_array(payload, "evidence_items")
                && nonempty_array(payload, "citations")
                && payload["plan_source"] == "classifier"
        }
        ResultShape::Classifier => {
            nonempty_array(payload, "capabilities")
                && payload["research_plan"]["decomposition"]
                    .as_array()
                    .is_some_and(|values| !values.is_empty())
        }
        ResultShape::SupplementalSearch => nonempty_array(payload, "results"),
        ResultShape::Fetch | ResultShape::Context7Docs => nonempty_string(payload, "content"),
        ResultShape::Context7Library
        | ResultShape::Exa
        | ResultShape::Anysearch
        | ResultShape::Map => nonempty_array(payload, "results"),
    }
}

fn nonempty_string(payload: &Value, field: &str) -> bool {
    payload[field]
        .as_str()
        .is_some_and(|value| !value.trim().is_empty())
}

fn nonempty_array(payload: &Value, field: &str) -> bool {
    payload[field]
        .as_array()
        .is_some_and(|values| !values.is_empty())
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn contains_runtime_error(value: &Value) -> bool {
    match value {
        Value::Object(object) => {
            object
                .get("error_kind")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind == "runtime")
                || object.values().any(contains_runtime_error)
        }
        Value::Array(values) => values.iter().any(contains_runtime_error),
        _ => false,
    }
}

fn case_is_configured(case_id: &str, runtime: &RuntimeConfig) -> bool {
    match case_id {
        "P1" => {
            runtime.classifier.configured()
                && runtime.main_search.configured_provider_count() > 0
                && runtime.web_search.configured_provider_count() > 0
        }
        "P2" => {
            let discovery_seams = [
                runtime.docs_search.configured_provider_count(),
                runtime.web_search.configured_provider_count(),
                runtime.vertical_search.configured_provider_count(),
            ]
            .into_iter()
            .filter(|count| *count > 0)
            .count();
            runtime.classifier.configured()
                && discovery_seams >= 2
                && runtime.web_fetch.configured_provider_count() > 0
        }
        "C01" => provider_is_configured(runtime, ProviderId::Xai),
        "C02" | "C03" => provider_is_configured(runtime, ProviderId::OpenAiCompatible),
        "C04" => runtime.classifier.configured(),
        "C05" | "C08" | "C17" => provider_is_configured(runtime, ProviderId::Tavily),
        "C06" | "C09" => provider_is_configured(runtime, ProviderId::Firecrawl),
        "C07" => provider_is_configured(runtime, ProviderId::Jina),
        "C10" | "C11" => provider_is_configured(runtime, ProviderId::Context7),
        "C12" | "C13" => provider_is_configured(runtime, ProviderId::Exa),
        "C14" | "C15" | "C16" => provider_is_configured(runtime, ProviderId::Anysearch),
        _ => false,
    }
}

fn provider_is_configured(runtime: &RuntimeConfig, provider: ProviderId) -> bool {
    match provider {
        ProviderId::Xai | ProviderId::OpenAiCompatible => runtime
            .main_search
            .provider(provider.name())
            .is_some_and(|config| !config.keys().is_empty()),
        ProviderId::Exa => !runtime.exa.keys.is_empty(),
        ProviderId::Tavily => !runtime.tavily.keys.is_empty(),
        ProviderId::Firecrawl | ProviderId::Jina => runtime
            .web_fetch
            .provider(provider.name())
            .is_some_and(|config| !config.keys.is_empty()),
        ProviderId::Context7 => !runtime.context7.keys.is_empty(),
        ProviderId::Anysearch => !runtime.anysearch.keys.is_empty(),
    }
}

pub(crate) fn run_offline() -> Result<(SmokeReport, u8), config::ConfigError> {
    let effective = serde_json::to_value(config::effective_view()?)
        .map_err(|error| config::ConfigError::Message(error.to_string()))?;
    let runtime = config::runtime_config()?;
    if runtime.main_search.configured_provider_count() == 0 {
        return Err(config::ConfigError::Message(
            "search.backends has no configured credentials".into(),
        ));
    }

    let registry = registry_status();
    let providers = providers::registrations()
        .iter()
        .map(|registration| credential_status(registration.id, &runtime, &effective))
        .collect();
    let classifier = CredentialStatus {
        provider: "classifier",
        configured: !runtime.classifier.keys.is_empty(),
        key_count: runtime.classifier.keys.len(),
        keys: vec![config::CREDENTIAL_MASK; runtime.classifier.keys.len()],
        source: credential_source(&effective, "classifier"),
    };
    let journal = directory_status(
        Some(runtime.journal.enabled),
        &runtime.journal.dir,
        &runtime.journal.credentials,
    );
    let credential_cursor = match credentials::state_directory() {
        Some(path) => directory_status(None, &path, &runtime.journal.credentials),
        None => DirectoryStatus {
            enabled: None,
            writable: false,
            message: Some("XDG_STATE_HOME and HOME do not resolve to an absolute path".into()),
        },
    };
    let permissions = permission_status()?;
    let ok = registry.ok && journal.writable && credential_cursor.writable && permissions.ok;
    Ok((
        SmokeReport {
            mode: "offline",
            ok,
            registry,
            providers,
            classifier,
            journal,
            credential_cursor,
            permissions,
        },
        if ok { 0 } else { 4 },
    ))
}

fn registry_status() -> RegistryStatus {
    let registrations = providers::registrations();
    let names = registrations
        .iter()
        .map(|registration| registration.name)
        .collect::<BTreeSet<_>>();
    let expected = EXPECTED_PROVIDERS.into_iter().collect::<BTreeSet<_>>();
    let descriptions_are_complete = registrations.iter().all(|registration| {
        registration.credentials_required
            && (!registration.capabilities.is_empty() || !registration.operations.is_empty())
    });
    RegistryStatus {
        ok: registrations.len() == EXPECTED_PROVIDERS.len()
            && names == expected
            && descriptions_are_complete,
        provider_count: registrations.len(),
    }
}

fn credential_status(
    id: ProviderId,
    runtime: &RuntimeConfig,
    effective: &Value,
) -> CredentialStatus {
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
    CredentialStatus {
        provider: id.name(),
        configured: key_count > 0,
        key_count,
        keys: vec![config::CREDENTIAL_MASK; key_count],
        source: effective["providers"][id.name()]["keys"]["source"]
            .as_str()
            .unwrap_or("default")
            .to_owned(),
    }
}

fn credential_source(effective: &Value, section: &str) -> String {
    effective[section]["keys"]["source"]
        .as_str()
        .unwrap_or("default")
        .to_owned()
}

fn directory_status(enabled: Option<bool>, path: &Path, credentials: &[String]) -> DirectoryStatus {
    match probe_private_directory(path) {
        Ok(()) => DirectoryStatus {
            enabled,
            writable: true,
            message: None,
        },
        Err(error) => DirectoryStatus {
            enabled,
            writable: false,
            message: Some(sanitize(&error.to_string(), credentials)),
        },
    }
}

fn probe_private_directory(path: &Path) -> io::Result<()> {
    config::ensure_private_directory(path)?;
    let sequence = PROBE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let probe = path.join(format!(
        ".forager-smoke-probe-{}-{nanos}-{sequence}",
        std::process::id()
    ));
    let result = config::create_private_file(&probe).and_then(|file| file.sync_all());
    let cleanup = fs::remove_file(&probe);
    result.and(cleanup)
}

fn permission_status() -> Result<PermissionStatus, config::ConfigError> {
    let config_file = config::ConfigLocation::discover()?.config_file();
    let mut issues = Vec::new();
    check_permissions(
        config_file
            .parent()
            .expect("configuration file has a parent"),
        0o700,
        "config directory",
        &mut issues,
    );
    if config_file.exists() {
        check_permissions(&config_file, 0o600, "config file", &mut issues);
    }
    Ok(PermissionStatus {
        ok: issues.is_empty(),
        issues,
    })
}

fn sanitize(message: &str, credentials: &[String]) -> String {
    config::redact_credentials(message, credentials)
}

fn check_permissions(path: &Path, expected: u32, label: &str, issues: &mut Vec<String>) {
    match config::has_private_permissions(path, expected) {
        Ok(true) => {}
        Ok(false) => issues.push(format!("{label} permissions are too broad")),
        Err(error) => issues.push(format!("{label} cannot be inspected: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::{
        execute_with_deadline, official_status_host, status_host_matches_endpoint,
        status_page_reports_outage,
    };
    use crate::types::Deadline;

    #[test]
    fn deadline_terminates_the_child_process() {
        let root = tempfile::tempdir().expect("create marker root");
        let marker = root.path().join("child-completed");
        let mut command = Command::new(std::env::current_exe().expect("resolve test executable"));
        command
            .args([
                "--exact",
                "smoke::tests::deadline_child_writes_marker",
                "--nocapture",
            ])
            .env("FORAGER_SMOKE_DEADLINE_MARKER", &marker);

        let started = Instant::now();
        let result = execute_with_deadline(command, Deadline::new(Duration::from_millis(50)));
        let elapsed = started.elapsed();
        thread::sleep(Duration::from_millis(1_100));

        assert_eq!(result.unwrap_err(), "live smoke hard deadline elapsed");
        assert!(elapsed < Duration::from_millis(500));
        assert!(!marker.exists(), "timed-out child kept running");
    }

    #[test]
    fn deadline_child_writes_marker() {
        let Some(marker) = std::env::var_os("FORAGER_SMOKE_DEADLINE_MARKER") else {
            return;
        };
        thread::sleep(Duration::from_secs(1));
        fs::write(marker, "completed").expect("write completion marker");
    }

    #[test]
    fn official_status_evidence_is_bound_to_the_case_provider() {
        assert_eq!(
            official_status_host("https://status.x.ai/"),
            Some("status.x.ai")
        );
        assert!(status_host_matches_endpoint(
            "status.x.ai",
            "https://api.x.ai/v1"
        ));
        assert!(!status_host_matches_endpoint(
            "status.openai.com",
            "https://api.x.ai/v1"
        ));
        assert_eq!(official_status_host("https://example.com/status"), None);
        assert_eq!(official_status_host("http://status.x.ai/"), None);
    }

    #[test]
    fn official_status_evidence_requires_an_active_outage_marker() {
        assert!(status_page_reports_outage(
            r#"{"status":{"indicator":"major"},"incidents":[]}"#
        ));
        assert!(!status_page_reports_outage(
            r#"{"status":{"indicator":"none"},"incidents":[{"status":"resolved"}]}"#
        ));
        assert!(!status_page_reports_outage(
            "All systems operational. Past incidents: Major outage"
        ));
        assert!(!status_page_reports_outage(
            "Resolved: Major outage affected the API"
        ));
    }
}
