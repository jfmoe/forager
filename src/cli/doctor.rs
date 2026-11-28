//! The `doctor` command's reports and how each one is rendered.

use std::fmt::Write as _;

use serde_json::Value;

use super::args::OutputFormat;
use super::dispatch::{AppError, CommandOutput};
use crate::health::{SUBJECT_FIELDS, label};
use crate::providers::ProviderId;

/// The default deadline of `doctor`, and of a whole `doctor --live` run.
pub(super) const DOCTOR_TIMEOUT_SECONDS: u64 = 30;
pub(super) const LIVE_TIMEOUT_SECONDS: u64 = 420;

/// Whether the `CI` variable says the process runs in CI: set, non-empty and not `false`.
pub(super) fn ci_environment(value: Option<std::ffi::OsString>) -> bool {
    value.is_some_and(|value| !value.is_empty() && value != "false")
}

/// Runs the report on one provider, or the all-provider report in the shallow or the offline
/// mode.
pub(super) fn run_report(
    provider: Option<ProviderId>,
    offline: bool,
    timeout_seconds: u64,
    format: OutputFormat,
) -> Result<CommandOutput, AppError> {
    let (stdout, exit_code) = if let Some(provider) = provider {
        let (report, exit_code) = crate::doctor::one_provider(provider, timeout_seconds)?;
        let stdout = match format {
            OutputFormat::Json => serde_json::to_string(&report)
                .map_err(|error| AppError::Runtime(error.to_string()))?,
            OutputFormat::Markdown => render_provider_doctor_markdown(&report)?,
        };
        (stdout, exit_code)
    } else {
        let (report, exit_code) = if offline {
            crate::doctor::offline()?
        } else {
            crate::doctor::shallow(timeout_seconds)?
        };
        let stdout = match format {
            OutputFormat::Json => serde_json::to_string(&report)
                .map_err(|error| AppError::Runtime(error.to_string()))?,
            OutputFormat::Markdown => render_doctor_markdown(&report)?,
        };
        (stdout, exit_code)
    };
    Ok(CommandOutput::Text {
        stdout,
        stderr: None,
        exit_code,
    })
}

/// Runs the health summary over the `days` before now.
pub(super) fn run_health(days: u32, format: OutputFormat) -> Result<CommandOutput, AppError> {
    let (report, exit_code) = crate::doctor::health(days)?;
    let value =
        serde_json::to_value(report).map_err(|error| AppError::Runtime(error.to_string()))?;
    let stdout = match format {
        OutputFormat::Json => value.to_string(),
        OutputFormat::Markdown => health_markdown(&value),
    };
    Ok(CommandOutput::Text {
        stdout,
        stderr: None,
        exit_code,
    })
}

/// The findings first, then one line per provider and command, then one per schedule.
fn health_markdown(value: &Value) -> String {
    let mut output = format!(
        "# forager doctor --health\n\nok: {}\ndays: {}, since {}\ntelemetry_enabled: {}\n",
        value["ok"].as_bool().unwrap_or(false),
        value["days"],
        scalar(&value["since"]),
        value["telemetry_enabled"].as_bool().unwrap_or(false),
    );
    output.push_str("\n## Findings\n\n");
    for finding in value["findings"].as_array().into_iter().flatten() {
        let _ = write!(
            output,
            "- {}: {}",
            label(finding),
            scalar(&finding["finding"])
        );
        for (field, detail) in finding.as_object().into_iter().flatten() {
            if !SUBJECT_FIELDS.contains(&field.as_str()) && field != "finding" {
                let _ = write!(output, ", {field}={}", scalar(detail));
            }
        }
        output.push('\n');
    }
    output.push_str("\n## Providers\n\n");
    for row in value["providers"].as_array().into_iter().flatten() {
        let _ = write!(output, "- {}:", label(row));
        let mut separator = " ";
        for field in [
            "calls",
            "previous_calls",
            "success_rate",
            "hidden_failures",
            "empty_rate",
            "short_content_rate",
        ] {
            if !row[field].is_null() {
                let _ = write!(output, "{separator}{field}={}", scalar(&row[field]));
                separator = ", ";
            }
        }
        for percentile in ["p50", "p95"] {
            let _ = write!(
                output,
                ", {percentile}={}ms",
                row["duration_ms"][percentile]
            );
        }
        let failure = &row["last_failure"];
        if !failure.is_null() {
            let _ = write!(
                output,
                ", last_failure={}",
                failure["error_kind"].as_str().unwrap_or("unknown")
            );
            if let Some(status) = failure["http_status"].as_u64() {
                let _ = write!(output, " HTTP {status}");
            }
            let _ = write!(output, " at {}", scalar(&failure["at"]));
        }
        output.push('\n');
    }
    output.push_str("\n## Schedules\n\n");
    for schedule in value["schedules"].as_array().into_iter().flatten() {
        let _ = write!(
            output,
            "- {}: {}",
            label(schedule),
            scalar(&schedule["status"])
        );
        if let Some(last) = schedule["last_run_at"].as_str() {
            let _ = write!(
                output,
                ", last run {last}, {} hours ago",
                schedule["age_hours"]
            );
        }
        if let Some(days) = schedule["interval_days"].as_u64()
            && !schedule["last_run_at"].is_null()
        {
            let _ = write!(output, ", every {days} days");
        }
        output.push('\n');
    }
    output
}

/// A JSON scalar as text: a string without its quotes.
fn scalar(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

pub(super) fn run_live(
    selection: &crate::live::LiveSelection,
    list: bool,
    timeout_seconds: u64,
    format: OutputFormat,
) -> Result<CommandOutput, AppError> {
    let live_error = |error| match error {
        crate::live::LiveError::Refused(message) => AppError::Argument(message),
        crate::live::LiveError::Config(error) => AppError::Config(error),
        crate::live::LiveError::Executable(error) => AppError::Runtime(error.to_string()),
    };
    let (value, exit_code) = if list {
        let listing = crate::live::list(selection).map_err(live_error)?;
        (serde_json::to_value(listing), 0)
    } else {
        let (report, exit_code) =
            crate::live::run(selection, timeout_seconds).map_err(live_error)?;
        (serde_json::to_value(report), exit_code)
    };
    let value = value.map_err(|error| AppError::Runtime(error.to_string()))?;
    let stdout = match format {
        OutputFormat::Json => value.to_string(),
        OutputFormat::Markdown => live_markdown(&value),
    };
    Ok(CommandOutput::Text {
        stdout,
        stderr: None,
        exit_code,
    })
}

/// One line per check, after the summary of a run.
fn live_markdown(value: &Value) -> String {
    let mut output = String::from("# forager doctor --live\n\n");
    if let Some(summary) = value["summary"].as_object() {
        let _ = writeln!(
            output,
            "ok: {}\nunattended: {}\ntiers: {}\n",
            value["ok"].as_bool().unwrap_or(false),
            value["unattended"].as_bool().unwrap_or(false),
            value["tiers"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        );
        let counts = summary
            .iter()
            .map(|(status, count)| format!("{status}={count}"))
            .collect::<Vec<_>>();
        let _ = writeln!(output, "summary: {}\n", counts.join(", "));
    }
    for check in value["checks"].as_array().into_iter().flatten() {
        let _ = write!(
            output,
            "- {} ({})",
            check["id"].as_str().unwrap_or_default(),
            check["tier"].as_str().unwrap_or_default()
        );
        if let Some(status) = check["status"].as_str() {
            let _ = write!(output, ": {status}");
        }
        if check["rerun"] == true {
            output.push_str(", rerun");
        }
        if let Some(transition) = check["transition"].as_str() {
            let _ = write!(output, ", {}", transition.replace('_', " "));
            if let Some(since) = check["failing_since"].as_str() {
                let _ = write!(output, " since {since}");
            }
        }
        if let Some(status) = check["http_status"].as_u64() {
            let _ = write!(output, ", HTTP {status}");
        }
        if let Some(selected) = check["selected"].as_bool() {
            let _ = write!(output, ": selected={selected}");
            for flag in ["expensive", "manual_only"] {
                if check[flag] == true {
                    let _ = write!(output, ", {flag}");
                }
            }
        }
        if let Some(message) = check["message"].as_str() {
            let _ = write!(output, " ({message})");
        }
        output.push('\n');
    }
    if let Some(notifications) = value["notifications"].as_array() {
        let _ = writeln!(output, "\nnotifications: {}", notifications.len());
        for notification in notifications {
            let _ = writeln!(
                output,
                "- {} the issue labelled {}",
                notification["action"].as_str().unwrap_or_default(),
                notification["label"].as_str().unwrap_or_default()
            );
        }
    }
    if let Some(warning) = value["record_warning"].as_str() {
        let _ = writeln!(output, "\nrecord_warning: {warning}");
    }
    output
}

fn render_doctor_markdown(report: &crate::doctor::DoctorReport) -> Result<String, AppError> {
    doctor_markdown(
        &serde_json::to_value(report).map_err(|error| AppError::Runtime(error.to_string()))?,
    )
}

fn doctor_markdown(value: &Value) -> Result<String, AppError> {
    let mut output = format!(
        "# forager doctor\n\nmode: {}\nok: {}\n",
        value["mode"].as_str().unwrap_or_default(),
        value["ok"].as_bool().unwrap_or(false)
    );
    if let Some(health) = value["health"].as_str() {
        let _ = writeln!(output, "health: {health}");
    }
    output.push('\n');
    for provider in value["providers"]
        .as_array()
        .expect("doctor providers serialize as an array")
    {
        let _ = write!(
            output,
            "- {}: configured={}, key_count={}, source={}",
            provider["provider"].as_str().unwrap_or_default(),
            provider["configured"].as_bool().unwrap_or(false),
            provider["key_count"].as_u64().unwrap_or(0),
            provider["source"].as_str().unwrap_or_default(),
        );
        for check in ["reachable", "preflight"] {
            if let Some(passed) = provider[check].as_bool() {
                let _ = write!(output, ", {check}={passed}");
            }
        }
        if let Some(message) = provider["message"].as_str() {
            let _ = write!(output, " ({message})");
        }
        output.push('\n');
    }
    output.push_str("\n## State directories\n\n");
    for name in ["state", "journal"] {
        let directory = &value["state_directories"][name];
        let fields = ["enabled", "writable"]
            .into_iter()
            .filter_map(|field| {
                directory[field]
                    .as_bool()
                    .map(|flag| format!("{field}={flag}"))
            })
            .collect::<Vec<_>>();
        let _ = write!(output, "- {name}: {}", fields.join(", "));
        if let Some(message) = directory["message"].as_str() {
            let _ = write!(output, " ({message})");
        }
        output.push('\n');
    }
    if let Some(warnings) = value["permission_warnings"].as_array()
        && !warnings.is_empty()
    {
        output.push_str("\n## Permission warnings\n\n");
        for warning in warnings {
            let _ = writeln!(output, "- {}", warning.as_str().unwrap_or_default());
        }
    }
    output.push_str("\n## Effective configuration\n\n```json\n");
    output.push_str(
        &serde_json::to_string_pretty(&value["config"])
            .map_err(|error| AppError::Runtime(error.to_string()))?,
    );
    output.push_str("\n```\n");
    Ok(output)
}

fn render_provider_doctor_markdown(
    report: &crate::doctor::ProviderReport,
) -> Result<String, AppError> {
    let value =
        serde_json::to_value(report).map_err(|error| AppError::Runtime(error.to_string()))?;
    let mut output = format!(
        "# forager doctor: {}\n\nStatus: {}\n\n- configured: {}\n- key_count: {}\n- source: {}\n- deadline_seconds: {}\n",
        value["provider"].as_str().unwrap_or_default(),
        if value["ok"].as_bool().unwrap_or(false) {
            "ok"
        } else {
            "failed"
        },
        value["configured"].as_bool().unwrap_or(false),
        value["key_count"].as_u64().unwrap_or(0),
        value["source"].as_str().unwrap_or_default(),
        value["deadline_seconds"].as_u64().unwrap_or(0),
    );
    for check in value["checks"]
        .as_array()
        .expect("doctor checks serialize as an array")
    {
        let _ = writeln!(
            output,
            "- {} ({}): {}",
            check["name"].as_str().unwrap_or_default(),
            check["transport"].as_str().unwrap_or_default(),
            if check["ok"].as_bool().unwrap_or(false) {
                "ok"
            } else {
                "failed"
            },
        );
    }
    for key in value["keys"].as_array().into_iter().flatten() {
        let _ = write!(
            output,
            "- key {}: {}",
            key["key_index"].as_u64().unwrap_or_default(),
            key["error_kind"].as_str().unwrap_or("ok"),
        );
        for field in [
            "searches_left",
            "plan_searches_left",
            "this_month_usage",
            "this_hour_searches",
            "hourly_limit",
        ] {
            if let Some(count) = key[field].as_i64() {
                let _ = write!(output, ", {field}={count}");
            }
        }
        output.push('\n');
    }
    if let Some(message) = value["message"].as_str() {
        let _ = write!(
            output,
            "\n{}: {message}\n",
            value["error_kind"].as_str().unwrap_or("runtime")
        );
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{ci_environment, doctor_markdown, health_markdown, live_markdown};

    #[test]
    fn health_markdown_lists_the_findings_then_each_provider_and_schedule() {
        let report = json!({
            "mode": "health",
            "ok": false,
            "days": 7,
            "since": "2026-11-21T06:00:00Z",
            "telemetry_enabled": true,
            "findings": [
                {
                    "subject": "provider", "provider": "jina", "command": "fetch",
                    "finding": "degraded", "metric": "success_rate", "current": 0.8, "previous": 1.0,
                },
                {"subject": "tier", "tier": "metered", "finding": "overdue", "age_hours": 216, "interval_days": 7},
                {"subject": "workflow_record", "finding": "not_recorded"},
            ],
            "providers": [
                {
                    "provider": "jina", "command": "fetch", "calls": 20, "previous_calls": 20,
                    "success_rate": 0.8, "hidden_failures": 0, "empty_rate": 0.0,
                    "short_content_rate": 0.0, "duration_ms": {"p50": 310, "p95": 900},
                    "last_failure": {"at": "2026-11-28T04:00:00Z", "error_kind": "timeout", "http_status": 504},
                },
                {
                    "provider": "youtube_api", "command": "platform", "platform": "youtube",
                    "operation": "comments", "calls": 2, "previous_calls": 0, "success_rate": 1.0,
                    "hidden_failures": 0, "empty_rate": null, "duration_ms": {"p50": 100, "p95": 120},
                    "last_failure": null,
                },
            ],
            "schedules": [
                {"subject": "tier", "tier": "free", "interval_days": 1, "last_run_at": "2026-11-28T03:00:00Z", "age_hours": 3, "status": "current"},
                {"subject": "tier", "tier": "metered", "interval_days": 7, "last_run_at": "2026-11-19T06:00:00Z", "age_hours": 216, "status": "overdue"},
                {"subject": "workflow_record", "interval_days": null, "last_run_at": null, "age_hours": null, "status": "not_recorded"},
            ],
        });

        assert_eq!(
            health_markdown(&report),
            "# forager doctor --health\n\n\
             ok: false\ndays: 7, since 2026-11-21T06:00:00Z\ntelemetry_enabled: true\n\n\
             ## Findings\n\n\
             - jina fetch: degraded, current=0.8, metric=success_rate, previous=1.0\n\
             - tier metered: overdue, age_hours=216, interval_days=7\n\
             - workflow_record: not_recorded\n\
             \n## Providers\n\n\
             - jina fetch: calls=20, previous_calls=20, success_rate=0.8, hidden_failures=0, \
             empty_rate=0.0, short_content_rate=0.0, p50=310ms, p95=900ms, \
             last_failure=timeout HTTP 504 at 2026-11-28T04:00:00Z\n\
             - youtube_api platform youtube comments: calls=2, previous_calls=0, \
             success_rate=1.0, hidden_failures=0, p50=100ms, p95=120ms\n\
             \n## Schedules\n\n\
             - tier free: current, last run 2026-11-28T03:00:00Z, 3 hours ago, every 1 days\n\
             - tier metered: overdue, last run 2026-11-19T06:00:00Z, 216 hours ago, every 7 days\n\
             - workflow_record: not_recorded\n"
        );
    }

    #[test]
    fn offline_doctor_markdown_shows_the_health_line_preflight_and_state_directories_without_reachability()
     {
        let report = json!({
            "mode": "offline",
            "ok": false,
            "providers": [
                {"provider": "exa", "configured": true, "key_count": 1, "source": "file"},
                {
                    "provider": "xiaohongshu_browser",
                    "configured": true,
                    "key_count": 0,
                    "source": "default",
                    "preflight": false,
                    "message": "no browser",
                },
            ],
            "state_directories": {
                "state": {"writable": true},
                "journal": {"enabled": true, "writable": false, "message": "Not a directory"},
            },
            "permission_warnings": [],
            "config": {},
            "health": "tier free overdue; run `forager doctor --health` for the report",
        });

        let markdown = doctor_markdown(&report).expect("render markdown");

        assert_eq!(
            markdown,
            "# forager doctor\n\nmode: offline\nok: false\n\
             health: tier free overdue; run `forager doctor --health` for the report\n\n\
             - exa: configured=true, key_count=1, source=file\n\
             - xiaohongshu_browser: configured=true, key_count=0, source=default, \
             preflight=false (no browser)\n\
             \n## State directories\n\n\
             - state: writable=true\n\
             - journal: enabled=true, writable=false (Not a directory)\n\
             \n## Effective configuration\n\n```json\n{}\n```\n"
        );
    }

    #[test]
    fn only_a_set_non_empty_ci_variable_other_than_false_marks_a_ci_environment() {
        let values = [None, Some(""), Some("false"), Some("true"), Some("1")];

        assert_eq!(
            values.map(|value| ci_environment(value.map(std::ffi::OsString::from))),
            [false, false, false, true, true]
        );
    }

    #[test]
    fn live_doctor_markdown_shows_the_summary_and_one_line_per_check_with_its_reason() {
        let report = json!({
            "mode": "live",
            "ok": false,
            "unattended": true,
            "tiers": ["free", "metered"],
            "summary": {
                "failed": 1,
                "not_started": 0,
                "passed": 1,
                "unconfigured": 1,
                "unselected": 30,
            },
            "checks": [
                {"id": "jina:web_fetch:anonymous", "tier": "free", "status": "passed", "attempts": 1},
                {
                    "id": "hn_api:search",
                    "tier": "free",
                    "status": "failed",
                    "attempts": 4,
                    "rerun": true,
                    "error_kind": "network",
                    "http_status": 502,
                    "message": "the command failed with network: connection refused",
                    "transition": "still_failing",
                    "failing_since": "2026-11-02T06:00:00Z",
                },
                {
                    "id": "exa:docs_search",
                    "tier": "metered",
                    "status": "unconfigured",
                    "attempts": 0,
                    "message": "providers.exa.keys has no configured credentials",
                },
            ],
            "record_warning": "cannot write the run record: disk full",
        });

        assert_eq!(
            live_markdown(&report),
            "# forager doctor --live\n\n\
             ok: false\nunattended: true\ntiers: free, metered\n\n\
             summary: failed=1, not_started=0, passed=1, unconfigured=1, unselected=30\n\n\
             - jina:web_fetch:anonymous (free): passed\n\
             - hn_api:search (free): failed, rerun, still failing since 2026-11-02T06:00:00Z, \
             HTTP 502 (the command failed with network: connection refused)\n\
             - exa:docs_search (metered): unconfigured \
             (providers.exa.keys has no configured credentials)\n\n\
             record_warning: cannot write the run record: disk full\n"
        );
    }

    #[test]
    fn live_doctor_markdown_names_each_notification_by_its_action_and_label() {
        let report = json!({
            "mode": "live",
            "checks": [{"id": "exa:docs_search", "tier": "metered", "status": "failed"}],
            "notifications": [
                {"provider": "exa", "action": "open", "label": "live-check:exa", "title": "t", "body": "b"},
                {"provider": "jina", "action": "close", "label": "live-check:jina", "title": "t", "body": "b"},
            ],
        });

        assert_eq!(
            live_markdown(&report),
            "# forager doctor --live\n\n\
             - exa:docs_search (metered): failed\n\n\
             notifications: 2\n\
             - open the issue labelled live-check:exa\n\
             - close the issue labelled live-check:jina\n"
        );
    }
}
