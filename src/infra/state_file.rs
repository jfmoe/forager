//! Private state files that forager processes share through a bounded exclusive file lock.
//!
//! State lives in `$XDG_STATE_HOME/forager`, or `$HOME/.local/state/forager` when
//! `XDG_STATE_HOME` is not an absolute path. Callers hold a [`StateLock`] while they read
//! and atomically replace a state file, and run that blocking work through
//! [`serialized_blocking`]. A lock can also be held across async work through
//! [`acquire_any_lock`].

use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use fs2::FileExt;
use serde_json::Value;
use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::secure_fs::{create_private_file, ensure_private_directory};
use crate::types::Deadline;

const ASYNC_LOCK_POLL: Duration = Duration::from_millis(20);

// Advisory file locks do not reliably exclude other handles in the same process, so work
// on each state file is also serialized in-process.
static STATE_WORK: LazyLock<std::sync::Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> =
    LazyLock::new(Default::default);

pub(crate) fn state_directory() -> Option<PathBuf> {
    absolute_env("XDG_STATE_HOME")
        .map(|path| path.join("forager"))
        .or_else(|| absolute_env("HOME").map(|path| path.join(".local/state/forager")))
}

fn absolute_env(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

/// An exclusive lock on a private lock file; dropping it releases the lock.
pub(crate) struct StateLock {
    file: File,
}

impl StateLock {
    /// Restricts the lock file's directory, then waits at most `wait` for the lock.
    ///
    /// # Errors
    ///
    /// Returns `WouldBlock` when another holder keeps the lock for longer than `wait`, or
    /// another I/O error when the directory or lock file is unusable.
    pub(crate) fn acquire(path: &Path, wait: Duration) -> io::Result<Self> {
        let file = open_lock_file(path)?;
        let deadline = Instant::now() + wait;
        loop {
            match file.try_lock_exclusive() {
                Ok(()) => return Ok(Self { file }),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(io::Error::new(
                            io::ErrorKind::WouldBlock,
                            format!("{} lock timed out", file_stem(path)),
                        ));
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => return Err(error),
            }
        }
    }
}

impl Drop for StateLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn open_lock_file(path: &Path) -> io::Result<File> {
    ensure_private_directory(parent(path)?)?;
    create_private_file(path)
}

/// Waits within `deadline`, without blocking the runtime, for the first free lock among
/// `paths`. Returns `None` when every lock stays held until the deadline.
///
/// # Errors
///
/// Returns an I/O error when a lock file's directory or the lock file is unusable.
pub(crate) async fn acquire_any_lock(
    paths: Vec<PathBuf>,
    deadline: Deadline,
) -> io::Result<Option<StateLock>> {
    let mut files = tokio::task::spawn_blocking(move || {
        paths
            .iter()
            .map(|path| open_lock_file(path))
            .collect::<io::Result<Vec<_>>>()
    })
    .await
    .map_err(io::Error::other)??;
    loop {
        let mut free = None;
        for (index, file) in files.iter().enumerate() {
            match file.try_lock_exclusive() {
                Ok(()) => {
                    free = Some(index);
                    break;
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error),
            }
        }
        if let Some(index) = free {
            return Ok(Some(StateLock {
                file: files.swap_remove(index),
            }));
        }
        let Some(remaining) = deadline
            .remaining()
            .filter(|remaining| !remaining.is_zero())
        else {
            return Ok(None);
        };
        tokio::time::sleep(remaining.min(ASYNC_LOCK_POLL)).await;
    }
}

/// Reads a state file, returning `None` when it does not exist yet.
pub(crate) fn read_state(path: &Path) -> io::Result<Option<String>> {
    let mut content = String::new();
    match File::open(path) {
        Ok(mut file) => {
            file.read_to_string(&mut content)?;
            Ok(Some(content))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Replaces a state file through a synced private temporary file and an atomic rename.
pub(crate) fn write_state(path: &Path, state: &Value) -> io::Result<()> {
    let temporary = parent(path)?.join(format!(".{}.{}.tmp", file_stem(path), std::process::id()));
    let result = (|| {
        let mut file = create_private_file(&temporary)?;
        file.set_len(0)?;
        serde_json::to_writer(&mut file, state).map_err(io::Error::other)?;
        file.write_all(b"\n")?;
        file.flush()?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        drop(create_private_file(path)?);
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Runs blocking state work on the blocking pool, one unit of work at a time per process.
///
/// Cancelling the returned future does not release the in-process serialization until the
/// blocking work finishes.
pub(crate) async fn serialized_blocking<T, F>(
    path: &Path,
    work: F,
) -> Result<T, tokio::task::JoinError>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    run_serialized(state_work_turn(path).await, work).await
}

/// The exclusive in-process turn to run blocking work on one state file.
pub(crate) struct StateWorkTurn {
    _guard: OwnedMutexGuard<()>,
}

/// Waits for the in-process turn on `path`, so a caller can bound the wait before any work
/// starts.
pub(crate) async fn state_work_turn(path: &Path) -> StateWorkTurn {
    let mutex = Arc::clone(
        STATE_WORK
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(path.to_path_buf())
            .or_default(),
    );
    StateWorkTurn {
        _guard: mutex.lock_owned().await,
    }
}

/// Runs blocking state work while holding `turn`; see [`serialized_blocking`].
pub(crate) async fn run_serialized<T, F>(
    turn: StateWorkTurn,
    work: F,
) -> Result<T, tokio::task::JoinError>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let _turn = turn;
        work()
    })
    .await
}

fn parent(path: &Path) -> io::Result<&Path> {
    path.parent()
        .ok_or_else(|| io::Error::other("state path has no parent"))
}

fn file_stem(path: &Path) -> String {
    path.file_stem().map_or_else(
        || "state".into(),
        |stem| stem.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::serialized_blocking;

    #[test]
    fn cancelling_a_waiter_keeps_blocking_state_work_serialized() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_time()
            .build()
            .expect("build test runtime");

        let path = Path::new("/state/serialization-test.json");
        runtime.block_on(async {
            let (first_entered_tx, first_entered_rx) = mpsc::channel();
            let (release_first_tx, release_first_rx) = mpsc::channel();
            let first = tokio::spawn(serialized_blocking(path, move || {
                first_entered_tx.send(()).expect("signal first work");
                release_first_rx.recv().expect("release first work");
            }));
            first_entered_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("first work entered the blocking pool");
            first.abort();

            let (second_entered_tx, second_entered_rx) = mpsc::channel();
            let second = tokio::spawn(serialized_blocking(path, move || {
                second_entered_tx.send(()).expect("signal second work");
            }));
            assert!(
                second_entered_rx
                    .recv_timeout(Duration::from_millis(50))
                    .is_err(),
                "cancelling the waiter released the in-process serialization"
            );

            release_first_tx.send(()).expect("release first work");
            second
                .await
                .expect("join second work")
                .expect("second work");
        });
    }
}
