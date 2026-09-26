use std::time::Duration;

use tokio::time::Instant;

pub(crate) const MIN_USEFUL_SLICE_SECONDS: u64 = 5;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Deadline {
    started: Instant,
    duration: Duration,
}

impl Deadline {
    pub(crate) fn new(duration: Duration) -> Self {
        Self {
            started: Instant::now(),
            duration,
        }
    }

    pub(crate) fn remaining(self) -> Option<Duration> {
        self.duration.checked_sub(self.started.elapsed())
    }
}
