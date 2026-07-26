use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;

use crate::config::{self, RuntimeConfig};
use crate::credentials;
use crate::providers::{self, ProviderId};

static PROBE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
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

pub(crate) fn run() -> Result<(SmokeReport, u8), config::ConfigError> {
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
