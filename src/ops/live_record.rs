//! Run records: one private file per `doctor --live` run in the state directory, removed after
//! the retention period, each check's transition against the scheduled runs recorded before it,
//! and the notification decision those runs give. A record holds check identifiers, statuses,
//! error kinds, HTTP statuses, durations and contract details, never a message, so no secret,
//! query, URL or content reaches it.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};

use super::notification::{self, Notification};
use super::{CheckResult, Outcome, Status, Summary};
use crate::catalog::LiveTier;
use crate::secure_fs::{create_new_private_file, ensure_private_directory};

/// The run records' directory under the state directory.
pub(super) const DIRECTORY: &str = "live_runs";
const PREFIX: &str = "live_run_";
const RETENTION: TimeDelta = TimeDelta::days(90);

/// Where run records are kept.
pub(super) struct Store {
    directory: PathBuf,
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
        Self { directory }
    }

    /// Every readable record still within the retention at `now`, oldest first; a damaged or
    /// foreign file is skipped.
    fn history(&self, now: DateTime<Utc>) -> Vec<PastRun> {
        let Ok(entries) = fs::read_dir(&self.directory) else {
            return Vec::new();
        };
        let cutoff = expiry(now);
        let mut runs = entries
            .filter_map(Result::ok)
            .filter(|entry| {
                recorded_millis(&entry.file_name()).is_some_and(|millis| millis >= cutoff)
            })
            .filter_map(|entry| fs::read(entry.path()).ok())
            .filter_map(|bytes| serde_json::from_slice::<PastRun>(&bytes).ok())
            .collect::<Vec<_>>();
        runs.sort_by_key(|run| run.recorded_at);
        runs
    }

    fn write(&self, record: &Record<'_>, millis: i64) -> io::Result<()> {
        ensure_private_directory(&self.directory)?;
        let path = self
            .directory
            .join(format!("{PREFIX}{millis}_{}.json", std::process::id()));
        let encoded = serde_json::to_vec(record).map_err(io::Error::other)?;
        let mut file = create_new_private_file(&path)?;
        file.write_all(&encoded)?;
        file.write_all(b"\n")?;
        file.sync_all()
    }

    /// Removes every record written more than the retention before `now`, going on past a file
    /// it cannot remove and returning the first such error. A record another run removed first is
    /// no error.
    fn remove_expired(&self, now: DateTime<Utc>) -> io::Result<()> {
        let cutoff = expiry(now);
        let mut first_error = None;
        for entry in fs::read_dir(&self.directory)? {
            let removed = entry.and_then(|entry| {
                if recorded_millis(&entry.file_name()).is_some_and(|millis| millis < cutoff) {
                    fs::remove_file(entry.path())
                } else {
                    Ok(())
                }
            });
            match removed {
                Err(error) if error.kind() != io::ErrorKind::NotFound => {
                    first_error.get_or_insert(error);
                }
                _ => {}
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

/// The time, in milliseconds, before which a record has expired at `now`.
fn expiry(now: DateTime<Utc>) -> i64 {
    (now - RETENTION).timestamp_millis()
}

/// When a record file was written, from its name; `None` for a file the store does not own.
fn recorded_millis(name: &std::ffi::OsStr) -> Option<i64> {
    let (millis, pid) = name
        .to_str()?
        .strip_prefix(PREFIX)?
        .strip_suffix(".json")?
        .split_once('_')?;
    pid.parse::<u32>().ok()?;
    millis.parse().ok()
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
        checks: results
            .iter()
            .map(|result| RecordedCheck {
                id: result.identity.id,
                outcome: &result.outcome,
            })
            .collect(),
    };
    let record = store
        .write(&record, millis)
        .map_err(RecordError::Write)
        .and_then(|()| {
            store
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
