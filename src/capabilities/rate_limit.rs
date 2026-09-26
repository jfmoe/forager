//! Cross-process request pacing for provider endpoints.
//!
//! Each send reserves a window under the shared state lock: the next window is the later of
//! now and the previous reservation plus the minimum interval. A window that does not fit the
//! remaining budget fails with `Timeout` and is not reserved. When the lock or state is
//! unavailable, the send fails with `Runtime`; unlike the credential pool, pacing never falls
//! back to optimistic sending.
//!
//! Known limits: spacing holds between reservation times, so a process delayed after it
//! reserves can send closer than the interval; coordination covers processes that share one
//! state directory; `max_concurrency` applies within one process only.
#![expect(dead_code, reason = "platform routes declare access policies in #165")]

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use thiserror::Error;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::Instant;

use crate::state_file::{self, StateLock, serialized_blocking};
use crate::types::{AttemptErrorKind, Deadline};

const STATE_FILE: &str = "rate_limit_state.json";
const STATE_SCHEMA_VERSION: u64 = 1;
const LOCK_WAIT: Duration = Duration::from_millis(100);

/// The request pacing a route declares for its endpoint.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AccessPolicy {
    pub(crate) min_interval: Duration,
    pub(crate) max_concurrency: usize,
}

#[derive(Debug, Error)]
pub(crate) enum RateLimitError {
    #[error("{route} request window does not fit the remaining time budget")]
    Timeout { route: &'static str },
    #[error("{route} rate limit state is unavailable: {reason}")]
    Unavailable { route: &'static str, reason: String },
}

impl RateLimitError {
    pub(crate) fn kind(&self) -> AttemptErrorKind {
        match self {
            Self::Timeout { .. } => AttemptErrorKind::Timeout,
            Self::Unavailable { .. } => AttemptErrorKind::Runtime,
        }
    }
}

/// Holds one of the route's in-process concurrency slots until the send completes.
pub(crate) struct RatePermit {
    _permit: OwnedSemaphorePermit,
}

/// Paces sends to one route. Clones share the in-process concurrency limit, so build one
/// limiter per route per process.
#[derive(Clone)]
pub(crate) struct RateLimiter {
    route: &'static str,
    min_interval: Duration,
    state_file: Option<PathBuf>,
    concurrency: Arc<Semaphore>,
    clock: WallClock,
}

impl RateLimiter {
    pub(crate) fn new(route: &'static str, policy: AccessPolicy) -> Self {
        Self::with_state(
            route,
            policy,
            state_file::state_directory().map(|directory| directory.join(STATE_FILE)),
            WallClock::new(),
        )
    }

    fn with_state(
        route: &'static str,
        policy: AccessPolicy,
        state_file: Option<PathBuf>,
        clock: WallClock,
    ) -> Self {
        Self {
            route,
            min_interval: policy.min_interval,
            state_file,
            concurrency: Arc::new(Semaphore::new(policy.max_concurrency)),
            clock,
        }
    }

    /// Waits for a concurrency slot and the next request window within `deadline`.
    ///
    /// # Errors
    ///
    /// Returns [`RateLimitError::Timeout`] when the slot or window does not fit the remaining
    /// budget, and [`RateLimitError::Unavailable`] when the shared state cannot be locked,
    /// read, or written. In both cases the caller must not send.
    pub(crate) async fn acquire(&self, deadline: Deadline) -> Result<RatePermit, RateLimitError> {
        let permit = self.acquire_slot(deadline).await?;
        let wait = self.reserve_window(deadline).await?;
        tokio::time::sleep(wait).await;
        Ok(RatePermit { _permit: permit })
    }

    async fn acquire_slot(
        &self,
        deadline: Deadline,
    ) -> Result<OwnedSemaphorePermit, RateLimitError> {
        let remaining = deadline.remaining().ok_or_else(|| self.timeout())?;
        match tokio::time::timeout(remaining, Arc::clone(&self.concurrency).acquire_owned()).await {
            Ok(Ok(permit)) => Ok(permit),
            Ok(Err(error)) => Err(self.unavailable(error.to_string())),
            Err(_) => Err(self.timeout()),
        }
    }

    async fn reserve_window(&self, deadline: Deadline) -> Result<Duration, RateLimitError> {
        let Some(state_file) = self.state_file.clone() else {
            return Err(self
                .unavailable("XDG_STATE_HOME and HOME do not resolve to an absolute path".into()));
        };
        let (route, min_interval, clock) = (self.route, self.min_interval, self.clock);
        // The window check runs under the lock against the same deadline, so time spent
        // waiting for the lock counts toward the budget and a late result reserves nothing.
        let reservation = serialized_blocking(move || {
            reserve_under_lock(&state_file, route, min_interval, clock, deadline)
        })
        .await;
        match reservation {
            Ok(Ok(Some(wait))) => Ok(wait),
            Ok(Ok(None)) => Err(self.timeout()),
            Ok(Err(error)) => Err(self.unavailable(error.to_string())),
            Err(error) => Err(self.unavailable(error.to_string())),
        }
    }

    fn timeout(&self) -> RateLimitError {
        RateLimitError::Timeout { route: self.route }
    }

    fn unavailable(&self, reason: String) -> RateLimitError {
        RateLimitError::Unavailable {
            route: self.route,
            reason,
        }
    }
}

/// Reserves the route's next window and returns the wait before it, or `None` without
/// reserving when the wait leaves no budget to send.
fn reserve_under_lock(
    path: &Path,
    route: &str,
    min_interval: Duration,
    clock: WallClock,
    deadline: Deadline,
) -> io::Result<Option<Duration>> {
    let _lock = StateLock::acquire(&path.with_extension("lock"), LOCK_WAIT)?;
    let now = clock.now_ms();
    // An unreadable reservation counts as one made now, so repairing it never shortens the
    // interval.
    let (mut state, last_reserved) =
        match state_file::read_state(path)?.map(|content| parse_state(&content)) {
            None => (empty_state(), None),
            Some(None) => (empty_state(), Some(now)),
            Some(Some(state)) => {
                let last_reserved = state["routes"]
                    .get(route)
                    .map(|entry| entry["reserved_at_ms"].as_u64().unwrap_or(now));
                (state, last_reserved)
            }
        };
    let window = next_window(now, last_reserved, min_interval);
    let wait = Duration::from_millis(window - now);
    if deadline
        .remaining()
        .is_none_or(|remaining| wait >= remaining)
    {
        return Ok(None);
    }
    state["routes"][route] = json!({"reserved_at_ms": window});
    state_file::write_state(path, &state)?;
    Ok(Some(wait))
}

fn next_window(now_ms: u64, last_reserved_ms: Option<u64>, min_interval: Duration) -> u64 {
    let interval_ms = u64::try_from(min_interval.as_millis()).unwrap_or(u64::MAX);
    last_reserved_ms.map_or(now_ms, |last| now_ms.max(last.saturating_add(interval_ms)))
}

fn parse_state(content: &str) -> Option<Value> {
    let state = serde_json::from_str::<Value>(content).ok()?;
    (state["schema_version"].as_u64() == Some(STATE_SCHEMA_VERSION) && state["routes"].is_object())
        .then_some(state)
}

fn empty_state() -> Value {
    json!({
        "schema_version": STATE_SCHEMA_VERSION,
        "routes": {}
    })
}

/// Wall-clock milliseconds that advance with the Tokio clock, so reservations share one
/// timeline with deadlines and waits.
#[derive(Clone, Copy)]
struct WallClock {
    origin_ms: u64,
    origin: Instant,
}

impl WallClock {
    fn new() -> Self {
        let since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        Self {
            origin_ms: u64::try_from(since_epoch.as_millis()).unwrap_or(u64::MAX),
            origin: Instant::now(),
        }
    }

    fn now_ms(self) -> u64 {
        let elapsed = u64::try_from(self.origin.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.origin_ms.saturating_add(elapsed)
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{self, OpenOptions};
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use fs2::FileExt;
    use tempfile::tempdir;
    use tokio::time::Instant;

    use super::{AccessPolicy, RateLimiter, WallClock};
    use crate::types::{AttemptErrorKind, Deadline};

    const INTERVAL: Duration = Duration::from_secs(3);

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .start_paused(true)
            .build()
            .expect("test runtime")
    }

    fn state_file(directory: &Path) -> PathBuf {
        directory.join("forager/rate_limit_state.json")
    }

    fn limiter(state_file: Option<PathBuf>) -> RateLimiter {
        limiter_with_clock(state_file, WallClock::new())
    }

    fn limiter_with_clock(state_file: Option<PathBuf>, clock: WallClock) -> RateLimiter {
        RateLimiter::with_state(
            "test_route",
            AccessPolicy {
                min_interval: INTERVAL,
                max_concurrency: 1,
            },
            state_file,
            clock,
        )
    }

    fn budget(seconds: u64) -> Deadline {
        Deadline::new(Duration::from_secs(seconds))
    }

    #[test]
    fn consecutive_sends_are_spaced_by_the_minimum_interval() {
        let directory = tempdir().expect("create state directory");
        runtime().block_on(async {
            let limiter = limiter(Some(state_file(directory.path())));
            let started = Instant::now();

            drop(limiter.acquire(budget(10)).await.expect("first window"));
            let first = started.elapsed();
            drop(limiter.acquire(budget(10)).await.expect("second window"));
            let second = started.elapsed();

            assert_eq!((first, second), (Duration::ZERO, INTERVAL));
        });
    }

    #[test]
    fn a_window_that_fits_the_remaining_budget_is_awaited() {
        let directory = tempdir().expect("create state directory");
        runtime().block_on(async {
            let limiter = limiter(Some(state_file(directory.path())));
            drop(limiter.acquire(budget(10)).await.expect("first window"));
            let started = Instant::now();

            let permit = limiter.acquire(budget(4)).await;

            assert_eq!((permit.is_ok(), started.elapsed()), (true, INTERVAL));
        });
    }

    #[test]
    fn a_window_beyond_the_budget_times_out_without_reserving_it() {
        let directory = tempdir().expect("create state directory");
        runtime().block_on(async {
            let limiter = limiter(Some(state_file(directory.path())));
            let started = Instant::now();
            drop(limiter.acquire(budget(10)).await.expect("first window"));

            let error = limiter
                .acquire(budget(2))
                .await
                .err()
                .expect("window does not fit");
            let rejected_at = started.elapsed();
            drop(limiter.acquire(budget(10)).await.expect("next window"));

            assert_eq!(
                (error.kind(), rejected_at, started.elapsed()),
                (AttemptErrorKind::Timeout, Duration::ZERO, INTERVAL)
            );
        });
    }

    #[test]
    fn a_held_permit_beyond_max_concurrency_times_out() {
        let directory = tempdir().expect("create state directory");
        runtime().block_on(async {
            let limiter = limiter(Some(state_file(directory.path())));
            let _held = limiter.acquire(budget(10)).await.expect("first window");

            let error = limiter
                .acquire(budget(1))
                .await
                .err()
                .expect("no permit is free");

            assert_eq!(error.kind(), AttemptErrorKind::Timeout);
        });
    }

    #[test]
    fn reservations_persist_for_every_limiter_sharing_the_state_file() {
        let directory = tempdir().expect("create state directory");
        runtime().block_on(async {
            let path = state_file(directory.path());
            // Separate processes share the wall clock, so both limiters use one clock.
            let clock = WallClock::new();
            let started = Instant::now();
            drop(
                limiter_with_clock(Some(path.clone()), clock)
                    .acquire(budget(10))
                    .await
                    .expect("first process window"),
            );

            drop(
                limiter_with_clock(Some(path), clock)
                    .acquire(budget(10))
                    .await
                    .expect("second process window"),
            );

            assert_eq!(started.elapsed(), INTERVAL);
        });
    }

    #[test]
    fn a_corrupt_state_file_is_replaced_after_a_full_interval() {
        let directory = tempdir().expect("create state directory");
        let path = state_file(directory.path());
        fs::create_dir_all(path.parent().expect("state parent")).expect("create state parent");
        fs::write(&path, "not json").expect("write corrupt state");
        runtime().block_on(async {
            let started = Instant::now();

            drop(
                limiter(Some(path.clone()))
                    .acquire(budget(10))
                    .await
                    .expect("window after corrupt state"),
            );

            assert_eq!(started.elapsed(), INTERVAL);
        });
        let state: serde_json::Value =
            serde_json::from_slice(&fs::read(path).expect("read repaired state"))
                .expect("parse repaired state");
        assert!(state["routes"]["test_route"]["reserved_at_ms"].is_u64());
    }

    #[test]
    fn a_busy_state_lock_fails_with_runtime() {
        let directory = tempdir().expect("create state directory");
        let path = state_file(directory.path());
        fs::create_dir_all(path.parent().expect("state parent")).expect("create state parent");
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path.with_extension("lock"))
            .expect("open state lock");
        lock.lock_exclusive().expect("hold state lock");

        let error = runtime().block_on(async {
            limiter(Some(path))
                .acquire(budget(10))
                .await
                .err()
                .expect("busy lock blocks the send")
        });

        FileExt::unlock(&lock).expect("release state lock");
        assert_eq!(error.kind(), AttemptErrorKind::Runtime);
    }

    #[test]
    fn an_unreadable_state_file_fails_with_runtime() {
        let directory = tempdir().expect("create state directory");
        let path = state_file(directory.path());
        fs::create_dir_all(&path).expect("occupy the state path with a directory");

        let error = runtime().block_on(async {
            limiter(Some(path))
                .acquire(budget(10))
                .await
                .err()
                .expect("unreadable state blocks the send")
        });

        assert_eq!(error.kind(), AttemptErrorKind::Runtime);
    }

    #[test]
    fn an_unresolved_state_directory_fails_with_runtime() {
        let error = runtime().block_on(async {
            limiter(None)
                .acquire(budget(10))
                .await
                .err()
                .expect("missing state blocks the send")
        });

        assert_eq!(error.kind(), AttemptErrorKind::Runtime);
    }
}
