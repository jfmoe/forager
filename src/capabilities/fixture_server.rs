//! The fixture server of the in-crate tests: the core shared with the command contract tests,
//! plus the modes and runners only the in-crate tests use.

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

// The core lives with the test support under `tests/` because the command contract tests share
// it.
#[path = "../../tests/support/fixture_server.rs"]
mod shared;

pub(crate) use shared::*;

// Shorter than any test's runner limit, so requests that never arrive together fail with this
// module's diagnosis.
const SYNCHRONIZATION_DEADLINE: Duration = Duration::from_secs(1);

impl Fixture {
    pub(crate) fn start_synchronized_sequence(responses: Vec<Response>) -> Self {
        Self::start_synchronized_sequences(vec![responses])
            .pop()
            .expect("synchronized fixture")
    }

    /// Starts one fixture per sequence, each answering its requests only once every fixture
    /// has received all of its own, so a test proves that the requests were in flight together.
    pub(crate) fn start_synchronized_sequences(
        response_sequences: Vec<Vec<Response>>,
    ) -> Vec<Self> {
        let total_requests = response_sequences.iter().map(Vec::len).sum();
        let synchronization = Arc::new(ConnectionBarrier::new(total_requests));
        response_sequences
            .into_iter()
            .map(|responses| {
                let expected_requests = responses.len();
                let synchronization = Arc::clone(&synchronization);
                Self::serve(
                    shared::fixture_listener("synchronized fixture"),
                    Some(expected_requests),
                    move |listener, stopped| {
                        let workers = responses
                            .into_iter()
                            .enumerate()
                            .map(|(received, response)| {
                                let stream = shared::accept_request(
                                    &listener,
                                    &stopped,
                                    expected_requests,
                                    received,
                                );
                                let synchronization = Arc::clone(&synchronization);
                                thread::spawn(move || {
                                    synchronization.arrive_and_wait();
                                    shared::respond(stream, &response)
                                })
                            })
                            .collect::<Vec<_>>();
                        let requests = workers
                            .into_iter()
                            .map(|worker| {
                                worker
                                    .join()
                                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
                            })
                            .collect();
                        shared::observe_extra_requests(&listener, &stopped, requests)
                    },
                )
            })
            .collect()
    }

    /// Accepts one request and never answers it, holding the connection open until the fixture
    /// is finished, for a test that runs past a deadline with [`advance_after_request`].
    pub(crate) fn start_unanswered() -> UnansweredFixture {
        let arrived = Arc::new(AtomicBool::new(false));
        let server_arrived = Arc::clone(&arrived);
        let fixture = Self::serve(
            shared::fixture_listener("unanswered fixture"),
            Some(1),
            move |listener, stopped| {
                let mut stream = shared::accept_request(&listener, &stopped, 1, 0);
                let request = shared::read_request(&mut stream);
                server_arrived.store(true, Ordering::Release);
                let _ = stopped.recv();
                vec![request]
            },
        );
        UnansweredFixture {
            url: fixture.url.clone(),
            fixture,
            arrived,
        }
    }
}

/// A fixture started with [`Fixture::start_unanswered`], which reports when its request arrived.
pub(crate) struct UnansweredFixture {
    pub(crate) url: String,
    fixture: Fixture,
    arrived: Arc<AtomicBool>,
}

impl UnansweredFixture {
    pub(crate) fn finish(self) -> String {
        self.fixture.finish()
    }
}

impl Response {
    /// Declares `length` as the body's length, so a client reading a shorter body sees it cut.
    pub(crate) fn with_declared_content_length(mut self, length: usize) -> Self {
        self.declared_content_length = Some(length);
        self
    }
}

struct ConnectionBarrier {
    expected: usize,
    arrived: Mutex<usize>,
    all_arrived: Condvar,
}

impl ConnectionBarrier {
    fn new(expected: usize) -> Self {
        Self {
            expected,
            arrived: Mutex::new(0),
            all_arrived: Condvar::new(),
        }
    }

    fn arrive_and_wait(&self) {
        let mut arrived = self.arrived.lock().expect("lock fixture barrier");
        *arrived += 1;
        if *arrived == self.expected {
            self.all_arrived.notify_all();
            return;
        }
        let (arrived, timeout) = self
            .all_arrived
            .wait_timeout_while(arrived, SYNCHRONIZATION_DEADLINE, |arrived| {
                *arrived < self.expected
            })
            .expect("wait on fixture barrier");
        assert!(
            !timeout.timed_out(),
            "fixture timed out waiting for concurrent requests: expected {}, received {}",
            self.expected,
            *arrived
        );
    }
}

/// Runs `operation` on the paused clock until `fixture` holds its request, then advances the
/// clock by `elapsed` and returns the outcome. Waiting for the request first keeps the clock from
/// passing a deadline while the request is still in flight, so the outcome does not depend on
/// scheduling.
pub(crate) fn advance_after_request<T>(
    fixture: &UnansweredFixture,
    elapsed: Duration,
    operation: impl Future<Output = T>,
) -> T {
    block_on_paused(async {
        let mut operation = std::pin::pin!(operation);
        while !fixture.arrived.load(Ordering::Acquire) {
            let polled = futures_util::poll!(operation.as_mut());
            assert!(
                polled.is_pending(),
                "the operation ended before the fixture received its request"
            );
            tokio::task::yield_now().await;
        }
        tokio::time::advance(elapsed).await;
        operation.await
    })
}

/// Runs `future` to completion on a current-thread runtime with I/O and time enabled, for an
/// in-crate test that drives async code without pausing the clock.
pub(crate) fn block_on<T>(future: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime")
        .block_on(future)
}

/// Runs `future` to completion on a current-thread runtime whose clock starts paused. Paused time
/// advances only while every task waits on a timer, so each timed step lands at an exact instant.
pub(crate) fn block_on_paused<T>(future: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .start_paused(true)
        .build()
        .expect("paused test runtime")
        .block_on(future)
}
