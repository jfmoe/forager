use std::io;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{TimeDelta, Utc};
use serde_json::{Value, json};

use crate::config::JournalRuntimeConfig;
use crate::net::duration_millis;
use crate::record_directory::{RecordDirectory, RecordKind};
use crate::redact::{Secret, redact_credentials};
use crate::search_fanout::SearchFailure;
use crate::types::{Capability, JournalOutcome, SearchOutcome};

#[derive(Clone, Copy)]
pub(crate) struct SearchRecord<'a> {
    pub(crate) query: &'a str,
    pub(crate) budget: Duration,
    pub(crate) elapsed: Duration,
    pub(crate) capabilities: &'a [Capability],
    pub(crate) plan_source: PlanSource,
    pub(crate) result: &'a Result<SearchOutcome, Box<SearchFailure>>,
}

/// Who chose the capabilities a search ran.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PlanSource {
    /// The caller declared them.
    Caller,
    /// The caller declared none, so the search ran main search only.
    DefaultMainSearchOnly,
}

impl PlanSource {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Caller => "caller",
            Self::DefaultMainSearchOnly => "default_main_search_only",
        }
    }
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

fn write_value(config: &JournalRuntimeConfig, mut value: Value) -> io::Result<WrittenRecord> {
    sanitize_record(&mut value, &config.credentials);
    let records = RecordDirectory::new(
        config.dir.clone(),
        RecordKind {
            prefix: "search_result_",
            retention: retention(config.retention_days),
            synced: true,
        },
    );
    let now = Utc::now();
    let path = records.write(now, &value)?;
    Ok(WrittenRecord {
        reference: path.display().to_string(),
        cleanup_warning: records
            .remove_expired_when_due(now)
            .err()
            .and_then(|error| cleanup_warning(&error)),
    })
}

/// How long a record is kept for `days` of configured retention, where zero keeps it
/// indefinitely, as does a retention too long to reach.
fn retention(days: u64) -> Option<TimeDelta> {
    (days > 0)
        .then(|| i64::try_from(days).ok().and_then(TimeDelta::try_days))
        .flatten()
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
            },
            "provider_attempts": [],
            "terminal_attribution": record.error_kind,
            "deadline_budget": {
                "total_ms": duration_millis(record.budget),
                "consumed_ms": 0,
                "exhausted": false,
            },
            "capability_gaps": Value::Null,
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
                "provider": outcome.provider,
                "answer": outcome.answer,
                "sources": outcome.sources,
                "extra_sources": outcome.extra_sources,
                "capabilities": outcome.capabilities,
                "capability_gaps": outcome.capability_gaps,
            }),
            &outcome.attempts,
            "ok",
        ),
        Err(failure) => (
            json!({
                "status": "error",
                "query": record.query,
                "error_kind": failure.error.kind.as_str(),
                "message": failure.error.message,
                "extra_sources": failure.extra_sources,
                "capability_gaps": failure.capability_gaps,
            }),
            &failure.error.attempts,
            failure.error.kind.as_str(),
        ),
    };
    json!({
        "schema_version": 1,
        "recorded_at_unix_ms": recorded_at_unix_ms,
        "result": result_surface,
        "execution": {
            "plan_summary": {
                "source": record.plan_source.as_str(),
                "capabilities": record.capabilities,
            },
            "provider_attempts": attempts,
            "terminal_attribution": terminal_attribution,
            "deadline_budget": {
                "total_ms": duration_millis(record.budget),
                "consumed_ms": duration_millis(record.elapsed),
                "exhausted": record.elapsed >= record.budget
            },
            "capability_gaps": match record.result {
                Ok(outcome) => &outcome.capability_gaps,
                Err(failure) => &failure.capability_gaps,
            }
        }
    })
}

fn cleanup_warning(error: &io::Error) -> Option<String> {
    match error.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied => None,
        _ => Some(format!("journal cleanup: {error}")),
    }
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
#[path = "journal_tests.rs"]
mod tests;
