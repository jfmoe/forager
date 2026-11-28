//! A directory of timestamped records. Each record is its own private, exclusively created JSON
//! file, named by when it was written, by the process that wrote it and by that process's write
//! sequence, so no two writes share a file. A reader skips a file it cannot parse or does not own.
//! A write's cleanup removes the records older than the kind's retention, at most once a sweep
//! period, because listing the directory on every write costs more as records accumulate. An
//! expired record can therefore stay on disk for up to one sweep period; a reader that applies the
//! retention, as `retained_since` gives it, still skips it.
//! A directory may be shared with other data, so only a regular file whose name fully matches the
//! kind's naming is owned: its prefix, then fields of digits for the time, the process and,
//! optionally, the sequence.

use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, TimeDelta, Utc};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::secure_fs::{create_new_private_file, create_private_file, ensure_private_directory};

/// How long after one sweep for expired records the next is due.
const SWEEP_PERIOD: TimeDelta = TimeDelta::hours(1);

/// What sets one kind of record apart in its directory.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RecordKind {
    /// The start of each record's file name.
    pub(crate) prefix: &'static str,
    /// How long a record is kept after it was written; `None` keeps it indefinitely.
    pub(crate) retention: Option<TimeDelta>,
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
        self.kind
            .retention
            .and_then(|retention| now.checked_sub_signed(retention))
            .unwrap_or(DateTime::<Utc>::MIN_UTC)
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
                let written = self.written_if_owned(&entry)?;
                (written.nanos >= since).then_some((written, entry.path()))
            })
            .filter_map(|(written, path)| {
                let record = serde_json::from_slice::<T>(&fs::read(path).ok()?).ok()?;
                Some((written, record))
            })
            .collect::<Vec<_>>();
        records.sort_by_key(|(written, _)| *written);
        records.into_iter().map(|(_, record)| record).collect()
    }

    /// Writes `record` as written at `at`, returning the record's path.
    pub(crate) fn write<T: Serialize>(&self, at: DateTime<Utc>, record: &T) -> io::Result<PathBuf> {
        ensure_private_directory(&self.path)?;
        let path = self.path.join(format!(
            "{}{}_{}_{}.json",
            self.kind.prefix,
            nanos(at),
            std::process::id(),
            WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut encoded = serde_json::to_vec(record).map_err(io::Error::other)?;
        encoded.push(b'\n');
        let mut file = create_new_private_file(&path)?;
        file.write_all(&encoded)?;
        if self.kind.synced {
            file.sync_all()?;
        }
        Ok(path)
    }

    /// Removes every record written more than the retention before `now`, when no sweep ran in
    /// the sweep period before `now` or the last one is recorded after it, as when the clock went
    /// back. The sweep goes on past a file it cannot remove and returns the first error; a record
    /// another process removed first is no error.
    pub(crate) fn remove_expired_when_due(&self, now: DateTime<Utc>) -> io::Result<()> {
        if self.kind.retention.is_none() {
            return Ok(());
        }
        let marker = self.sweep_marker();
        // Anything but a regular file under the marker's name belongs to someone else, so it is
        // neither read nor touched, and every write sweeps.
        let recorded = match fs::symlink_metadata(&marker) {
            Ok(found) if !found.is_file() => Ok(()),
            Ok(found) if !sweep_due(&found, now) => return Ok(()),
            _ => create_private_file(&marker).and_then(|file| file.set_modified(now.into())),
        };
        self.remove_expired(now).and(recorded)
    }

    /// The file whose modification time records the kind's last sweep; no record name matches it.
    fn sweep_marker(&self) -> PathBuf {
        self.path.join(format!(".{}last_sweep", self.kind.prefix))
    }

    fn remove_expired(&self, now: DateTime<Utc>) -> io::Result<()> {
        let cutoff = nanos(self.retained_since(now));
        let mut first_error = None;
        for entry in fs::read_dir(&self.path)? {
            let removed = entry.and_then(|entry| {
                if self
                    .written_if_owned(&entry)
                    .is_some_and(|written| written.nanos < cutoff)
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

    /// When a record was written, from its name; `None` for an entry this kind does not own.
    fn written_if_owned(&self, entry: &fs::DirEntry) -> Option<Written> {
        let written = self.written(&entry.file_name())?;
        entry
            .file_type()
            .is_ok_and(|kind| kind.is_file())
            .then_some(written)
    }

    fn written(&self, name: &OsStr) -> Option<Written> {
        let mut fields = name
            .to_str()?
            .strip_prefix(self.kind.prefix)?
            .strip_suffix(".json")?
            .split('_')
            .map(|field| {
                (!field.is_empty() && field.bytes().all(|byte| byte.is_ascii_digit()))
                    .then_some(field)
            });
        let nanos = fields.next()??.parse().ok()?;
        let pid = fields.next()??.parse().ok()?;
        let sequence = match fields.next() {
            Some(field) => Some(field?.parse().ok()?),
            None => None,
        };
        fields.next().is_none().then_some(Written {
            nanos,
            pid,
            sequence,
        })
    }
}

/// The next write of this process, which keeps two records written at the same instant apart.
static WRITE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// When, in nanoseconds, by which process and as which of its writes a record was written.
#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
struct Written {
    nanos: i64,
    pid: u32,
    sequence: Option<u64>,
}

/// Whether a sweep is due at `now` after the one `marker` records.
fn sweep_due(marker: &fs::Metadata, now: DateTime<Utc>) -> bool {
    marker.modified().map_or(true, |swept| {
        let swept = DateTime::<Utc>::from(swept);
        swept > now || now - swept >= SWEEP_PERIOD
    })
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
        retention: Some(TimeDelta::days(10)),
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
            .remove_expired_when_due(now())
            .expect("remove the expired records");

        assert_eq!(
            (
                records.read::<Value>(now() - TimeDelta::days(30)),
                directory.path().join("notes.json").exists(),
            ),
            (vec![json!({ "at": now() - TimeDelta::days(10) })], true)
        );
    }

    #[test]
    fn two_records_written_at_the_same_instant_by_one_process_are_both_kept() {
        let directory = tempfile::tempdir().expect("create the record directory");
        let records = RecordDirectory::new(directory.path().to_path_buf(), KIND);

        let paths = [
            records.write(now(), &json!({ "write": 1 })),
            records.write(now(), &json!({ "write": 2 })),
        ]
        .map(|written| written.expect("write a record"));

        assert_eq!(
            (
                paths.map(|path| fs::read(path).expect("read the record")),
                records.read::<Value>(now()).len(),
            ),
            (
                [b"{\"write\":1}\n".to_vec(), b"{\"write\":2}\n".to_vec()],
                2
            )
        );
    }

    #[test]
    fn cleanup_removes_only_expired_regular_files_whose_names_fully_match_the_kind() {
        let directory = tempfile::tempdir().expect("create the record directory");
        let path = directory.path();
        let expired = (now() - TimeDelta::days(11))
            .timestamp_nanos_opt()
            .expect("a time in range");
        let removed = [
            format!("note_{expired}_2_3.json"),
            format!("note_{expired}_2.json"),
        ];
        let kept = [
            format!(
                "note_{}_2_3.json",
                now().timestamp_nanos_opt().expect("a time in range")
            ),
            "note__2_3.json".to_owned(),
            format!("note_{expired}__3.json"),
            format!("note_{expired}_2_.json"),
            format!("note_+{expired}_2.json"),
            format!("note_{expired}_two.json"),
            format!("note_{expired}_2_3_4.json"),
            format!("old_note_{expired}_2.json"),
            format!("note_{expired}_2.json.bak"),
            "notes.json".to_owned(),
        ];
        for name in removed.iter().chain(&kept) {
            fs::write(path.join(name), "{}").expect("write a file");
        }
        let directory_named_as_a_record = path.join(format!("note_{expired}_7.json"));
        fs::create_dir(&directory_named_as_a_record).expect("create a directory");
        #[cfg(unix)]
        let symlink_named_as_a_record = {
            let link = path.join(format!("note_{expired}_8.json"));
            std::os::unix::fs::symlink(path.join("notes.json"), &link).expect("create a symlink");
            link
        };
        let records = RecordDirectory::new(path.to_path_buf(), KIND);

        records
            .remove_expired_when_due(now())
            .expect("remove the expired records");

        let remaining = file_names(path);
        assert_eq!(
            (
                removed
                    .iter()
                    .filter(|name| remaining.contains(name))
                    .count(),
                kept.iter().filter(|name| remaining.contains(name)).count(),
                directory_named_as_a_record.is_dir(),
            ),
            (0, kept.len(), true)
        );
        #[cfg(unix)]
        assert!(symlink_named_as_a_record.symlink_metadata().is_ok());
    }

    #[test]
    fn a_sweep_within_an_hour_of_the_last_leaves_expired_records_and_the_next_removes_them() {
        let directory = tempfile::tempdir().expect("create the record directory");
        let records = RecordDirectory::new(directory.path().to_path_buf(), KIND);
        records
            .remove_expired_when_due(now())
            .expect("sweep the empty directory");
        written(&records, now() - TimeDelta::days(11));
        let remaining = |at| {
            records
                .remove_expired_when_due(at)
                .expect("remove the expired records");
            records.read::<Value>(now() - TimeDelta::days(30)).len()
        };

        assert_eq!(
            [
                remaining(now() + TimeDelta::minutes(59)),
                remaining(now() + TimeDelta::hours(1)),
            ],
            [1, 0]
        );
    }

    #[test]
    fn a_sweep_after_the_clock_went_back_past_the_last_removes_expired_records() {
        let directory = tempfile::tempdir().expect("create the record directory");
        let records = RecordDirectory::new(directory.path().to_path_buf(), KIND);
        records
            .remove_expired_when_due(now())
            .expect("sweep the empty directory");
        let earlier = now() - TimeDelta::days(1);
        written(&records, earlier - TimeDelta::days(11));

        records
            .remove_expired_when_due(earlier)
            .expect("remove the expired records");

        assert_eq!(
            records.read::<Value>(now() - TimeDelta::days(30)),
            Vec::<Value>::new()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_sweep_leaves_the_file_a_link_named_as_its_marker_points_to_untouched() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("create the record directory");
        let path = directory.path();
        let neighbour = path.join("notes.txt");
        fs::write(&neighbour, "the user's own file").expect("write a neighbouring file");
        fs::set_permissions(&neighbour, fs::Permissions::from_mode(0o644)).expect("set its mode");
        let modified = |path: &std::path::Path| fs::metadata(path).expect("stat").modified();
        let before = modified(&neighbour).expect("its modification time");
        std::os::unix::fs::symlink(&neighbour, path.join(".note_last_sweep"))
            .expect("link the marker's name to the neighbour");
        let records = RecordDirectory::new(path.to_path_buf(), KIND);
        written(&records, now() - TimeDelta::days(11));

        records
            .remove_expired_when_due(now())
            .expect("remove the expired records");

        assert_eq!(
            (
                fs::metadata(&neighbour).expect("stat").permissions().mode() & 0o777,
                modified(&neighbour).expect("its modification time"),
                records.read::<Value>(now() - TimeDelta::days(30)).len(),
            ),
            (0o644, before, 0)
        );
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_returns_the_error_of_a_record_it_cannot_remove() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("create the state directory");
        let path = directory.path().join("notes");
        let records = RecordDirectory::new(path.clone(), KIND);
        written(&records, now() - TimeDelta::days(11));
        records
            .remove_expired_when_due(now() - TimeDelta::days(12))
            .expect("sweep before the record expired");
        // A record in a directory the user cannot write to cannot be removed.
        let set_mode = |mode| {
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).expect("set the mode");
        };
        set_mode(0o500);

        let removed = records.remove_expired_when_due(now());

        set_mode(0o700);
        assert_eq!(
            (
                removed.is_err(),
                records.read::<Value>(now() - TimeDelta::days(30)).len()
            ),
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
