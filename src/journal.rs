use std::fs;
use std::io::{self, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::config::JournalRuntimeConfig;
use crate::net::duration_millis;
use crate::providers::ProviderError;
use crate::redact::{CREDENTIAL_MASK, Secret, redact_urls};
use crate::secure_fs::{create_private_file, ensure_private_directory};
use crate::types::{Capability, JournalOutcome, ResearchError, ResearchOutcome, SearchOutcome};

static RECORD_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
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

#[derive(Clone, Copy)]
pub(crate) struct ResearchRecord<'a> {
    pub(crate) query: &'a str,
    pub(crate) budget: Duration,
    pub(crate) elapsed: Duration,
    pub(crate) capabilities: &'a [Capability],
    pub(crate) plan_source: &'static str,
    pub(crate) classifier_degraded: bool,
    pub(crate) classifier_duration: Option<Duration>,
    pub(crate) result: &'a Result<ResearchOutcome, ResearchError>,
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
        Ok(written) => JournalOutcome {
            status: "written",
            reference: Some(sanitize_text(config, &written.reference)),
            warning: written
                .cleanup_warning
                .map(|warning| sanitize_warning(config, &warning)),
        },
        Err(error) => JournalOutcome {
            status: "failed",
            reference: None,
            warning: Some(sanitize_warning(config, &error.to_string())),
        },
    }
}

pub(crate) fn record_research(
    config: &JournalRuntimeConfig,
    record: ResearchRecord<'_>,
) -> JournalOutcome {
    if !config.enabled {
        return JournalOutcome {
            status: "disabled",
            reference: None,
            warning: None,
        };
    }
    match write_value(config, build_research_record(record)) {
        Ok(written) => JournalOutcome {
            status: "written",
            reference: Some(sanitize_text(config, &written.reference)),
            warning: written
                .cleanup_warning
                .map(|warning| sanitize_warning(config, &warning)),
        },
        Err(error) => JournalOutcome {
            status: "failed",
            reference: None,
            warning: Some(sanitize_warning(config, &error.to_string())),
        },
    }
}

struct WrittenRecord {
    reference: String,
    cleanup_warning: Option<String>,
}

fn write_record(
    config: &JournalRuntimeConfig,
    record: SearchRecord<'_>,
) -> io::Result<WrittenRecord> {
    write_value(config, build_record(record))
}

fn write_value(config: &JournalRuntimeConfig, mut value: Value) -> io::Result<WrittenRecord> {
    ensure_private_directory(&config.dir)?;
    let path = config.dir.join(record_name());
    if path.try_exists()? {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("journal record already exists: {}", path.display()),
        ));
    }
    sanitize_value(&mut value, &config.credentials);
    let encoded = serde_json::to_vec(&value).map_err(io::Error::other)?;
    let mut file = create_private_file(&path)?;
    file.write_all(&encoded)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(WrittenRecord {
        reference: path.display().to_string(),
        cleanup_warning: cleanup_expired(config),
    })
}

fn build_research_record(record: ResearchRecord<'_>) -> Value {
    let recorded_at_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let (result_surface, attempts, terminal_attribution, capability_gaps) = match record.result {
        Ok(outcome) => (
            json!({
                "status": "ok",
                "query": record.query,
                "evidence_items": outcome.evidence_items,
                "capability_gaps": outcome.capability_gaps,
                "gap_check": outcome.gap_check,
                "evidence_dir": outcome.evidence_dir,
                "plan_path": outcome.plan_path,
                "unconsumed_candidates": outcome.unconsumed_candidates,
                "synthesis_policy": outcome.synthesis_policy,
            }),
            &outcome.attempts,
            "ok",
            &outcome.capability_gaps,
        ),
        Err(error) => (
            json!({
                "status": "error",
                "query": record.query,
                "error_kind": error.kind.as_str(),
                "message": error.message,
                "evidence_items": error.evidence_items,
                "capability_gaps": error.capability_gaps,
                "gap_check": error.gap_check,
                "evidence_dir": error.evidence_dir,
                "plan_path": error.plan_path,
                "unconsumed_candidates": error.unconsumed_candidates,
                "synthesis_policy": error.synthesis_policy,
            }),
            &error.attempts,
            error.kind.as_str(),
            &error.capability_gaps,
        ),
    };
    json!({
        "schema_version": 1,
        "recorded_at_unix_ms": recorded_at_unix_ms,
        "result": result_surface,
        "execution": {
            "plan_summary": {
                "source": record.plan_source,
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
                .map_or(Value::Null, Value::from),
            "capability_gaps": capability_gaps
        }
    })
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
                "extra_sources": outcome.extra_sources,
                "vertical_results": outcome.vertical_results,
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
                .map_or(Value::Null, Value::from),
            "capability_gaps": record.result
                .as_ref()
                .map(|outcome| &outcome.capability_gaps)
                .ok()
        }
    })
}

fn cleanup_expired(config: &JournalRuntimeConfig) -> Option<String> {
    if config.retention_days == 0 {
        return None;
    }
    let retention = Duration::from_secs(config.retention_days.saturating_mul(86_400));
    let cutoff = SystemTime::now()
        .checked_sub(retention)
        .unwrap_or(UNIX_EPOCH);
    let entries = match fs::read_dir(&config.dir) {
        Ok(entries) => entries,
        Err(error) => return cleanup_warning(&error),
    };
    let mut warnings = Vec::new();
    for entry in entries {
        if let Err(error) = entry.and_then(|entry| cleanup_entry(&entry, cutoff))
            && let Some(warning) = cleanup_warning(&error)
        {
            warnings.push(warning);
        }
    }
    (!warnings.is_empty()).then(|| warnings.join("; "))
}

fn cleanup_entry(entry: &fs::DirEntry, cutoff: SystemTime) -> io::Result<()> {
    let path = entry.path();
    if path.extension().is_none_or(|extension| extension != "json") || !entry.file_type()?.is_file()
    {
        return Ok(());
    }
    if entry.metadata()?.modified()? < cutoff {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn cleanup_warning(error: &io::Error) -> Option<String> {
    match error.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied => None,
        _ => Some(format!("journal cleanup: {error}")),
    }
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

fn sanitize_value(value: &mut Value, credentials: &[Secret]) {
    match value {
        Value::String(text) => {
            *text = redact_journal_text(text, credentials);
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
    redact_journal_text(value, &config.credentials)
}

fn redact_journal_text(value: &str, credentials: &[Secret]) -> String {
    credentials
        .iter()
        .fold(redact_urls(value), |redacted, credential| {
            let credential = credential.expose();
            if redacted.contains(credential) {
                redacted.replace(credential, CREDENTIAL_MASK)
            } else {
                redacted
            }
        })
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn write_value_persists_json_that_can_be_read_back() {
        let root = tempdir().expect("create temporary directory");
        let config = journal_config(root.path().join("journal"));
        let expected = json!({"result": {"status": "ok"}});

        let written = write_value(&config, expected.clone()).expect("write journal record");
        let persisted: Value =
            serde_json::from_slice(&fs::read(&written.reference).expect("read journal record"))
                .expect("parse journal record");

        assert_eq!(persisted, expected);
    }

    #[test]
    fn cleanup_expired_silences_missing_directory() {
        let root = tempdir().expect("create temporary directory");
        let config = journal_config(root.path().join("missing"));

        let warning = cleanup_expired(&config);

        assert!(warning.is_none(), "unexpected cleanup warning: {warning:?}");
    }

    #[test]
    fn cleanup_expired_removes_json_older_than_retention_limit() {
        let root = tempdir().expect("create temporary directory");
        let journal_path = root.path().join("journal");
        fs::create_dir(&journal_path).expect("create journal directory");
        let stale_record = journal_path.join("stale.json");
        fs::write(&stale_record, "{}").expect("create stale journal record");
        let stale_time = SystemTime::now()
            .checked_sub(Duration::from_hours(48))
            .expect("calculate stale modification time");
        fs::File::open(&stale_record)
            .expect("open stale journal record")
            .set_times(fs::FileTimes::new().set_modified(stale_time))
            .expect("set stale modification time");
        let config = journal_config(journal_path);

        let warning = cleanup_expired(&config);

        assert!(
            !stale_record.try_exists().expect("check stale record"),
            "stale journal record was not removed; cleanup warning: {warning:?}"
        );
    }

    #[test]
    fn cleanup_expired_tolerates_malformed_json_within_retention_limit() {
        let root = tempdir().expect("create temporary directory");
        let journal_path = root.path().join("journal");
        fs::create_dir(&journal_path).expect("create journal directory");
        let malformed_record = journal_path.join("malformed.json");
        fs::write(&malformed_record, "not json").expect("create malformed journal record");
        let config = journal_config(journal_path);

        let warning = cleanup_expired(&config);

        assert!(
            malformed_record
                .try_exists()
                .expect("check malformed record"),
            "unexpired malformed journal record was removed; cleanup warning: {warning:?}"
        );
    }

    #[test]
    fn cleanup_expired_reports_unexpected_scan_error() {
        let root = tempdir().expect("create temporary directory");
        let journal_path = root.path().join("journal");
        fs::write(&journal_path, "not a directory").expect("create blocking file");
        let config = journal_config(journal_path);

        let warning = cleanup_expired(&config);

        assert!(warning.is_some(), "expected cleanup warning");
    }

    #[test]
    fn cleanup_warning_silences_permission_denied() {
        let error = io::Error::from(io::ErrorKind::PermissionDenied);

        let warning = cleanup_warning(&error);

        assert!(warning.is_none(), "unexpected cleanup warning: {warning:?}");
    }

    fn journal_config(dir: std::path::PathBuf) -> JournalRuntimeConfig {
        JournalRuntimeConfig {
            enabled: true,
            dir,
            retention_days: 1,
            credentials: Vec::new(),
        }
    }
}
