use std::time::Duration;

use reqwest::{Client, StatusCode};

use crate::types::AttemptErrorKind;

#[derive(Clone, Copy, Debug)]
pub(crate) struct RetryPolicy {
    max_attempts: usize,
    multiplier: f64,
    max_wait: Duration,
}

impl RetryPolicy {
    pub(crate) fn new(max_attempts: usize, multiplier: f64, max_wait: Duration) -> Self {
        Self {
            max_attempts,
            multiplier,
            max_wait,
        }
    }

    pub(crate) fn max_attempts(self) -> usize {
        self.max_attempts
    }

    pub(crate) fn wait(self, retry_count: usize) -> Duration {
        let seconds = self.multiplier * retry_count as f64;
        Duration::from_secs_f64(seconds).min(self.max_wait)
    }
}

pub(crate) fn build_client(ssl_verify: bool) -> Result<Client, reqwest::Error> {
    Client::builder()
        .danger_accept_invalid_certs(!ssl_verify)
        .build()
}

pub(crate) fn error_kind_for_status(status: StatusCode, body: &str) -> AttemptErrorKind {
    match status.as_u16() {
        400 | 404 | 405 | 409 | 422 => AttemptErrorKind::Parameter,
        401 | 403 => AttemptErrorKind::Auth,
        429 if body.to_ascii_lowercase().contains("quota") => AttemptErrorKind::QuotaExhausted,
        429 => AttemptErrorKind::RateLimited,
        408 | 504 => AttemptErrorKind::Timeout,
        500..=599 => AttemptErrorKind::Network,
        _ => AttemptErrorKind::Runtime,
    }
}

pub(crate) fn truncate_message(message: &str) -> String {
    message.chars().take(500).collect()
}
