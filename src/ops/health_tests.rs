use std::collections::BTreeMap;

use chrono::{DateTime, TimeDelta, Utc};
use serde_json::{Value, json};

use super::{Evidence, WorkflowStatus, summarize};
use crate::catalog::LiveTier;
use crate::chain::skipped_attempt;
use crate::telemetry::{
    Invocation, LocalKind, Record, ResultSignal, RetrievalCommand, TerminalKind,
};
use crate::types::{
    AttemptDisposition, AttemptErrorKind, AttemptTarget, Platform, ProviderAttempt,
};

fn now() -> DateTime<Utc> {
    "2026-11-28T06:00:00Z".parse().expect("a valid time")
}

fn ago(hours: i64) -> DateTime<Utc> {
    now() - TimeDelta::hours(hours)
}

/// The hours before `now` of the previous period's `hours`-th hour.
fn previous(hours: i64) -> DateTime<Utc> {
    ago(7 * 24 + hours)
}

fn attempt(
    provider: &'static str,
    failure: Option<(AttemptErrorKind, u16)>,
    duration_ms: u64,
) -> ProviderAttempt {
    ProviderAttempt {
        provider,
        target: AttemptTarget::seam("web_fetch"),
        disposition: if failure.is_some() {
            AttemptDisposition::Failed
        } else {
            AttemptDisposition::Succeeded
        },
        error_kind: failure.map(|(kind, _)| kind),
        http_status: Some(failure.map_or(200, |(_, status)| status)),
        duration_ms,
        credential_index: 0,
        retry_count: 0,
        rotation_count: 0,
        anonymous: false,
        message: String::new(),
        model: None,
        transport: Some("http"),
        endpoint_host: None,
    }
}

fn page(chars: usize) -> String {
    "a".repeat(chars)
}

/// A fetch at `at` that `provider` answered with a page of `chars` characters after `attempts`.
fn fetched(
    at: DateTime<Utc>,
    attempts: &[ProviderAttempt],
    provider: &'static str,
    chars: usize,
) -> Record {
    Record::new(
        RetrievalCommand::Fetch,
        Invocation::succeeded(attempts, provider, ResultSignal::content(&page(chars))),
        0,
        at,
    )
}

/// A fetch at `at` that `provider` answered at once with a long page.
fn fetched_by(at: DateTime<Utc>, provider: &'static str) -> Record {
    fetched(at, &[attempt(provider, None, 100)], provider, 2000)
}

/// A fetch at `at` that failed after `attempts`.
fn failed_fetch(at: DateTime<Utc>, attempts: &[ProviderAttempt]) -> Record {
    Record::new(
        RetrievalCommand::Fetch,
        Invocation::failed(attempts, AttemptErrorKind::Network),
        4,
        at,
    )
}

/// `count` fetches that `provider` answered at once, in the hours `first..` of each period.
fn steady(provider: &'static str, count: i64, first: i64) -> Vec<Record> {
    (first..first + count)
        .flat_map(|hour| {
            [
                fetched_by(ago(hour), provider),
                fetched_by(previous(hour), provider),
            ]
        })
        .collect()
}

/// Both scheduled tiers ran within their intervals.
fn current_runs() -> BTreeMap<LiveTier, DateTime<Utc>> {
    BTreeMap::from([(LiveTier::Free, ago(3)), (LiveTier::Metered, ago(24))])
}

fn workflow(name: &str, last_success: Option<DateTime<Utc>>) -> WorkflowStatus {
    WorkflowStatus {
        workflow: name.to_owned(),
        interval_days: 7.try_into().expect("a positive interval"),
        last_success,
    }
}

fn current_workflows() -> Vec<WorkflowStatus> {
    vec![workflow("build", Some(ago(48)))]
}

/// The report of `telemetry` with every schedule current, as JSON.
fn report_of(telemetry: &[Record]) -> Value {
    let workflows = current_workflows();
    serde_json::to_value(summarize(
        &Evidence {
            telemetry,
            scheduled_runs: &current_runs(),
            workflows: Some(&workflows),
        },
        now(),
        7,
    ))
    .expect("serialize the report")
}

#[test]
fn hidden_and_short_answers_a_small_sample_and_stale_schedules_are_exactly_the_findings() {
    let mut telemetry = steady("jina", 20, 1);
    telemetry.push(fetched_by(previous(21), "jina"));
    telemetry.push(fetched(
        ago(30),
        &[
            attempt("jina", Some((AttemptErrorKind::Network, 502)), 100),
            attempt("tavily", None, 100),
        ],
        "tavily",
        2000,
    ));
    telemetry.push(fetched(ago(40), &[attempt("jina", None, 100)], "jina", 300));
    telemetry.sort_by_key(|record| record.recorded_at);
    let runs = BTreeMap::from([(LiveTier::Free, ago(20)), (LiveTier::Metered, ago(9 * 24))]);
    let workflows = [
        workflow("build", Some(ago(3 * 24))),
        workflow("advisories", Some(ago(10 * 24))),
    ];

    let report = serde_json::to_value(summarize(
        &Evidence {
            telemetry: &telemetry,
            scheduled_runs: &runs,
            workflows: Some(&workflows),
        },
        now(),
        7,
    ))
    .expect("serialize the report");

    let jina = json!({"subject": "provider", "provider": "jina", "command": "fetch"});
    let with = |subject: &Value, finding: Value| {
        let mut entry = subject.clone();
        entry
            .as_object_mut()
            .expect("an object")
            .extend(finding.as_object().expect("an object").clone());
        entry
    };
    assert_eq!(
        (&report["ok"], &report["findings"]),
        (
            &json!(false),
            &json!([
                with(&jina, json!({"finding": "hidden_failures", "count": 1})),
                with(&jina, json!({"finding": "short_content", "count": 1})),
                {
                    "subject": "provider", "provider": "tavily", "command": "fetch",
                    "finding": "sample_too_small", "calls": 1, "previous_calls": 0, "minimum": 20,
                },
                {
                    "subject": "tier", "tier": "metered",
                    "finding": "overdue", "age_hours": 216, "interval_days": 7,
                },
                {
                    "subject": "workflow", "workflow": "advisories",
                    "finding": "overdue", "age_hours": 240, "interval_days": 7,
                },
            ])
        )
    );
}

/// `failures` of `count` fetches by `provider` failed in this period, in the hours `first..`.
fn failing(provider: &'static str, count: i64, failures: i64, first: i64) -> Vec<Record> {
    (first..first + count)
        .map(|hour| {
            if hour - first < failures {
                failed_fetch(
                    ago(hour),
                    &[attempt(
                        provider,
                        Some((AttemptErrorKind::Timeout, 504)),
                        100,
                    )],
                )
            } else {
                fetched_by(ago(hour), provider)
            }
        })
        .chain((first..first + count).map(|hour| fetched_by(previous(hour), provider)))
        .collect()
}

#[test]
fn a_rate_a_fifth_worse_than_the_previous_period_is_degraded_and_a_smaller_move_is_not() {
    let mut telemetry = failing("jina", 20, 4, 1);
    telemetry.extend(failing("tavily", 20, 3, 30));
    telemetry.extend(steady("firecrawl", 20, 60));
    for hour in 60..65 {
        telemetry.push(fetched(
            ago(hour),
            &[attempt("firecrawl", None, 100)],
            "firecrawl",
            300,
        ));
    }
    telemetry.sort_by_key(|record| record.recorded_at);

    let report = report_of(&telemetry);

    assert_eq!(
        (&report["ok"], &report["findings"]),
        (
            &json!(false),
            &json!([
                {
                    "subject": "provider", "provider": "firecrawl", "command": "fetch",
                    "finding": "degraded", "metric": "short_content_rate",
                    "current": 0.2, "previous": 0.0,
                },
                {
                    "subject": "provider", "provider": "firecrawl", "command": "fetch",
                    "finding": "short_content", "count": 5,
                },
                {
                    "subject": "provider", "provider": "jina", "command": "fetch",
                    "finding": "degraded", "metric": "success_rate",
                    "current": 0.8, "previous": 1.0,
                },
            ])
        )
    );
}

#[test]
fn a_period_below_the_minimum_calls_is_too_small_to_compare_even_when_every_call_failed() {
    let mut telemetry = failing("jina", 20, 20, 1);
    telemetry.retain(|record| record.recorded_at != previous(1));

    let report = report_of(&telemetry);

    assert_eq!(
        (&report["ok"], &report["findings"]),
        (
            &json!(true),
            &json!([{
                "subject": "provider", "provider": "jina", "command": "fetch",
                "finding": "sample_too_small", "calls": 20, "previous_calls": 19, "minimum": 20,
            }])
        )
    );
}

#[test]
fn argument_configuration_and_output_errors_count_as_no_failure() {
    let answered = Record::new(
        RetrievalCommand::Fetch,
        Invocation::succeeded(
            &[attempt("jina", None, 100)],
            "jina",
            ResultSignal::content(&page(2000)),
        )
        .with_unwritten_output(),
        3,
        ago(1),
    );
    let refused = [LocalKind::Argument, LocalKind::Config].map(|kind| {
        Record::new(
            RetrievalCommand::Fetch,
            Invocation::preflight(TerminalKind::Local(kind)),
            if kind == LocalKind::Argument { 2 } else { 3 },
            ago(2),
        )
    });
    let mut telemetry = vec![answered];
    telemetry.extend(refused);

    let report = report_of(&telemetry);

    assert_eq!(
        report["providers"]
            .as_array()
            .expect("providers")
            .iter()
            .map(|row| (
                &row["provider"],
                &row["calls"],
                &row["success_rate"],
                &row["last_failure"]
            ))
            .collect::<Vec<_>>(),
        [(&json!("jina"), &json!(1), &json!(1.0), &Value::Null)]
    );
}

#[test]
fn a_row_gives_one_provider_and_command_its_rates_durations_and_last_failure() {
    let comments = |at, attempts: &[ProviderAttempt], items: Option<usize>| {
        Record::new(
            RetrievalCommand::Platform(Platform::Youtube, "comments"),
            match items {
                Some(items) => {
                    Invocation::succeeded(attempts, "youtube_api", ResultSignal::items(items))
                }
                None => Invocation::failed(attempts, AttemptErrorKind::Network),
            },
            if items.is_some() { 0 } else { 4 },
            at,
        )
    };
    let skipped = skipped_attempt(
        AttemptTarget::platform("youtube", "comments"),
        "youtube_web",
        "not configured",
    );
    let telemetry = [
        comments(
            ago(10),
            &[skipped, attempt("youtube_api", None, 100)],
            Some(3),
        ),
        comments(ago(8), &[attempt("youtube_api", None, 200)], Some(0)),
        comments(
            ago(5),
            &[attempt(
                "youtube_api",
                Some((AttemptErrorKind::Network, 503)),
                300,
            )],
            None,
        ),
        comments(
            ago(2),
            &[attempt(
                "youtube_api",
                Some((AttemptErrorKind::RateLimited, 429)),
                400,
            )],
            None,
        ),
    ];

    let report = report_of(&telemetry);

    assert_eq!(
        report["providers"],
        json!([{
            "provider": "youtube_api",
            "command": "platform",
            "platform": "youtube",
            "operation": "comments",
            "calls": 4,
            "previous_calls": 0,
            "success_rate": 0.5,
            "hidden_failures": 0,
            "empty_rate": 0.5,
            "duration_ms": {"p50": 200, "p95": 400},
            "last_failure": {
                "at": "2026-11-28T04:00:00Z",
                "error_kind": "rate_limited",
                "http_status": 429,
            },
        }])
    );
}

#[test]
fn a_schedule_is_overdue_past_its_interval_and_a_day_and_one_without_a_record_is_not_recorded() {
    let runs = BTreeMap::from([
        (LiveTier::Free, ago(48)),
        (LiveTier::Metered, ago(8 * 24 + 1)),
    ]);
    let workflows = [workflow("advisories", None)];
    let report = |workflows| {
        serde_json::to_value(summarize(
            &Evidence {
                telemetry: &[],
                scheduled_runs: &runs,
                workflows,
            },
            now(),
            7,
        ))
        .expect("serialize the report")
    };

    let statuses = [report(Some(&workflows)), report(None)].map(|report| {
        report["schedules"]
            .as_array()
            .expect("schedules")
            .iter()
            .map(|schedule| {
                format!(
                    "{} {}: {}",
                    schedule["subject"].as_str().unwrap_or_default(),
                    schedule["tier"]
                        .as_str()
                        .or(schedule["workflow"].as_str())
                        .unwrap_or_default(),
                    schedule["status"].as_str().unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
    });

    assert_eq!(
        statuses,
        [
            [
                "tier free: current",
                "tier metered: overdue",
                "workflow advisories: not_recorded",
            ],
            [
                "tier free: current",
                "tier metered: overdue",
                "workflow_record : not_recorded",
            ],
        ]
    );
}

#[test]
fn the_attention_line_names_what_is_overdue_or_degraded_and_nothing_else_raises_it() {
    let mut degraded = failing("jina", 20, 10, 1);
    degraded.push(fetched(
        ago(40),
        &[
            attempt("tavily", Some((AttemptErrorKind::Network, 502)), 100),
            attempt("jina", None, 100),
        ],
        "jina",
        300,
    ));
    let runs = BTreeMap::from([(LiveTier::Free, ago(3 * 24))]);
    let quiet = [fetched(
        ago(1),
        &[
            attempt("tavily", Some((AttemptErrorKind::Network, 502)), 100),
            attempt("jina", None, 100),
        ],
        "jina",
        300,
    )];

    let lines = [
        (&degraded[..], &runs, None),
        (&quiet[..], &BTreeMap::new(), None),
    ]
    .map(|(telemetry, scheduled_runs, workflows)| {
        summarize(
            &Evidence {
                telemetry,
                scheduled_runs,
                workflows,
            },
            now(),
            7,
        )
        .attention()
    });

    assert_eq!(
        lines,
        [
            Some(
                "jina fetch degraded, tier free overdue; run `forager doctor --health` for the report"
                    .to_owned()
            ),
            None,
        ]
    );
}

#[test]
fn a_missing_or_malformed_workflow_record_is_not_recorded() {
    let directory = tempfile::tempdir().expect("create the state directory");
    let path = directory.path().join(super::WORKFLOWS_FILE);
    let valid = json!({
        "schema_version": 1,
        "workflows": [
            {"workflow": "build", "interval_days": 7, "last_success": "2026-11-27T03:00:00Z"},
            {"workflow": "advisories", "interval_days": 7, "last_success": null},
        ],
    });
    let with = |field: &str, value: Value| {
        let mut record = valid.clone();
        record["workflows"][0][field] = value;
        record.to_string()
    };
    let cases = [
        ("valid", Some(valid.to_string())),
        ("missing", None),
        ("not JSON", Some("{\"schema_version\": 1,".to_owned())),
        (
            "another schema",
            Some(
                valid
                    .to_string()
                    .replace("\"schema_version\":1", "\"schema_version\":2"),
            ),
        ),
        ("a zero interval", Some(with("interval_days", json!(0)))),
        (
            "a success that is no time",
            Some(with("last_success", json!("last week"))),
        ),
        ("no workflow name", Some(with("workflow", Value::Null))),
    ];

    let read = cases.map(|(case, text)| {
        match text {
            Some(text) => std::fs::write(&path, text).expect("write the record"),
            None => {
                let _ = std::fs::remove_file(&path);
            }
        }
        (
            case,
            super::read_workflows(&path).map(|workflows| {
                workflows
                    .iter()
                    .map(|workflow| (workflow.workflow.clone(), workflow.last_success.is_some()))
                    .collect::<Vec<_>>()
            }),
        )
    });

    assert_eq!(
        read,
        [
            (
                "valid",
                Some(vec![
                    ("build".to_owned(), true),
                    ("advisories".to_owned(), false)
                ])
            ),
            ("missing", None),
            ("not JSON", None),
            ("another schema", None),
            ("a zero interval", None),
            ("a success that is no time", None),
            ("no workflow name", None),
        ]
    );
}
