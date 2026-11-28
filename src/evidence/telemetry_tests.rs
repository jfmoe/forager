use std::borrow::Cow;
use std::fs;

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
            true,
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
fn a_reader_skips_a_damaged_record_and_a_file_the_store_does_not_own() {
    let directory = tempfile::tempdir().expect("telemetry directory");
    record_at(directory.path(), now());
    fs::write(
        directory.path().join("invocation_1_1.json"),
        "{\"schema_version\": 1, \"recor",
    )
    .expect("a damaged record");
    fs::write(directory.path().join("notes.json"), "{}").expect("a foreign file");
    record_at(directory.path(), now() + TimeDelta::seconds(1));

    let records = read(directory.path(), now() - TimeDelta::days(1));

    assert_eq!(
        records
            .iter()
            .map(|record| record.recorded_at)
            .collect::<Vec<_>>(),
        [now(), now() + TimeDelta::seconds(1)]
    );
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
