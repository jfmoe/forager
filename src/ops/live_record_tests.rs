use std::fs;
use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use serde_json::{Value, json};

use super::{Run, Store, conclude};
use crate::catalog::{
    Field, LiveArgument, LiveCheck, LiveCommand, LiveContract, LiveTier, ProviderId, RoutePin,
};
use crate::live::{CheckResult, Declared, Execution, Prepared, Summary, run_checks, summarize};
use crate::redact::Secret;
use crate::types::Deadline;

const CONTRACT: LiveContract = LiveContract::fields(&[Field::list("results", 1)]);
const SECRET: &str = "sk-record-secret";

fn declared() -> [Declared; 1] {
    [Declared {
        provider: Some(ProviderId::Exa),
        check: LiveCheck::new(
            "exa:docs_search",
            LiveTier::Metered,
            LiveCommand {
                arguments: &[
                    LiveArgument::Text("exa"),
                    LiveArgument::Text("MARKER-QUERY"),
                ],
                routes: RoutePin::Configured,
            },
            CONTRACT,
            "a test sample",
        ),
    }]
}

fn passed() -> Execution {
    Execution::Exited {
        code: Some(0),
        stdout: json!({"results": [{"url": "https://example.com/MARKER-URL"}]})
            .to_string()
            .into_bytes(),
    }
}

/// A failure whose message carries the query, a URL, a configured key and page content.
fn failed() -> Execution {
    Execution::Exited {
        code: Some(4),
        stdout: json!({
            "error_kind": "network",
            "message": format!("MARKER-QUERY at https://example.com/MARKER-URL with {SECRET}: MARKER-CONTENT"),
            "provider_attempts": [{"provider": "exa", "error_kind": "network", "http_status": 502}],
        })
        .to_string()
        .into_bytes(),
    }
}

/// The check's result when each of its commands ends as `executions` says in turn.
fn results(executions: Vec<Execution>) -> (Vec<CheckResult>, Summary) {
    let declared = declared();
    let mut executions = executions.into_iter();
    let mut execute =
        |_: &Prepared, _: Duration| executions.next().expect("an execution for every command");
    let results = run_checks(
        &declared,
        &[true],
        &[None],
        Deadline::new(Duration::from_mins(10)),
        &[Secret::from(SECRET)],
        &mut execute,
        None,
    );
    let summary = summarize(&results, declared.len());
    (results, summary)
}

/// Records a run of the check at `now` and returns its transition as the report shows it.
fn record(store: &Store, now: DateTime<Utc>, scheduled: bool, executions: Vec<Execution>) -> Value {
    let (mut results, summary) = results(executions);
    conclude(
        store,
        now,
        &Run {
            tiers: &["metered"],
            scheduled,
            summary: &summary,
        },
        &mut results,
    )
    .expect("record the run");
    let reported = serde_json::to_value(&results[0]).expect("serialize the result");
    json!({
        "transition": reported["transition"],
        "failing_since": reported["failing_since"],
    })
}

fn day(day: u32) -> DateTime<Utc> {
    format!("2026-11-{day:02}T06:00:00Z")
        .parse()
        .expect("a valid time")
}

fn transition(name: &str) -> Value {
    json!({"transition": name, "failing_since": null})
}

#[test]
fn transitions_follow_the_scheduled_runs_and_an_unscheduled_run_changes_none_of_them() {
    let directory = tempfile::tempdir().expect("create the record directory");
    let store = Store::new(directory.path().join("live_runs"));
    // A timeout is retried once in the run, so a failing run times out twice.
    let failing = || vec![Execution::TimedOut, Execution::TimedOut];
    let passing = || vec![passed()];

    let sequence = [
        record(&store, day(1), false, failing()),
        record(&store, day(2), true, failing()),
        record(&store, day(3), true, failing()),
        record(&store, day(4), false, failing()),
        record(&store, day(5), true, passing()),
        record(&store, day(6), false, failing()),
        record(&store, day(7), true, passing()),
    ];

    let still_failing =
        json!({"transition": "still_failing", "failing_since": "2026-11-02T06:00:00Z"});
    assert_eq!(
        sequence,
        [
            transition("newly_failed"),
            transition("newly_failed"),
            still_failing.clone(),
            still_failing,
            transition("recovered"),
            transition("newly_failed"),
            json!({"transition": null, "failing_since": null}),
        ]
    );
}

fn only_record(directory: &Path) -> (std::path::PathBuf, String) {
    let entries = fs::read_dir(directory)
        .expect("read the record directory")
        .map(|entry| entry.expect("a directory entry").path())
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 1, "{entries:?}");
    let text = fs::read_to_string(&entries[0]).expect("read the record");
    (entries[0].clone(), text)
}

#[test]
fn a_run_record_holds_the_run_and_each_check_without_a_message_query_url_key_or_content() {
    let directory = tempfile::tempdir().expect("create the record directory");
    let records = directory.path().join("live_runs");
    let (mut results, summary) = results(vec![failed(), failed()]);

    conclude(
        &Store::new(records.clone()),
        day(10),
        &Run {
            tiers: &["free", "metered"],
            scheduled: true,
            summary: &summary,
        },
        &mut results,
    )
    .expect("record the run");

    let (_, text) = only_record(&records);
    let record: Value = serde_json::from_str(&text).expect("parse the record");
    assert_eq!(
        (
            [
                "schema_version",
                "run_id",
                "recorded_at",
                "forager_version",
                "tiers",
                "scheduled",
                "summary",
                "checks"
            ]
            .map(|field| record.get(field).is_some()),
            &record["tiers"],
            &record["scheduled"],
            &record["summary"]["failed"],
            &record["checks"][0]["id"],
            &record["checks"][0]["status"],
            &record["checks"][0]["error_kind"],
            &record["checks"][0]["http_status"],
            &record["checks"][0]["detail"],
            record["checks"][0]["duration_ms"].is_u64(),
            ["MARKER", SECRET, "example.com"]
                .into_iter()
                .filter(|marker| text.contains(marker))
                .collect::<Vec<_>>(),
        ),
        (
            [true; 8],
            &json!(["free", "metered"]),
            &json!(true),
            &json!(1),
            &json!("exa:docs_search"),
            &json!("failed"),
            &json!("network"),
            &json!(502),
            &json!({"rule": "command_succeeds"}),
            true,
            Vec::<&str>::new(),
        ),
        "{text}"
    );
}

#[cfg(unix)]
#[test]
fn a_run_record_and_its_directory_are_private() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().expect("create the record directory");
    let records = directory.path().join("live_runs");
    let (mut results, summary) = results(vec![passed()]);

    conclude(
        &Store::new(records.clone()),
        day(10),
        &Run {
            tiers: &["free"],
            scheduled: false,
            summary: &summary,
        },
        &mut results,
    )
    .expect("record the run");

    let (file, _) = only_record(&records);
    let mode = |path: &Path| fs::metadata(path).expect("stat").permissions().mode() & 0o777;
    assert_eq!((mode(&records), mode(&file)), (0o700, 0o600));
}

#[test]
fn records_older_than_90_days_are_removed_and_a_damaged_record_is_skipped() {
    let directory = tempfile::tempdir().expect("create the record directory");
    let records = directory.path().join("live_runs");
    fs::create_dir_all(&records).expect("create the records");
    let now = day(20);
    let name = |at: DateTime<Utc>| format!("live_run_{}_1.json", at.timestamp_millis());
    let failed_run = json!({
        "recorded_at": now - TimeDelta::days(1),
        "scheduled": true,
        "checks": [{"id": "exa:docs_search", "status": "failed"}],
    });
    let expired = name(now - TimeDelta::days(91));
    let kept = name(now - TimeDelta::days(1));
    let damaged = name(now - TimeDelta::hours(1));
    fs::write(records.join(&expired), failed_run.to_string()).expect("write the expired record");
    fs::write(records.join(&kept), failed_run.to_string()).expect("write the kept record");
    fs::write(records.join(&damaged), "{\"recorded_at\": ").expect("write the damaged record");
    fs::write(records.join("notes.txt"), "not a record").expect("write a foreign file");
    let (mut results, summary) = results(vec![passed()]);

    conclude(
        &Store::new(records.clone()),
        now,
        &Run {
            tiers: &["metered"],
            scheduled: true,
            summary: &summary,
        },
        &mut results,
    )
    .expect("record the run");

    let mut names = fs::read_dir(&records)
        .expect("read the records")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .into_string()
                .expect("name")
        })
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(
        (
            names.contains(&expired),
            names.contains(&kept),
            names.contains(&damaged),
            names.len(),
            serde_json::to_value(&results[0]).expect("serialize")["transition"].clone(),
        ),
        (false, true, true, 4, json!("recovered"))
    );
}
