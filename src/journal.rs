use std::fs;
use std::io::{self, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::config::JournalRuntimeConfig;
use crate::net::duration_millis;
use crate::providers::ProviderError;
use crate::redact::{Secret, redact_credentials};
use crate::secure_fs::{create_new_private_file, ensure_private_directory};
use crate::types::{Capability, JournalOutcome, ResearchError, ResearchOutcome, SearchOutcome};

static RECORD_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
pub(crate) struct SearchRecord<'a> {
    pub(crate) query: &'a str,
    pub(crate) budget: Duration,
    pub(crate) elapsed: Duration,
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

#[derive(Clone, Copy)]
pub(crate) struct SearchPreflightRecord<'a> {
    pub(crate) query: &'a str,
    pub(crate) budget: Duration,
    pub(crate) error_kind: &'static str,
    pub(crate) message: &'a str,
}

pub(crate) fn record_search(
    config: &JournalRuntimeConfig,
    record: SearchRecord<'_>,
) -> JournalOutcome {
    record_value(config, || build_record(record))
}

pub(crate) fn record_research(
    config: &JournalRuntimeConfig,
    record: ResearchRecord<'_>,
) -> JournalOutcome {
    record_value(config, || build_research_record(record))
}

pub(crate) fn record_search_preflight(
    config: &JournalRuntimeConfig,
    record: SearchPreflightRecord<'_>,
) -> JournalOutcome {
    record_value(config, || build_search_preflight_record(record))
}

fn record_value(config: &JournalRuntimeConfig, build: impl FnOnce() -> Value) -> JournalOutcome {
    if !config.enabled {
        return JournalOutcome {
            status: "disabled",
            reference: None,
            warning: None,
        };
    }
    match write_value(config, build()) {
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

fn write_value(config: &JournalRuntimeConfig, value: Value) -> io::Result<WrittenRecord> {
    ensure_private_directory(&config.dir)?;
    let path = config.dir.join(record_name());
    write_value_to(config, &path, value)
}

fn write_value_to(
    config: &JournalRuntimeConfig,
    path: &std::path::Path,
    mut value: Value,
) -> io::Result<WrittenRecord> {
    sanitize_record(&mut value, &config.credentials);
    let encoded = serde_json::to_vec(&value).map_err(io::Error::other)?;
    let mut file = create_new_private_file(path)?;
    file.write_all(&encoded)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(WrittenRecord {
        reference: path.display().to_string(),
        cleanup_warning: cleanup_expired(config),
    })
}

fn build_search_preflight_record(record: SearchPreflightRecord<'_>) -> Value {
    let recorded_at_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    json!({
        "schema_version": 1,
        "recorded_at_unix_ms": recorded_at_unix_ms,
        "result": {
            "status": "error",
            "query": record.query,
            "error_kind": record.error_kind,
            "message": record.message,
        },
        "execution": {
            "plan_summary": {
                "source": "preflight",
                "capabilities": [],
                "classifier_degraded": false,
            },
            "provider_attempts": [],
            "terminal_attribution": record.error_kind,
            "deadline_budget": {
                "total_ms": duration_millis(record.budget),
                "consumed_ms": 0,
                "exhausted": false,
            },
            "classifier_duration_ms": Value::Null,
            "capability_gaps": Value::Null,
        }
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
                "summary_path": error.summary_path,
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
    let (result_surface, attempts, terminal_attribution) = match record.result {
        Ok(outcome) => (
            json!({
                "status": "ok",
                "query": record.query,
                "answer": outcome.answer,
                "sources": outcome.sources,
                "extra_sources": outcome.extra_sources,
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
    if !is_owned_record_name(&entry.file_name()) || !entry.file_type()?.is_file() {
        return Ok(());
    }
    if entry.metadata()?.modified()? < cutoff {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn is_owned_record_name(name: &std::ffi::OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    let Some(fields) = name
        .strip_prefix("search_result_")
        .and_then(|name| name.strip_suffix(".json"))
    else {
        return false;
    };
    let mut fields = fields.split('_');
    let (Some(nanos), Some(pid), Some(sequence), None) =
        (fields.next(), fields.next(), fields.next(), fields.next())
    else {
        return false;
    };
    [nanos, pid, sequence]
        .into_iter()
        .all(|field| !field.is_empty() && field.bytes().all(|byte| byte.is_ascii_digit()))
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
            *text = redact_credentials(text, credentials);
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

fn sanitize_record(value: &mut Value, credentials: &[Secret]) {
    let answer = value
        .get_mut("result")
        .and_then(Value::as_object_mut)
        .and_then(|result| result.remove("answer"));
    sanitize_value(value, credentials);
    if let Some(answer) = answer
        && let Some(result) = value.get_mut("result").and_then(Value::as_object_mut)
    {
        result.insert("answer".into(), answer);
    }
}

fn sanitize_warning(config: &JournalRuntimeConfig, message: &str) -> String {
    sanitize_text(config, message).chars().take(240).collect()
}

fn sanitize_text(config: &JournalRuntimeConfig, value: &str) -> String {
    redact_credentials(value, &config.credentials)
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
    fn write_value_does_not_replace_an_existing_record() {
        let root = tempdir().expect("create temporary directory");
        let config = journal_config(root.path().join("journal"));
        fs::create_dir(&config.dir).expect("create journal directory");
        let path = config.dir.join("search_result_1_2_3.json");
        fs::write(&path, "existing record\n").expect("create existing record");

        let result = write_value_to(&config, &path, json!({"replacement": true}));

        let Err(error) = result else {
            panic!("existing record must reject replacement");
        };
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            fs::read_to_string(path).expect("read existing record"),
            "existing record\n"
        );
    }

    #[test]
    fn cleanup_expired_silences_missing_directory() {
        let root = tempdir().expect("create temporary directory");
        let config = journal_config(root.path().join("missing"));

        let warning = cleanup_expired(&config);

        assert!(warning.is_none(), "unexpected cleanup warning: {warning:?}");
    }

    #[test]
    fn cleanup_expired_only_removes_owned_regular_files_past_retention() {
        let root = tempdir().expect("create temporary directory");
        let journal_path = root.path().join("journal");
        fs::create_dir(&journal_path).expect("create journal directory");
        let stale_time = SystemTime::now()
            .checked_sub(Duration::from_hours(48))
            .expect("calculate stale modification time");
        let cases = [
            ("search_result_1_2_3.json", true, true),
            ("search_result_4_5_6.json", false, false),
            ("search_result__2_3.json", true, false),
            ("search_result_1__3.json", true, false),
            ("search_result_1_2_.json", true, false),
            ("search_result_one_2_3.json", true, false),
            ("search_result_1_two_3.json", true, false),
            ("search_result_1_2_three.json", true, false),
            ("prefix_search_result_1_2_3.json", true, false),
            ("search_result_1_2_3.json.bak", true, false),
            ("stale.json", true, false),
            ("search_results_20260809.jsonl", true, false),
        ];
        for (name, stale, _) in cases {
            let path = journal_path.join(name);
            fs::write(&path, "{}").expect("create retention case");
            if stale {
                fs::File::open(&path)
                    .expect("open retention case")
                    .set_times(fs::FileTimes::new().set_modified(stale_time))
                    .expect("set retention case modification time");
            }
        }
        let owned_directory = journal_path.join("search_result_7_8_9.json");
        fs::create_dir(&owned_directory).expect("create owned-name directory");
        #[cfg(unix)]
        let owned_symlink = {
            let path = journal_path.join("search_result_10_11_12.json");
            std::os::unix::fs::symlink(journal_path.join("stale.json"), &path)
                .expect("create owned-name symlink");
            path
        };
        let config = journal_config(journal_path.clone());

        let warning = cleanup_expired(&config);

        assert!(warning.is_none(), "unexpected cleanup warning: {warning:?}");
        for (name, _, removed) in cases {
            assert_eq!(
                journal_path.join(name).exists(),
                !removed,
                "retention result for {name}"
            );
        }
        assert!(owned_directory.is_dir());
        #[cfg(unix)]
        assert!(owned_symlink.symlink_metadata().is_ok());
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
