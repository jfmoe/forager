use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::config::{self, JournalRuntimeConfig};
use crate::providers::ProviderError;
use crate::types::{Capability, JournalOutcome, SearchOutcome};

static RECORD_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) struct SearchRecord<'a> {
    pub(crate) query: &'a str,
    pub(crate) budget: Duration,
    pub(crate) elapsed: Duration,
    pub(crate) model: &'a str,
    pub(crate) endpoint_host: &'a str,
    pub(crate) capabilities: &'a [Capability],
    pub(crate) decision_source: &'static str,
    pub(crate) classifier_degraded: bool,
    pub(crate) classifier_duration: Option<Duration>,
    pub(crate) result: &'a Result<SearchOutcome, ProviderError>,
}

pub(crate) fn record_search(
    config: &JournalRuntimeConfig,
    record: SearchRecord<'_>,
) -> JournalOutcome {
    if !config.enabled {
        return JournalOutcome {
            status: "disabled",
            reference: None,
            warning: None,
        };
    }
    match write_record(config, record) {
        Ok(reference) => JournalOutcome {
            status: "written",
            reference: Some(sanitize_text(config, &reference)),
            warning: None,
        },
        Err(error) => JournalOutcome {
            status: "failed",
            reference: None,
            warning: Some(sanitize_warning(config, &error.to_string())),
        },
    }
}

fn write_record(config: &JournalRuntimeConfig, record: SearchRecord<'_>) -> io::Result<String> {
    fs::create_dir_all(&config.dir)?;
    restrict_directory(&config.dir)?;
    cleanup_expired(config)?;
    let path = config.dir.join(record_name());
    let mut value = build_record(record);
    sanitize_value(&mut value, &config.credentials);
    let encoded = serde_json::to_vec(&value).map_err(io::Error::other)?;
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path)?;
    file.write_all(&encoded)?;
    file.write_all(b"\n")?;
    restrict_file(&path)?;
    Ok(path.display().to_string())
}

fn build_record(record: SearchRecord<'_>) -> Value {
    let recorded_at_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let (result_surface, raw_attempts, terminal_attribution) = match record.result {
        Ok(outcome) => (
            json!({
                "status": "ok",
                "query": record.query,
                "answer": outcome.answer,
                "sources": outcome.sources,
                "capabilities": outcome.capabilities,
                "capability_gaps": outcome.capability_gaps,
            }),
            &outcome.attempts,
            "ok",
        ),
        Err(error) => (
            json!({
                "status": "error",
                "query": record.query,
                "error_kind": error.kind.as_str(),
                "message": error.message,
            }),
            &error.attempts,
            error.kind.as_str(),
        ),
    };
    let mut attempts = json!(raw_attempts);
    for attempt in attempts.as_array_mut().into_iter().flatten() {
        if let Some(attempt) = attempt.as_object_mut() {
            attempt
                .entry("model")
                .or_insert_with(|| Value::String(record.model.into()));
            attempt
                .entry("endpoint_host")
                .or_insert_with(|| Value::String(record.endpoint_host.into()));
        }
    }
    json!({
        "schema_version": 1,
        "recorded_at_unix_ms": recorded_at_unix_ms,
        "result": result_surface,
        "execution": {
            "plan_summary": {
                "source": record.decision_source,
                "capabilities": record.capabilities,
                "classifier_degraded": record.classifier_degraded
            },
            "provider_attempts": attempts,
            "terminal_attribution": terminal_attribution,
            "deadline_budget": {
                "total_ms": duration_millis(record.budget),
                "consumed_ms": duration_millis(record.elapsed),
                "exhausted": record.elapsed >= record.budget
            },
            "classifier_duration_ms": record.classifier_duration
                .map(duration_millis)
                .map(Value::from)
                .unwrap_or(Value::Null),
            "capability_gaps": record.result
                .as_ref()
                .map(|outcome| &outcome.capability_gaps)
                .ok()
        }
    })
}

fn cleanup_expired(config: &JournalRuntimeConfig) -> io::Result<()> {
    if config.retention_days == 0 {
        return Ok(());
    }
    let retention = Duration::from_secs(config.retention_days.saturating_mul(86_400));
    let cutoff = SystemTime::now()
        .checked_sub(retention)
        .unwrap_or(UNIX_EPOCH);
    for entry in fs::read_dir(&config.dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "json")
            || !entry.file_type()?.is_file()
        {
            continue;
        }
        if entry
            .metadata()?
            .modified()
            .is_ok_and(|modified| modified < cutoff)
        {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

fn record_name() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = RECORD_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!(
        "search_result_{nanos}_{}_{}.json",
        std::process::id(),
        sequence
    )
}

fn sanitize_value(value: &mut Value, credentials: &[String]) {
    match value {
        Value::String(text) => {
            *text = credentials
                .iter()
                .fold(config::redact_urls(text), |redacted, credential| {
                    redacted.replace(credential, "********")
                });
        }
        Value::Array(values) => {
            for value in values {
                sanitize_value(value, credentials);
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                sanitize_value(value, credentials);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn sanitize_warning(config: &JournalRuntimeConfig, message: &str) -> String {
    sanitize_text(config, message).chars().take(240).collect()
}

fn sanitize_text(config: &JournalRuntimeConfig, value: &str) -> String {
    config
        .credentials
        .iter()
        .fold(config::redact_urls(value), |redacted, credential| {
            redacted.replace(credential, "********")
        })
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

#[cfg(unix)]
fn restrict_directory(path: &std::path::Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn restrict_directory(_path: &std::path::Path) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn restrict_file(path: &std::path::Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict_file(_path: &std::path::Path) -> io::Result<()> {
    Ok(())
}
