//! Invocation Telemetry: one private record per retrieval command in the state directory, kept
//! for 30 days.
//!
//! A record holds health signals only: the command, its exit code and terminal error kind, the
//! providers that were tried and answered, the shape of the result, and the safe fields of each
//! attempt. Its types have no field for a query, a URL beyond the fetched host, content, a
//! credential, an error message, an endpoint host, a model name or a Platform Ref, and the record
//! is built field by field, never projected from an attempt, so none of them can reach it.

use std::borrow::Cow;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};

use crate::record_directory::{RecordDirectory, RecordKind};
use crate::types::{AttemptDisposition, AttemptErrorKind, Platform, ProviderAttempt};

/// The records' directory under the state directory.
pub(crate) const DIRECTORY: &str = "telemetry";
const RECORDS: RecordKind = RecordKind {
    prefix: "invocation_",
    retention: TimeDelta::days(30),
    // Every retrieval command writes a record, a sync on each would cost more than the record is
    // worth, and a record a crash cuts short is skipped by the reader.
    synced: false,
};
const SCHEMA_VERSION: u32 = 1;

/// A retrieval command: one that reaches a provider for its result and writes a record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetrievalCommand {
    /// `search`.
    Search,
    /// `fetch`.
    Fetch,
    /// `map`.
    Map,
    /// An `exa` command, by its operation.
    Exa(&'static str),
    /// A `platform <id> <op>` command.
    Platform(Platform, &'static str),
}

/// What one invocation leaves for its record besides its command and exit code.
#[derive(Debug, Default)]
pub struct Invocation {
    error_kind: Option<TerminalKind>,
    first_choice: Option<&'static str>,
    succeeded: Option<&'static str>,
    fallback: bool,
    result: ResultSignal,
    host: Option<String>,
    attempts: Vec<RecordedAttempt>,
}

/// The shape of a successful result: how many items it holds and how long its content is, when
/// it has them, and whether it was a legitimate empty result.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ResultSignal {
    pub(crate) items: Option<usize>,
    pub(crate) content_chars: Option<usize>,
    pub(crate) empty: bool,
}

impl ResultSignal {
    /// A result of `count` items.
    pub(crate) const fn items(count: usize) -> Self {
        Self {
            items: Some(count),
            content_chars: None,
            empty: count == 0,
        }
    }

    /// A result whose content is `content`.
    pub(crate) fn content(content: &str) -> Self {
        Self {
            items: None,
            content_chars: Some(content.chars().count()),
            empty: false,
        }
    }

    /// The same result, which also lists `count` items beside its content.
    pub(crate) const fn with_items(self, count: usize) -> Self {
        Self {
            items: Some(count),
            ..self
        }
    }
}

/// Why an invocation ended with a failure: an attempt's error kind, or an error of forager's own.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub(crate) enum TerminalKind {
    Attempt(AttemptErrorKind),
    Local(LocalKind),
}

/// An error of forager's own, which no provider caused.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LocalKind {
    /// An argument error before any request.
    Argument,
    /// A configuration error before any request.
    Config,
    /// The `--output` copy of a result could not be written.
    Output,
}

impl Invocation {
    /// An invocation that answered with `result` from `provider` after `attempts`. It fell back
    /// when another provider ran and failed before `provider` answered; a route that was skipped
    /// never ran, so it does not count.
    pub(crate) fn succeeded(
        attempts: &[ProviderAttempt],
        provider: &'static str,
        result: ResultSignal,
    ) -> Self {
        let invocation = Self::from_attempts(attempts);
        let fallback = failed_before_answer(&invocation.attempts, provider)
            .next()
            .is_some();
        Self {
            succeeded: Some(provider),
            fallback,
            result,
            ..invocation
        }
    }

    /// An invocation that failed with `kind` after `attempts`.
    pub(crate) fn failed(attempts: &[ProviderAttempt], kind: AttemptErrorKind) -> Self {
        Self {
            error_kind: Some(TerminalKind::Attempt(kind)),
            ..Self::from_attempts(attempts)
        }
    }

    /// An invocation that ended with `kind` before any attempt.
    pub(crate) fn preflight(kind: TerminalKind) -> Self {
        Self {
            error_kind: Some(kind),
            ..Self::default()
        }
    }

    /// The same invocation, whose `--output` copy could not be written. A run that failed keeps
    /// the kind of its failure; one that answered ends with [`LocalKind::Output`] and still names
    /// the provider that answered.
    #[must_use]
    pub fn with_unwritten_output(self) -> Self {
        Self {
            error_kind: self
                .error_kind
                .or(Some(TerminalKind::Local(LocalKind::Output))),
            ..self
        }
    }

    /// The same invocation, which fetched from `host`.
    pub(crate) fn with_host(self, host: Option<String>) -> Self {
        Self { host, ..self }
    }

    fn from_attempts(attempts: &[ProviderAttempt]) -> Self {
        Self {
            first_choice: attempts
                .iter()
                .find(|attempt| attempt.disposition != AttemptDisposition::Skipped)
                .map(|attempt| attempt.provider),
            attempts: attempts.iter().map(RecordedAttempt::of).collect(),
            ..Self::default()
        }
    }
}

/// One stored record.
#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Record {
    pub(crate) schema_version: u32,
    pub(crate) recorded_at: DateTime<Utc>,
    pub(crate) command: CommandName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) platform: Option<Platform>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) operation: Option<Cow<'static, str>>,
    pub(crate) exit_code: u8,
    pub(crate) error_kind: Option<TerminalKind>,
    /// The provider of the first attempt that ran.
    pub(crate) first_choice: Option<Cow<'static, str>>,
    /// The provider whose answer the result carries.
    pub(crate) succeeded: Option<Cow<'static, str>>,
    /// Whether another provider ran and failed before the one that answered.
    pub(crate) fallback: bool,
    pub(crate) result: ResultSignal,
    /// The host a `fetch` read from, without userinfo or port.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) host: Option<String>,
    pub(crate) attempts: Vec<RecordedAttempt>,
}

/// The command of a record, without its platform and operation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CommandName {
    Search,
    Fetch,
    Map,
    Exa,
    Platform,
}

/// The safe fields of one attempt.
#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct RecordedAttempt {
    pub(crate) provider: Cow<'static, str>,
    pub(crate) disposition: AttemptDisposition,
    pub(crate) error_kind: Option<AttemptErrorKind>,
    pub(crate) http_status: Option<u16>,
    pub(crate) duration_ms: u64,
    pub(crate) anonymous: bool,
    pub(crate) retry_count: usize,
    pub(crate) rotation_count: usize,
    pub(crate) credential_index: usize,
}

/// The attempts of providers other than `provider` that ran and failed before `provider`
/// answered. A route that was skipped never ran, so none of its attempts is among them.
fn failed_before_answer<'a>(
    attempts: &'a [RecordedAttempt],
    provider: &'a str,
) -> impl Iterator<Item = &'a RecordedAttempt> {
    let answered = attempts
        .iter()
        .position(|attempt| {
            attempt.provider == provider && attempt.disposition == AttemptDisposition::Succeeded
        })
        .unwrap_or(attempts.len());
    attempts[..answered].iter().filter(move |attempt| {
        attempt.provider != provider && attempt.disposition == AttemptDisposition::Failed
    })
}

impl RecordedAttempt {
    fn of(attempt: &ProviderAttempt) -> Self {
        Self {
            provider: Cow::Borrowed(attempt.provider),
            disposition: attempt.disposition,
            error_kind: attempt.error_kind,
            http_status: attempt.http_status,
            duration_ms: attempt.duration_ms,
            anonymous: attempt.anonymous,
            retry_count: attempt.retry_count,
            rotation_count: attempt.rotation_count,
            credential_index: attempt.credential_index,
        }
    }
}

impl Record {
    pub(crate) fn new(
        command: RetrievalCommand,
        invocation: Invocation,
        exit_code: u8,
        now: DateTime<Utc>,
    ) -> Self {
        let (command, platform, operation) = match command {
            RetrievalCommand::Search => (CommandName::Search, None, None),
            RetrievalCommand::Fetch => (CommandName::Fetch, None, None),
            RetrievalCommand::Map => (CommandName::Map, None, None),
            RetrievalCommand::Exa(operation) => (CommandName::Exa, None, Some(operation)),
            RetrievalCommand::Platform(platform, operation) => {
                (CommandName::Platform, Some(platform), Some(operation))
            }
        };
        Self {
            schema_version: SCHEMA_VERSION,
            recorded_at: now,
            command,
            platform,
            operation: operation.map(Cow::Borrowed),
            exit_code,
            error_kind: invocation.error_kind,
            first_choice: invocation.first_choice.map(Cow::Borrowed),
            succeeded: invocation.succeeded.map(Cow::Borrowed),
            fallback: invocation.fallback,
            result: invocation.result,
            host: invocation.host,
            attempts: invocation.attempts,
        }
    }

    /// The providers whose attempts failed before the provider that answered, a failure the
    /// fallback hid; none when the invocation did not answer.
    pub(crate) fn hidden_failures(&self) -> impl Iterator<Item = &str> {
        self.succeeded.iter().flat_map(|provider| {
            failed_before_answer(&self.attempts, provider).map(|attempt| attempt.provider.as_ref())
        })
    }
}

/// Why a record was not written; the command's output and exit code never depend on it.
#[derive(Debug, thiserror::Error)]
pub(crate) enum TelemetryError {
    #[error("cannot write the telemetry record: {0}")]
    Write(io::Error),
    #[error("cannot remove expired telemetry records: {0}")]
    RemoveExpired(io::Error),
}

/// The records' directory, when the state directory resolves.
pub(crate) fn directory() -> Option<PathBuf> {
    crate::state_file::state_directory().map(|state| state.join(DIRECTORY))
}

/// Writes the record of one invocation of `command` that ended with `exit_code` into
/// `directory` at `now`, then removes the records that expired.
pub(crate) fn record(
    directory: &Path,
    command: RetrievalCommand,
    invocation: Invocation,
    exit_code: u8,
    now: DateTime<Utc>,
) -> Result<(), TelemetryError> {
    let records = RecordDirectory::new(directory.to_path_buf(), RECORDS);
    let record = Record::new(command, invocation, exit_code, now);
    records
        .write(record.recorded_at, &record)
        .map_err(TelemetryError::Write)?;
    records
        .remove_expired(now)
        .map_err(TelemetryError::RemoveExpired)
}

/// Every readable record in `directory` written at or after `since`, oldest first; a damaged or
/// foreign file is skipped.
pub(crate) fn read(directory: &Path, since: DateTime<Utc>) -> Vec<Record> {
    RecordDirectory::new(directory.to_path_buf(), RECORDS).read(since)
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use chrono::{DateTime, TimeDelta, Utc};

    use super::{Invocation, RecordedAttempt, ResultSignal, RetrievalCommand, read, record};
    use crate::chain::skipped_attempt;
    use crate::types::{
        AttemptDisposition, AttemptErrorKind, AttemptTarget, Platform, ProviderAttempt,
    };

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-28T06:00:00Z")
            .expect("a valid time")
            .with_timezone(&Utc)
    }

    fn attempt(provider: &'static str, kind: Option<AttemptErrorKind>) -> ProviderAttempt {
        ProviderAttempt {
            provider,
            target: AttemptTarget::platform("youtube", "comments"),
            disposition: if kind.is_some() {
                AttemptDisposition::Failed
            } else {
                AttemptDisposition::Succeeded
            },
            error_kind: kind,
            http_status: Some(if kind.is_some() { 503 } else { 200 }),
            duration_ms: 40,
            credential_index: 1,
            retry_count: 1,
            rotation_count: 0,
            anonymous: false,
            message: "upstream said no".into(),
            model: Some("a-model".into()),
            transport: Some("http"),
            endpoint_host: Some("gateway.example".into()),
        }
    }

    /// Records one invocation of YouTube comments in `directory` at `at`.
    fn record_at(directory: &std::path::Path, at: DateTime<Utc>) {
        record(
            directory,
            RetrievalCommand::Platform(Platform::Youtube, "comments"),
            Invocation::failed(&[], AttemptErrorKind::Network),
            4,
            at,
        )
        .expect("record written");
    }

    #[test]
    fn a_record_names_the_first_provider_that_ran_the_one_that_answered_and_each_attempt() {
        let directory = tempfile::tempdir().expect("telemetry directory");
        let attempts = [
            skipped_attempt(
                AttemptTarget::platform("youtube", "comments"),
                "youtube_web",
                "not configured",
            ),
            attempt("youtube_api", Some(AttemptErrorKind::Network)),
            attempt("youtube_api", None),
        ];

        record(
            directory.path(),
            RetrievalCommand::Platform(Platform::Youtube, "comments"),
            Invocation::succeeded(&attempts, "youtube_api", ResultSignal::items(0)),
            0,
            now(),
        )
        .expect("record written");
        let records = read(directory.path(), now());

        let [record] = records.as_slice() else {
            panic!("one record, found {records:?}");
        };
        assert_eq!(
            (
                record.platform,
                record.operation.as_deref(),
                record.first_choice.as_deref(),
                record.succeeded.as_deref(),
                record.fallback,
                record.result,
                &record.attempts[1],
            ),
            (
                Some(Platform::Youtube),
                Some("comments"),
                Some("youtube_api"),
                Some("youtube_api"),
                false,
                ResultSignal {
                    items: Some(0),
                    content_chars: None,
                    empty: true,
                },
                &RecordedAttempt {
                    provider: Cow::Borrowed("youtube_api"),
                    disposition: AttemptDisposition::Failed,
                    error_kind: Some(AttemptErrorKind::Network),
                    http_status: Some(503),
                    duration_ms: 40,
                    anonymous: false,
                    retry_count: 1,
                    rotation_count: 0,
                    credential_index: 1,
                },
            )
        );
    }

    #[test]
    fn a_run_fell_back_only_when_another_provider_ran_and_failed_before_the_answer() {
        let skipped = || {
            skipped_attempt(
                AttemptTarget::platform("youtube", "comments"),
                "youtube_web",
                "not configured",
            )
        };
        let cases = [
            (
                "a skipped route and a retried provider",
                vec![
                    skipped(),
                    attempt("youtube_api", Some(AttemptErrorKind::Network)),
                    attempt("youtube_api", None),
                ],
                false,
            ),
            (
                "another provider that failed",
                vec![
                    attempt("youtube_web", Some(AttemptErrorKind::Network)),
                    attempt("youtube_api", None),
                ],
                true,
            ),
            (
                "another provider that answered an earlier stage",
                vec![attempt("youtube_web", None), attempt("youtube_api", None)],
                false,
            ),
            (
                "another provider that failed after the answer",
                vec![
                    attempt("youtube_api", None),
                    attempt("youtube_web", Some(AttemptErrorKind::Network)),
                ],
                false,
            ),
        ];

        for (case, attempts, fallback) in cases {
            assert_eq!(
                Invocation::succeeded(&attempts, "youtube_api", ResultSignal::items(1)).fallback,
                fallback,
                "{case}"
            );
        }
    }

    #[test]
    fn writing_a_record_removes_the_records_older_than_thirty_days() {
        let directory = tempfile::tempdir().expect("telemetry directory");
        let expired = now() - TimeDelta::days(30) - TimeDelta::seconds(1);
        let kept = now() - TimeDelta::days(29);
        record_at(directory.path(), expired);
        record_at(directory.path(), kept);

        record_at(directory.path(), now());

        assert_eq!(
            read(directory.path(), expired - TimeDelta::days(1))
                .iter()
                .map(|record| record.recorded_at)
                .collect::<Vec<_>>(),
            [kept, now()]
        );
    }
}
