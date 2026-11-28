use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use serde_json::{Value, json};

use super::{Run, Store, conclude, last_scheduled_runs};
use crate::catalog::{
    Field, LiveArgument, LiveCheck, LiveCommand, LiveContract, LiveTier, ProviderId, RoutePin,
};
use crate::live::{
    CONFIRMATION_DELAY, CheckResult, Confirmation, Declared, Execution, Prepared, Summary,
    run_checks, summarize,
};
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
    results_within(executions, Deadline::new(Duration::from_mins(10)), false)
}

/// As [`results`], within `deadline`, and unattended when `confirming`.
fn results_within(
    executions: Vec<Execution>,
    deadline: Deadline,
    confirming: bool,
) -> (Vec<CheckResult>, Summary) {
    let declared = declared();
    let mut executions = executions.into_iter();
    let mut execute =
        |_: &Prepared, _: Duration| executions.next().expect("an execution for every command");
    let results = run_checks(
        &declared,
        &[true],
        &[None],
        deadline,
        &[Secret::from(SECRET)],
        &mut execute,
        confirming.then_some(Confirmation {
            delay: CONFIRMATION_DELAY,
            wait: &mut |_| {},
        }),
    );
    let summary = summarize(&results, declared.len());
    (results, summary)
}

/// Records a run of the check at `now` and returns its transition as the report shows it.
fn record(store: &Store, now: DateTime<Utc>, scheduled: bool, executions: Vec<Execution>) -> Value {
    record_results(store, now, scheduled, results(executions))
}

fn record_results(
    store: &Store,
    now: DateTime<Utc>,
    scheduled: bool,
    (mut results, summary): (Vec<CheckResult>, Summary),
) -> Value {
    conclude(
        store,
        now,
        &Run {
            tiers: &["metered"],
            scheduled,
            summary: &summary,
            declared: &["exa:docs_search"],
        },
        &mut results,
    )
    .record
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
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
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
            declared: &["exa:docs_search"],
        },
        &mut results,
    )
    .record
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

#[test]
fn a_failure_left_unconfirmed_has_no_transition_and_is_no_baseline() {
    let directory = tempfile::tempdir().expect("create the record directory");
    let store = Store::new(directory.path().join("live_runs"));
    // An unattended run with no room for the rerun leaves its failure unconfirmed.
    let unconfirmed = || {
        results_within(
            vec![failed(), failed()],
            Deadline::new(CONFIRMATION_DELAY),
            true,
        )
    };

    let sequence = [
        record(&store, day(1), true, vec![passed()]),
        record_results(&store, day(2), true, unconfirmed()),
        record(&store, day(3), true, vec![passed()]),
        record(&store, day(4), true, vec![failed(), failed()]),
        record_results(&store, day(5), true, unconfirmed()),
        record(&store, day(6), true, vec![failed(), failed()]),
    ];

    let none = json!({"transition": null, "failing_since": null});
    assert_eq!(
        sequence,
        [
            none.clone(),
            none.clone(),
            none.clone(),
            transition("newly_failed"),
            none,
            json!({"transition": "still_failing", "failing_since": "2026-11-04T06:00:00Z"}),
        ]
    );
}

#[test]
fn an_expired_record_is_no_baseline_even_before_it_is_removed() {
    let directory = tempfile::tempdir().expect("create the record directory");
    let records = directory.path().join("live_runs");
    let store = Store::new(records.clone());
    let now = day(20);
    let expired = now - TimeDelta::days(91);
    store
        .records
        .write(
            expired,
            &json!({
                "recorded_at": expired,
                "scheduled": true,
                "checks": [{"id": "exa:docs_search", "status": "failed"}],
            }),
        )
        .expect("write the expired record");
    let (expired_path, _) = only_record(&records);
    let (mut results, summary) = results(vec![passed()]);

    let concluded = conclude(
        &store,
        now,
        &Run {
            tiers: &["metered"],
            scheduled: true,
            summary: &summary,
            declared: &["exa:docs_search"],
        },
        &mut results,
    );

    assert_eq!(
        (
            serde_json::to_value(&results[0]).expect("serialize")["transition"].clone(),
            expired_path.exists(),
            concluded.record.is_ok(),
        ),
        (Value::Null, false, true)
    );
}

#[test]
fn each_tier_s_last_scheduled_run_counts_a_run_left_unconfirmed_and_no_run_by_hand() {
    let directory = tempfile::tempdir().expect("create the record directory");
    let store = Store::new(directory.path().join("live_runs"));
    let run = |at,
               tiers: &[&'static str],
               scheduled,
               (mut results, summary): (Vec<CheckResult>, Summary)| {
        conclude(
            &store,
            at,
            &Run {
                tiers,
                scheduled,
                summary: &summary,
                declared: &["exa:docs_search"],
            },
            &mut results,
        )
        .record
        .expect("record the run");
    };
    run(day(1), &["free", "metered"], true, results(vec![passed()]));
    // An unattended run with no room for the rerun leaves its failure unconfirmed.
    run(
        day(3),
        &["free"],
        true,
        results_within(
            vec![failed(), failed()],
            Deadline::new(CONFIRMATION_DELAY),
            true,
        ),
    );
    run(day(4), &["free", "metered"], false, results(vec![passed()]));

    assert_eq!(
        last_scheduled_runs(&store, day(5)),
        BTreeMap::from([(LiveTier::Free, day(3)), (LiveTier::Metered, day(1))])
    );
}

/// A failure whose own words carry a configured key, a private endpoint and a usage figure, and
/// whose error kind is `error_kind`.
fn failed_quoting_the_configuration(error_kind: &str) -> Execution {
    Execution::Exited {
        code: Some(4),
        stdout: json!({
            "error_kind": error_kind,
            "message": format!("{SECRET} at https://gateway.internal.example/v1: searches_left 4183"),
            "provider_attempts": [{"provider": "exa", "error_kind": "network", "http_status": 502}],
        })
        .to_string()
        .into_bytes(),
    }
}

#[test]
fn an_unattended_runs_issue_text_holds_only_check_identifiers_error_kinds_and_http_statuses() {
    let markers = [SECRET, "gateway.internal.example", "4183"];
    let notice = |error_kind: &str| {
        let directory = tempfile::tempdir().expect("create the record directory");
        // A network failure is retried once in each observation, and the rerun is the second.
        let (mut results, summary) = results_within(
            (0..4)
                .map(|_| failed_quoting_the_configuration(error_kind))
                .collect(),
            Deadline::new(Duration::from_mins(10)),
            true,
        );
        let conclusion = conclude(
            &Store::new(directory.path().join("live_runs")),
            day(10),
            &Run {
                tiers: &["metered"],
                scheduled: true,
                summary: &summary,
                declared: &["exa:docs_search"],
            },
            &mut results,
        );
        serde_json::to_value(&conclusion.notifications).expect("serialize the notifications")
    };

    let network = notice("network");
    // The error kind comes from the command's output, so one forager does not name is not shown.
    let unnamed = notice("https://gateway.internal.example/v1");

    let text = format!("{network}{unnamed}");
    assert_eq!(
        (
            &network[0]["label"],
            network[0]["body"]
                .as_str()
                .is_some_and(|body| body.contains("- `exa:docs_search`: `network`, HTTP 502\n")),
            unnamed[0]["body"].as_str().is_some_and(
                |body| body.contains("- `exa:docs_search`: `unrecognised`, HTTP 502\n")
            ),
            markers
                .into_iter()
                .filter(|marker| text.contains(marker))
                .collect::<Vec<_>>(),
        ),
        (&json!("live-check:exa"), true, true, Vec::<&str>::new(),),
        "{text}"
    );
}
