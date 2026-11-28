//! Run records: one private file per `doctor --live` run in the state directory, removed after
//! the retention period, each check's transition against the scheduled runs recorded before it,
//! and the notification decision those runs give. A record holds check identifiers, statuses,
//! error kinds, HTTP statuses, durations and contract details, never a message, so no secret,
//! query, URL or content reaches it.

use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};

use super::notification::{self, Notification};
use super::{CheckResult, Outcome, Status, Summary};
use crate::catalog::LiveTier;
use crate::record_directory::{RecordDirectory, RecordKind};

/// The run records' directory under the state directory.
pub(super) const DIRECTORY: &str = "live_runs";
const RECORDS: RecordKind = RecordKind {
    prefix: "live_run_",
    retention: TimeDelta::days(90),
    synced: true,
};

/// Where run records are kept.
pub(super) struct Store {
    records: RecordDirectory,
}

/// Why a run left no record, or left expired ones behind; the run's result does not depend on it.
#[derive(Debug, thiserror::Error)]
pub(super) enum RecordError {
    #[error("no state directory, so the run was not recorded")]
    NoStateDirectory,
    #[error("cannot write the run record: {0}")]
    Write(io::Error),
    #[error("cannot remove expired run records: {0}")]
    RemoveExpired(io::Error),
}

/// What a run record says about the run as a whole.
pub(super) struct Run<'a> {
    pub(super) tiers: &'a [&'static str],
    /// Whether the run was unattended; only such a run is a baseline for later transitions.
    pub(super) scheduled: bool,
    pub(super) summary: &'a Summary,
    /// The identifier of every check this forager declares, selected or not.
    pub(super) declared: &'a [&'static str],
}

#[derive(Serialize)]
struct Record<'a> {
    schema_version: u32,
    run_id: &'a str,
    recorded_at: DateTime<Utc>,
    forager_version: &'static str,
    tiers: &'a [&'static str],
    scheduled: bool,
    summary: &'a Summary,
    declared: &'a [&'static str],
    checks: Vec<RecordedCheck<'a>>,
}

#[derive(Serialize)]
struct RecordedCheck<'a> {
    id: &'static str,
    #[serde(flatten)]
    outcome: &'a Outcome,
}

/// What a transition or a notification decision needs from a record; every other field is
/// ignored.
#[derive(Deserialize)]
pub(super) struct PastRun {
    recorded_at: DateTime<Utc>,
    pub(super) scheduled: bool,
    #[serde(default)]
    tiers: Vec<String>,
    /// The checks the forager that ran it declared; `None` in a record that does not list them.
    #[serde(default)]
    pub(super) declared: Option<Vec<String>>,
    pub(super) checks: Vec<PastCheck>,
}

#[derive(Deserialize)]
pub(super) struct PastCheck {
    pub(super) id: String,
    pub(super) status: Status,
    #[serde(default)]
    pub(super) confirmed: Option<bool>,
    #[serde(default)]
    pub(super) error_kind: Option<String>,
    #[serde(default)]
    pub(super) http_status: Option<u16>,
}

impl PastCheck {
    /// Whether the check reached a result: it passed, or it failed without the run leaving its
    /// failure unconfirmed.
    pub(super) fn concluded(&self) -> bool {
        concluded(self.status, self.confirmed)
    }
}

/// Whether a check that ended as `status` reached a result: it passed, or it failed without the
/// run leaving its failure unconfirmed.
fn concluded(status: Status, confirmed: Option<bool>) -> bool {
    status == Status::Passed || (status == Status::Failed && confirmed != Some(false))
}

/// How a check that ran moved since the most recent scheduled run in which it ran.
#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "transition", rename_all = "snake_case")]
pub(super) enum Transition {
    /// It failed, and it passed or had not run before.
    NewlyFailed,
    /// It failed again; `failing_since` is the first scheduled failure in the unbroken run of
    /// failures.
    StillFailing { failing_since: DateTime<Utc> },
    /// It passed after a failure.
    Recovered,
}

impl Store {
    pub(super) const fn new(directory: PathBuf) -> Self {
        Self {
            records: RecordDirectory::new(directory, RECORDS),
        }
    }

    /// Every readable record still within the retention at `now`, oldest first.
    fn history(&self, now: DateTime<Utc>) -> Vec<PastRun> {
        self.records.read(self.records.retained_since(now))
    }
}

/// What concluding a run gives besides each result's transition.
pub(super) struct Conclusion {
    /// The notification decision of each provider whose issue the run changes; empty for a run
    /// that is not scheduled.
    pub(super) notifications: Vec<Notification>,
    /// Whether the run was recorded and the expired records removed.
    pub(super) record: Result<(), RecordError>,
}

/// Gives each result its transition against the recorded scheduled runs and decides each
/// provider's notification, then records this run at `now` and removes the expired records. The
/// results keep their transitions, and the decision stands, whether or not the record could be
/// written.
pub(super) fn conclude(
    store: &Store,
    now: DateTime<Utc>,
    run: &Run<'_>,
    results: &mut [CheckResult],
) -> Conclusion {
    let history = store.history(now);
    for result in results.iter_mut() {
        let outcome = &result.outcome;
        result.transition = concluded(outcome.status, outcome.confirmed)
            .then(|| transition(&history, result.identity.id, outcome.status))
            .flatten();
    }
    let notifications = notification::decide(
        &history,
        &PastRun {
            recorded_at: now,
            scheduled: run.scheduled,
            tiers: run.tiers.iter().map(|tier| (*tier).to_owned()).collect(),
            declared: Some(run.declared.iter().map(|id| (*id).to_owned()).collect()),
            checks: results
                .iter()
                .map(|result| PastCheck {
                    id: result.identity.id.to_owned(),
                    status: result.outcome.status,
                    confirmed: result.outcome.confirmed,
                    error_kind: result.outcome.error_kind.clone(),
                    http_status: result.outcome.http_status,
                })
                .collect(),
        },
    );
    let millis = now.timestamp_millis();
    let run_id = format!("{millis}-{}", std::process::id());
    let record = Record {
        schema_version: 1,
        run_id: &run_id,
        recorded_at: now,
        forager_version: env!("CARGO_PKG_VERSION"),
        tiers: run.tiers,
        scheduled: run.scheduled,
        summary: run.summary,
        declared: run.declared,
        checks: results
            .iter()
            .map(|result| RecordedCheck {
                id: result.identity.id,
                outcome: &result.outcome,
            })
            .collect(),
    };
    let record = store
        .records
        .write(now, &record)
        .map_err(RecordError::Write)
        .and_then(|()| {
            store
                .records
                .remove_expired(now)
                .map_err(RecordError::RemoveExpired)
        });
    Conclusion {
        notifications,
        record,
    }
}

/// When the latest scheduled run of each tier was recorded, among the records still within the
/// retention at `now`. A run counts for every tier it selected, whatever its checks concluded.
pub(super) fn last_scheduled_runs(
    store: &Store,
    now: DateTime<Utc>,
) -> BTreeMap<LiveTier, DateTime<Utc>> {
    let mut last = BTreeMap::new();
    for run in store.history(now).into_iter().filter(|run| run.scheduled) {
        for tier in run.tiers.iter().filter_map(|tier| LiveTier::parse(tier)) {
            last.insert(tier, run.recorded_at);
        }
    }
    last
}

/// The transition of check `id`, which ended as `status`, against the scheduled runs in
/// `history` in which it reached a result; `None` when it passed as it did before.
fn transition(history: &[PastRun], id: &str, status: Status) -> Option<Transition> {
    let mut ran = history
        .iter()
        .rev()
        .filter(|run| run.scheduled)
        .filter_map(|run| {
            run.checks
                .iter()
                .find(|check| check.id == id)
                .filter(|check| check.concluded())
                .map(|check| (run.recorded_at, check.status))
        })
        .peekable();
    let failed_before = ran.peek().is_some_and(|(_, last)| *last == Status::Failed);
    match status {
        Status::Failed if failed_before => {
            let failing_since = ran
                .take_while(|(_, status)| *status == Status::Failed)
                .last()
                .map(|(recorded_at, _)| recorded_at)?;
            Some(Transition::StillFailing { failing_since })
        }
        Status::Failed => Some(Transition::NewlyFailed),
        Status::Passed if failed_before => Some(Transition::Recovered),
        _ => None,
    }
}

#[cfg(test)]
#[path = "live_record_tests.rs"]
mod tests;
