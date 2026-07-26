use std::future::Future;
use std::time::{Duration, Instant};

use crate::credentials::CredentialPool;
use crate::net::RetryPolicy;
use crate::providers::ProviderError;
use crate::types::{AttemptErrorKind, Deadline, ProviderAttempt};

pub(super) struct ExecutionOutcome<T> {
    pub(super) value: T,
    pub(super) attempts: Vec<ProviderAttempt>,
    pub(super) diagnostic: Option<String>,
}

pub(super) struct AttemptFailure {
    pub(super) kind: AttemptErrorKind,
    pub(super) status: Option<u16>,
    pub(super) message: String,
}

pub(super) struct ExecutionSettings {
    pub(super) provider: &'static str,
    pub(super) seam: &'static str,
    pub(super) retry_policy: RetryPolicy,
    pub(super) deadline: Deadline,
    pub(super) attempt_timeout: Duration,
    pub(super) verbose: bool,
    pub(super) timeout_message: &'static str,
}

pub(super) async fn execute<T, F, Fut>(
    credentials: &CredentialPool,
    settings: ExecutionSettings,
    mut send_once: F,
) -> Result<ExecutionOutcome<T>, ProviderError>
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Result<(u16, T), AttemptFailure>>,
{
    let selection = credentials.claim();
    let mut attempts = Vec::new();
    let mut credential_index = selection.index;
    let mut retry_count = 0;
    let mut rotation_count = 0;

    loop {
        let Some(remaining) = settings.deadline.remaining() else {
            return Err(terminal_error(
                settings.provider,
                AttemptErrorKind::Timeout,
                attempts,
                settings.verbose,
                selection.diagnostic.clone(),
            ));
        };
        let started = Instant::now();
        let response = tokio::time::timeout(
            remaining.min(settings.attempt_timeout),
            send_once(credentials.key(credential_index).to_owned()),
        )
        .await;
        let failure = match response {
            Ok(Ok((status, value))) => {
                attempts.push(ProviderAttempt {
                    provider: settings.provider,
                    seam: settings.seam,
                    error_kind: None,
                    http_status: Some(status),
                    duration_ms: millis(started.elapsed()),
                    credential_index,
                    retry_count,
                    rotation_count,
                    message: String::new(),
                });
                return Ok(ExecutionOutcome {
                    value,
                    attempts,
                    diagnostic: selection.diagnostic,
                });
            }
            Ok(Err(failure)) => failure,
            Err(_) => AttemptFailure {
                kind: AttemptErrorKind::Timeout,
                status: None,
                message: settings.timeout_message.into(),
            },
        };
        let kind = failure.kind;
        attempts.push(ProviderAttempt {
            provider: settings.provider,
            seam: settings.seam,
            error_kind: Some(kind),
            http_status: failure.status,
            duration_ms: millis(started.elapsed()),
            credential_index,
            retry_count,
            rotation_count,
            message: failure.message,
        });

        if kind.rotates_credential() && rotation_count + 1 < credentials.len() {
            rotation_count += 1;
            credential_index = credentials.rotated_index(selection.index, rotation_count);
            continue;
        }
        if kind.is_retryable() && attempts.len() < settings.retry_policy.max_attempts() {
            retry_count += 1;
            let wait = settings.retry_policy.wait(retry_count);
            if settings
                .deadline
                .remaining()
                .is_none_or(|remaining| wait >= remaining)
            {
                return Err(terminal_error(
                    settings.provider,
                    AttemptErrorKind::Timeout,
                    attempts,
                    settings.verbose,
                    selection.diagnostic.clone(),
                ));
            }
            tokio::time::sleep(wait).await;
            continue;
        }
        return Err(terminal_error(
            settings.provider,
            kind,
            attempts,
            settings.verbose,
            selection.diagnostic.clone(),
        ));
    }
}

fn terminal_error(
    provider: &'static str,
    kind: AttemptErrorKind,
    attempts: Vec<ProviderAttempt>,
    verbose: bool,
    diagnostic: Option<String>,
) -> ProviderError {
    let message = attempts.last().map_or_else(
        || format!("{provider} request failed"),
        |attempt| attempt.message.clone(),
    );
    ProviderError {
        kind,
        message,
        attempts,
        verbose,
        diagnostic,
        redirected_library_id: None,
    }
}

fn millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}
