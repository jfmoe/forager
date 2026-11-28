//! A directory of timestamped records. Each record is its own private, exclusively created JSON
//! file, named by when it was written and by the process that wrote it, so concurrent processes
//! never write the same file. A reader skips a file it cannot parse or does not own, and a write's
//! cleanup removes the records older than the kind's retention.

use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use chrono::{DateTime, TimeDelta, Utc};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::secure_fs::{create_new_private_file, ensure_private_directory};

/// What sets one kind of record apart in its directory.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RecordKind {
    /// The start of each record's file name.
    pub(crate) prefix: &'static str,
    /// How long a record is kept after it was written.
    pub(crate) retention: TimeDelta,
    /// Whether a write returns only once the record has reached the disk.
    pub(crate) synced: bool,
}

/// The records of one kind in one directory.
pub(crate) struct RecordDirectory {
    path: PathBuf,
    kind: RecordKind,
}

impl RecordDirectory {
    pub(crate) const fn new(path: PathBuf, kind: RecordKind) -> Self {
        Self { path, kind }
    }

    /// The time before which a record has expired at `now`.
    pub(crate) fn retained_since(&self, now: DateTime<Utc>) -> DateTime<Utc> {
        now - self.kind.retention
    }

    /// Every readable record written at or after `since`, oldest first; a damaged or foreign file
    /// is skipped.
    pub(crate) fn read<T: DeserializeOwned>(&self, since: DateTime<Utc>) -> Vec<T> {
        let Ok(entries) = fs::read_dir(&self.path) else {
            return Vec::new();
        };
        let since = nanos(since);
        let mut records = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let written = self.written(&entry.file_name())?;
                (written.0 >= since).then_some((written, entry.path()))
            })
            .filter_map(|(written, path)| {
                let record = serde_json::from_slice::<T>(&fs::read(path).ok()?).ok()?;
                Some((written, record))
            })
            .collect::<Vec<_>>();
        records.sort_by_key(|(written, _)| *written);
        records.into_iter().map(|(_, record)| record).collect()
    }

    /// Writes `record` as written at `at`.
    pub(crate) fn write<T: Serialize>(&self, at: DateTime<Utc>, record: &T) -> io::Result<()> {
        ensure_private_directory(&self.path)?;
        let path = self.path.join(format!(
            "{}{}_{}.json",
            self.kind.prefix,
            nanos(at),
            std::process::id()
        ));
        let mut encoded = serde_json::to_vec(record).map_err(io::Error::other)?;
        encoded.push(b'\n');
        let mut file = create_new_private_file(&path)?;
        file.write_all(&encoded)?;
        if self.kind.synced {
            file.sync_all()?;
        }
        Ok(())
    }

    /// Removes every record written more than the retention before `now`, going on past a file
    /// it cannot remove and returning the first such error. A record another process removed
    /// first is no error.
    pub(crate) fn remove_expired(&self, now: DateTime<Utc>) -> io::Result<()> {
        let cutoff = nanos(self.retained_since(now));
        let mut first_error = None;
        for entry in fs::read_dir(&self.path)? {
            let removed = entry.and_then(|entry| {
                if self
                    .written(&entry.file_name())
                    .is_some_and(|(at, _)| at < cutoff)
                {
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

    /// When, in nanoseconds, and by which process a record file was written, from its name;
    /// `None` for a file this kind does not own.
    fn written(&self, name: &OsStr) -> Option<(i64, u32)> {
        let (nanos, pid) = name
            .to_str()?
            .strip_prefix(self.kind.prefix)?
            .strip_suffix(".json")?
            .split_once('_')?;
        Some((nanos.parse().ok()?, pid.parse().ok()?))
    }
}

fn nanos(at: DateTime<Utc>) -> i64 {
    at.timestamp_nanos_opt().unwrap_or(i64::MIN)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use chrono::{DateTime, TimeDelta, Utc};
    use serde_json::{Value, json};

    use super::{RecordDirectory, RecordKind};

    const KIND: RecordKind = RecordKind {
        prefix: "note_",
        retention: TimeDelta::days(10),
        synced: true,
    };

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-28T06:00:00Z")
            .expect("a valid time")
            .with_timezone(&Utc)
    }

    fn written(records: &RecordDirectory, at: DateTime<Utc>) {
        records
            .write(at, &json!({ "at": at }))
            .expect("write a record");
    }

    fn file_names(path: &std::path::Path) -> Vec<String> {
        let mut names = fs::read_dir(path)
            .expect("read the records")
            .map(|entry| {
                entry
                    .expect("a directory entry")
                    .file_name()
                    .into_string()
                    .expect("a file name")
            })
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    fn only_file(path: &std::path::Path) -> PathBuf {
        let [name] = file_names(path).try_into().expect("one record");
        path.join(name)
    }

    #[test]
    fn a_reader_returns_the_records_since_oldest_first_and_skips_damaged_and_foreign_files() {
        let directory = tempfile::tempdir().expect("create the record directory");
        let records = RecordDirectory::new(directory.path().to_path_buf(), KIND);
        // A record cut short, as a crash during an unsynced write leaves it.
        written(&records, now() - TimeDelta::hours(1));
        let damaged = only_file(directory.path());
        let text = fs::read(&damaged).expect("read the record");
        fs::write(&damaged, &text[..text.len() / 2]).expect("cut the record short");
        written(&records, now());
        written(&records, now() - TimeDelta::hours(2));
        written(&records, now() - TimeDelta::days(1));
        fs::write(directory.path().join("notes.json"), "{}").expect("write a foreign file");

        let read = records.read::<Value>(now() - TimeDelta::days(1) + TimeDelta::seconds(1));

        assert_eq!(
            read,
            [
                json!({ "at": now() - TimeDelta::hours(2) }),
                json!({ "at": now() })
            ]
        );
    }

    #[test]
    fn cleanup_removes_only_the_records_older_than_the_retention() {
        let directory = tempfile::tempdir().expect("create the record directory");
        let records = RecordDirectory::new(directory.path().to_path_buf(), KIND);
        written(
            &records,
            now() - TimeDelta::days(10) - TimeDelta::seconds(1),
        );
        written(&records, now() - TimeDelta::days(10));
        fs::write(directory.path().join("notes.json"), "{}").expect("write a foreign file");

        records
            .remove_expired(now())
            .expect("remove the expired records");

        assert_eq!(
            (
                records.read::<Value>(now() - TimeDelta::days(30)),
                file_names(directory.path()).len(),
            ),
            (vec![json!({ "at": now() - TimeDelta::days(10) })], 2)
        );
    }

    #[test]
    fn cleanup_goes_on_past_an_entry_it_cannot_remove_and_returns_its_error() {
        let directory = tempfile::tempdir().expect("create the record directory");
        let records = RecordDirectory::new(directory.path().to_path_buf(), KIND);
        // A directory with a record's name cannot be removed as a file.
        written(&records, now() - TimeDelta::days(12));
        let blocking = only_file(directory.path());
        fs::remove_file(&blocking).expect("remove the record");
        fs::create_dir(&blocking).expect("create a directory named as a record");
        written(&records, now() - TimeDelta::days(11));

        let removed = records.remove_expired(now());

        assert_eq!(
            (removed.is_err(), file_names(directory.path()).len()),
            (true, 1)
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_record_and_its_directory_are_private() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("create the state directory");
        let path = directory.path().join("notes");
        let records = RecordDirectory::new(path.clone(), KIND);

        written(&records, now());

        let mode =
            |path: &std::path::Path| fs::metadata(path).expect("stat").permissions().mode() & 0o777;
        assert_eq!((mode(&path), mode(&only_file(&path))), (0o700, 0o600));
    }
}
