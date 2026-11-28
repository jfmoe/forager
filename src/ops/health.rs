//! The health summary: what Invocation Telemetry, the scheduled Live Check runs and the record of
//! the GitHub scheduled workflows say about the last few days, read from the state directory
//! without sending a request.

use std::collections::BTreeMap;
use std::fs;
use std::num::NonZeroU32;
use std::path::Path;

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize, Serializer};

use crate::catalog::LiveTier;
use crate::telemetry::{self, CommandName, Record};
use crate::types::{AttemptDisposition, AttemptErrorKind, Platform};

/// The days a summary covers by default.
pub(crate) const DEFAULT_DAYS: u32 = 7;
/// The most days a summary covers, so the period before it stays inside telemetry's retention.
pub(crate) const MAX_DAYS: u32 = 15;
/// The calls a provider needs in each of the two periods before they are compared.
const MINIMUM_CALLS: usize = 20;
/// A provider whose calls succeed less often than this fraction, with the minimum calls in the
/// period, is failing, whatever the period before said: a fallback can hide such a provider for
/// weeks, and a comparison sees nothing once both periods fail alike.
const FAILING_BELOW: (usize, usize) = (1, 2);
/// A rate that moved this far for the worse, as a fraction, is a degradation.
const DEGRADATION: (usize, usize) = (1, 5);
/// A fetched page shorter than this many characters is short. Thinner content already fails
/// the fetch, so a short page that passed is the likeliest form of a block page answered as
/// content.
const SHORT_CONTENT_CHARS: usize = 500;
/// How late a schedule may be past its interval before it is overdue.
const GRACE: TimeDelta = TimeDelta::days(1);
/// The record of the GitHub scheduled workflows, in the state directory.
const WORKFLOWS_FILE: &str = "github_workflows.json";
const WORKFLOW_SCHEMA_VERSION: u32 = 1;

/// What the summary reads, already parsed.
pub(crate) struct Evidence<'a> {
    /// The telemetry records of the period and the one before it.
    pub(crate) telemetry: &'a [Record],
    /// When the latest scheduled run of each tier was recorded.
    pub(crate) scheduled_runs: &'a BTreeMap<LiveTier, DateTime<Utc>>,
    /// The last success of each GitHub scheduled workflow; `None` when it was not recorded.
    pub(crate) workflows: Option<&'a [WorkflowStatus]>,
}

/// The record of the GitHub scheduled workflows that the weekly job writes.
#[derive(Debug, Deserialize)]
struct WorkflowRecord {
    schema_version: u32,
    workflows: Vec<WorkflowStatus>,
}

/// One GitHub scheduled workflow: how often it runs and when it last succeeded.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WorkflowStatus {
    pub(crate) workflow: String,
    pub(crate) interval_days: NonZeroU32,
    pub(crate) last_success: Option<DateTime<Utc>>,
}

/// The GitHub workflow record at `path`; `None` when it is missing or malformed.
fn read_workflows(path: &Path) -> Option<Vec<WorkflowStatus>> {
    let record = serde_json::from_slice::<WorkflowRecord>(&fs::read(path).ok()?).ok()?;
    (record.schema_version == WORKFLOW_SCHEMA_VERSION).then_some(record.workflows)
}

/// The summary of the `days` before `now`, read from forager's state directory; with no state
/// directory, nothing is recorded.
pub(crate) fn report(days: u32, now: DateTime<Utc>) -> HealthReport {
    let state = crate::state_file::state_directory();
    let telemetry = state
        .as_ref()
        .map(|state| {
            telemetry::read(
                &state.join(telemetry::DIRECTORY),
                now - TimeDelta::days(2 * i64::from(days)),
            )
        })
        .unwrap_or_default();
    let scheduled_runs = state
        .as_ref()
        .map(|state| crate::live::last_scheduled_runs(state, now))
        .unwrap_or_default();
    let workflows = state
        .as_ref()
        .and_then(|state| read_workflows(&state.join(WORKFLOWS_FILE)));
    summarize(
        &Evidence {
            telemetry: &telemetry,
            scheduled_runs: &scheduled_runs,
            workflows: workflows.as_deref(),
        },
        now,
        days,
    )
}

/// The summary of the `days` before its end.
#[derive(Debug, Serialize)]
pub(crate) struct HealthReport {
    mode: &'static str,
    /// No finding needs attention: nothing is failing, degraded or overdue.
    ok: bool,
    days: u32,
    since: DateTime<Utc>,
    findings: Vec<Finding>,
    providers: Vec<ProviderHealth>,
    schedules: Vec<ScheduleHealth>,
}

/// One provider serving one command.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct ProviderUse {
    provider: String,
    command: CommandName,
    #[serde(skip_serializing_if = "Option::is_none")]
    platform: Option<Platform>,
    #[serde(skip_serializing_if = "Option::is_none")]
    operation: Option<String>,
}

/// What a finding or a schedule is about.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "subject", rename_all = "snake_case")]
enum Subject {
    Provider(ProviderUse),
    Tier {
        tier: &'static str,
    },
    Workflow {
        workflow: String,
    },
    /// The record of the GitHub scheduled workflows as a whole.
    WorkflowRecord,
}

#[derive(Debug, Serialize)]
struct Finding {
    #[serde(flatten)]
    subject: Subject,
    #[serde(flatten)]
    kind: FindingKind,
}

#[derive(Debug, Serialize)]
#[serde(tag = "finding", rename_all = "snake_case")]
enum FindingKind {
    /// Too few of the period's calls succeeded.
    Failing {
        success_rate: Rate,
        calls: usize,
    },
    /// A rate moved for the worse against the period before.
    Degraded {
        metric: Metric,
        current: Rate,
        previous: Rate,
    },
    /// Failures that another provider's answer hid.
    HiddenFailures {
        count: usize,
    },
    /// Fetched pages that were short.
    ShortContent {
        count: usize,
    },
    /// Too few calls in a period to compare the two.
    SampleTooSmall {
        calls: usize,
        previous_calls: usize,
        minimum: usize,
    },
    /// The last run is older than its interval and a day.
    Overdue {
        age_hours: i64,
        interval_days: u32,
    },
    NotRecorded,
}

impl FindingKind {
    /// One finding of each kind, in the order the product skill lists them.
    const VOCABULARY: [Self; 7] = [
        Self::Failing {
            success_rate: Rate(0.0),
            calls: 0,
        },
        Self::Degraded {
            metric: Metric::Success,
            current: Rate(0.0),
            previous: Rate(0.0),
        },
        Self::HiddenFailures { count: 0 },
        Self::ShortContent { count: 0 },
        Self::SampleTooSmall {
            calls: 0,
            previous_calls: 0,
            minimum: 0,
        },
        Self::Overdue {
            age_hours: 0,
            interval_days: 0,
        },
        Self::NotRecorded,
    ];
}

/// The rate a degradation is about.
#[derive(Clone, Copy, Debug, Serialize)]
enum Metric {
    #[serde(rename = "success_rate")]
    Success,
    #[serde(rename = "empty_rate")]
    Empty,
    #[serde(rename = "short_content_rate")]
    ShortContent,
}

/// A share, shown to three decimal places.
#[derive(Clone, Copy, Debug)]
struct Rate(f64);

impl Serialize for Rate {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64((self.0 * 1000.0).round() / 1000.0)
    }
}

#[derive(Debug, Serialize)]
struct ProviderHealth {
    #[serde(flatten)]
    usage: ProviderUse,
    calls: usize,
    previous_calls: usize,
    success_rate: Rate,
    hidden_failures: usize,
    /// The share of its answers that were a legitimate empty result; `null` without an answer.
    empty_rate: Option<Rate>,
    /// The share of its fetched pages that were short; only `fetch` has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    short_content_rate: Option<Rate>,
    duration_ms: Durations,
    last_failure: Option<LastFailure>,
}

#[derive(Debug, Serialize)]
struct Durations {
    p50: u64,
    p95: u64,
}

#[derive(Clone, Debug, Serialize)]
struct LastFailure {
    at: DateTime<Utc>,
    error_kind: Option<AttemptErrorKind>,
    http_status: Option<u16>,
}

#[derive(Debug, Serialize)]
struct ScheduleHealth {
    #[serde(flatten)]
    subject: Subject,
    interval_days: Option<u32>,
    last_run_at: Option<DateTime<Utc>>,
    age_hours: Option<i64>,
    status: ScheduleStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ScheduleStatus {
    Current,
    Overdue,
    NotRecorded,
}

impl ScheduleStatus {
    /// Every status, in the order the product skill lists them.
    const VOCABULARY: [Self; 3] = [Self::Current, Self::Overdue, Self::NotRecorded];
}

/// The names of the health summary's findings and schedule statuses, in the order the product
/// skill lists them.
pub(crate) fn vocabularies() -> [(&'static str, Vec<String>); 2] {
    let name = |value: serde_json::Value, field: Option<&str>| {
        field
            .map_or(&value, |field| &value[field])
            .as_str()
            .unwrap_or_default()
            .to_owned()
    };
    [
        (
            "health finding",
            FindingKind::VOCABULARY
                .iter()
                .map(|kind| name(serde_json::json!(kind), Some("finding")))
                .collect(),
        ),
        (
            "schedule status",
            ScheduleStatus::VOCABULARY
                .iter()
                .map(|status| name(serde_json::json!(status), None))
                .collect(),
        ),
    ]
}

/// Summarises `evidence` over the `days` before `now`, against the `days` before those.
pub(crate) fn summarize(evidence: &Evidence<'_>, now: DateTime<Utc>, days: u32) -> HealthReport {
    let since = now - TimeDelta::days(i64::from(days));
    let (current, previous): (Vec<&Record>, Vec<&Record>) = evidence
        .telemetry
        .iter()
        .filter(|record| record.recorded_at >= since - TimeDelta::days(i64::from(days)))
        .partition(|record| record.recorded_at >= since);
    let previous = tally(previous);
    let mut findings = Vec::new();
    let providers =
        tally(current)
            .into_iter()
            .map(|(usage, tally)| {
                let before = previous.get(&usage);
                findings.extend(provider_findings(&tally, before).into_iter().map(|kind| {
                    Finding {
                        subject: Subject::Provider(usage.clone()),
                        kind,
                    }
                }));
                tally.health(usage, before.map_or(0, |before| before.calls))
            })
            .collect::<Vec<_>>();
    let schedules = schedules(evidence, now);
    findings.extend(schedules.iter().filter_map(ScheduleHealth::finding));
    HealthReport {
        mode: "health",
        ok: !findings.iter().any(Finding::needs_attention),
        days,
        since,
        findings,
        providers,
        schedules,
    }
}

impl HealthReport {
    /// Whether nothing is failing, degraded or overdue.
    pub(crate) const fn ok(&self) -> bool {
        self.ok
    }

    /// One line naming every finding that needs attention, when there is one.
    pub(crate) fn attention(&self) -> Option<String> {
        let mut named = Vec::<String>::new();
        for finding in &self.findings {
            let Some(state) = finding.kind.attention() else {
                continue;
            };
            let line = format!("{} {state}", finding.subject.label());
            if !named.contains(&line) {
                named.push(line);
            }
        }
        (!named.is_empty()).then(|| {
            format!(
                "{}; run `forager doctor --health` for the report",
                named.join(", ")
            )
        })
    }
}

impl Subject {
    /// How a line of text names the subject, such as `jina fetch` or `tier free`.
    fn label(&self) -> String {
        label(&serde_json::to_value(self).expect("a subject serializes"))
    }
}

/// The fields that name what an entry of the report is about: a finding, a provider row or a
/// schedule.
pub(crate) const SUBJECT_FIELDS: [&str; 7] = [
    "subject",
    "provider",
    "command",
    "platform",
    "operation",
    "tier",
    "workflow",
];

/// How a line of text names what a serialized entry of the report is about, such as
/// `jina fetch`, `youtube_api platform youtube comments` or `tier free`.
pub(crate) fn label(entry: &serde_json::Value) -> String {
    let named = SUBJECT_FIELDS[1..]
        .iter()
        .filter_map(|field| entry[*field].as_str())
        .collect::<Vec<_>>();
    match entry["subject"].as_str() {
        Some(subject @ ("tier" | "workflow")) => format!("{subject} {}", named.join(" ")),
        Some(subject) if named.is_empty() => subject.to_owned(),
        _ => named.join(" "),
    }
}

/// What the records say about one provider serving one command in one period.
#[derive(Default)]
struct Tally {
    calls: usize,
    successes: usize,
    hidden_failures: usize,
    answers: usize,
    empty: usize,
    /// Short fetched pages; `None` for a command that fetches no page.
    short: Option<usize>,
    durations: Vec<u64>,
    last_failure: Option<LastFailure>,
}

/// Tallies each provider that ran in `records`, oldest first. A provider made one call per
/// record in which it ran, which succeeded when any of its attempts did. Only an attempt's
/// failure counts, and every attempt error kind is of the transport or the content family:
/// forager's own errors (an argument, the configuration, the `--output` copy) end a record
/// without failing an attempt, so they never count.
fn tally(records: Vec<&Record>) -> BTreeMap<ProviderUse, Tally> {
    let mut tallies = BTreeMap::<ProviderUse, Tally>::new();
    for record in records {
        let hidden = record.hidden_failures().collect::<Vec<_>>();
        let mut ran = Vec::<&str>::new();
        for attempt in &record.attempts {
            if attempt.disposition != AttemptDisposition::Skipped
                && !ran.contains(&attempt.provider.as_ref())
            {
                ran.push(&attempt.provider);
            }
        }
        for provider in ran {
            let attempts = record
                .attempts
                .iter()
                .filter(|attempt| attempt.provider == provider)
                .collect::<Vec<_>>();
            let tally = tallies
                .entry(ProviderUse {
                    provider: provider.to_owned(),
                    command: record.command,
                    platform: record.platform,
                    operation: record.operation.as_deref().map(str::to_owned),
                })
                .or_default();
            tally.calls += 1;
            tally
                .durations
                .push(attempts.iter().map(|attempt| attempt.duration_ms).sum());
            if record.command == CommandName::Fetch {
                tally.short.get_or_insert(0);
            }
            if attempts
                .iter()
                .any(|attempt| attempt.disposition == AttemptDisposition::Succeeded)
            {
                tally.successes += 1;
                if record.succeeded.as_deref() == Some(provider) {
                    tally.answers += 1;
                    tally.empty += usize::from(record.result.empty);
                    if let Some(short) = tally.short.as_mut()
                        && record
                            .result
                            .content_chars
                            .is_some_and(|chars| chars < SHORT_CONTENT_CHARS)
                    {
                        *short += 1;
                    }
                }
            } else if let Some(failed) = attempts
                .iter()
                .rev()
                .find(|attempt| attempt.disposition == AttemptDisposition::Failed)
            {
                tally.hidden_failures += usize::from(hidden.contains(&provider));
                tally.last_failure = Some(LastFailure {
                    at: record.recorded_at,
                    error_kind: failed.error_kind,
                    http_status: failed.http_status,
                });
            }
        }
    }
    tallies
}

impl Tally {
    fn success_rate(&self) -> (usize, usize) {
        (self.successes, self.calls)
    }

    fn empty_rate(&self) -> Option<(usize, usize)> {
        (self.answers > 0).then_some((self.empty, self.answers))
    }

    fn short_content_rate(&self) -> Option<(usize, usize)> {
        self.short
            .filter(|_| self.answers > 0)
            .map(|short| (short, self.answers))
    }

    fn health(mut self, usage: ProviderUse, previous_calls: usize) -> ProviderHealth {
        self.durations.sort_unstable();
        ProviderHealth {
            usage,
            calls: self.calls,
            previous_calls,
            success_rate: rate(self.success_rate()),
            hidden_failures: self.hidden_failures,
            empty_rate: self.empty_rate().map(rate),
            short_content_rate: self.short_content_rate().map(rate),
            duration_ms: Durations {
                p50: percentile(&self.durations, 50),
                p95: percentile(&self.durations, 95),
            },
            last_failure: self.last_failure,
        }
    }
}

fn rate((part, whole): (usize, usize)) -> Rate {
    #[expect(
        clippy::cast_precision_loss,
        reason = "counts of calls stay far below the precision of a float"
    )]
    Rate(part as f64 / whole as f64)
}

/// The nearest-rank `percent`-th percentile of `sorted`, which is not empty.
fn percentile(sorted: &[u64], percent: usize) -> u64 {
    sorted[(sorted.len() * percent).div_ceil(100).max(1) - 1]
}

/// Whether the share `current` is worse than `previous` by at least the degradation, where a
/// higher share is worse unless `higher_is_better`.
fn worsened(current: (usize, usize), previous: (usize, usize), higher_is_better: bool) -> bool {
    let ((current, current_of), (previous, previous_of)) = (current, previous);
    let (worse, better) = if higher_is_better {
        (current * previous_of, previous * current_of)
    } else {
        (previous * current_of, current * previous_of)
    };
    let (part, whole) = DEGRADATION;
    better.saturating_sub(worse) * whole >= part * current_of * previous_of
}

fn provider_findings(tally: &Tally, previous: Option<&Tally>) -> Vec<FindingKind> {
    let mut findings = Vec::new();
    let previous_calls = previous.map_or(0, |previous| previous.calls);
    let (below, of) = FAILING_BELOW;
    if tally.calls >= MINIMUM_CALLS && tally.successes * of < below * tally.calls {
        findings.push(FindingKind::Failing {
            success_rate: rate(tally.success_rate()),
            calls: tally.calls,
        });
    }
    if let Some(previous) =
        previous.filter(|previous| previous.calls >= MINIMUM_CALLS && tally.calls >= MINIMUM_CALLS)
    {
        let metrics = [
            (
                Metric::Success,
                Some(tally.success_rate()),
                Some(previous.success_rate()),
                true,
            ),
            (
                Metric::Empty,
                tally.empty_rate(),
                previous.empty_rate(),
                false,
            ),
            (
                Metric::ShortContent,
                tally.short_content_rate(),
                previous.short_content_rate(),
                false,
            ),
        ];
        for (metric, current, before, higher_is_better) in metrics {
            if let (Some(current), Some(before)) = (current, before)
                && worsened(current, before, higher_is_better)
            {
                findings.push(FindingKind::Degraded {
                    metric,
                    current: rate(current),
                    previous: rate(before),
                });
            }
        }
    }
    if tally.hidden_failures > 0 {
        findings.push(FindingKind::HiddenFailures {
            count: tally.hidden_failures,
        });
    }
    if let Some(count) = tally.short.filter(|count| *count > 0) {
        findings.push(FindingKind::ShortContent { count });
    }
    if tally.calls < MINIMUM_CALLS || previous_calls < MINIMUM_CALLS {
        findings.push(FindingKind::SampleTooSmall {
            calls: tally.calls,
            previous_calls,
            minimum: MINIMUM_CALLS,
        });
    }
    findings
}

/// Each scheduled tier, then each GitHub scheduled workflow, with the age of its last run.
fn schedules(evidence: &Evidence<'_>, now: DateTime<Utc>) -> Vec<ScheduleHealth> {
    let tiers = LiveTier::ALL.into_iter().filter_map(|tier| {
        tier.schedule_days().map(|interval_days| {
            ScheduleHealth::of(
                Subject::Tier {
                    tier: tier.as_str(),
                },
                interval_days,
                evidence.scheduled_runs.get(&tier).copied(),
                now,
            )
        })
    });
    let workflows = match evidence.workflows {
        Some(workflows) => workflows
            .iter()
            .map(|workflow| {
                ScheduleHealth::of(
                    Subject::Workflow {
                        workflow: workflow.workflow.clone(),
                    },
                    workflow.interval_days.get(),
                    workflow.last_success,
                    now,
                )
            })
            .collect(),
        None => vec![ScheduleHealth {
            subject: Subject::WorkflowRecord,
            interval_days: None,
            last_run_at: None,
            age_hours: None,
            status: ScheduleStatus::NotRecorded,
        }],
    };
    tiers.chain(workflows).collect()
}

impl ScheduleHealth {
    /// A schedule that runs every `interval_days` and last ran at `last_run_at`, if ever. It is
    /// overdue once its last run is older than its interval and a day.
    fn of(
        subject: Subject,
        interval_days: u32,
        last_run_at: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> Self {
        let age = last_run_at.map(|last| now - last);
        Self {
            subject,
            interval_days: Some(interval_days),
            last_run_at,
            age_hours: age.map(|age| age.num_hours()),
            status: match age {
                None => ScheduleStatus::NotRecorded,
                Some(age) if age > TimeDelta::days(i64::from(interval_days)) + GRACE => {
                    ScheduleStatus::Overdue
                }
                Some(_) => ScheduleStatus::Current,
            },
        }
    }

    fn finding(&self) -> Option<Finding> {
        let kind = match (self.status, self.age_hours, self.interval_days) {
            (ScheduleStatus::Overdue, Some(age_hours), Some(interval_days)) => {
                FindingKind::Overdue {
                    age_hours,
                    interval_days,
                }
            }
            (ScheduleStatus::NotRecorded, ..) => FindingKind::NotRecorded,
            _ => return None,
        };
        Some(Finding {
            subject: self.subject.clone(),
            kind,
        })
    }
}

impl Finding {
    /// Whether the finding is one that the default `doctor` reports: something failing,
    /// degraded or overdue.
    fn needs_attention(&self) -> bool {
        self.kind.attention().is_some()
    }
}

impl FindingKind {
    /// How the attention line states a finding that the default `doctor` reports; `None` for
    /// one only the health summary shows.
    const fn attention(&self) -> Option<&'static str> {
        match self {
            Self::Failing { .. } => Some("failing"),
            Self::Degraded { .. } => Some("degraded"),
            Self::Overdue { .. } => Some("overdue"),
            Self::HiddenFailures { .. }
            | Self::ShortContent { .. }
            | Self::SampleTooSmall { .. }
            | Self::NotRecorded => None,
        }
    }
}

#[cfg(test)]
#[path = "health_tests.rs"]
mod tests;
