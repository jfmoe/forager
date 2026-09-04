use std::future::Future;
use std::time::Duration;

use tokio::time::Instant;

use crate::credentials::CredentialPool;
use crate::net::{AttemptFailure, RetryPolicy, duration_millis};
use crate::providers::ProviderError;
use crate::redact::Secret;
use crate::types::{
    AttemptDisposition, AttemptErrorKind, AttemptTarget, Deadline, ProviderAttempt,
};

pub(crate) struct ExecutionOutcome<T> {
    pub(crate) value: T,
    pub(crate) attempts: Vec<ProviderAttempt>,
    pub(crate) diagnostic: Option<String>,
}

pub(crate) struct ExecutionSettings {
    pub(crate) provider: &'static str,
    pub(crate) target: AttemptTarget,
    pub(crate) retry_policy: RetryPolicy,
    pub(crate) deadline: Deadline,
    pub(crate) attempt_timeout: Duration,
    pub(crate) verbose: bool,
    pub(crate) timeout_message: &'static str,
    pub(crate) model: Option<String>,
    pub(crate) transport: Option<&'static str>,
    pub(crate) endpoint_host: Option<String>,
    pub(crate) breaker_event: Option<&'static str>,
}

// The shared retry loop keeps every terminal path under the same attempt accounting rules.
#[expect(clippy::too_many_lines)]
pub(crate) async fn execute_v2<T, F, Fut>(
    credentials: &CredentialPool,
    settings: ExecutionSettings,
    mut send_once: F,
) -> Result<ExecutionOutcome<T>, ProviderError>
where
    F: FnMut(Secret, Deadline) -> Fut,
    Fut: Future<Output = Result<(u16, T), AttemptFailure>>,
{
    let selection = credentials.claim().await;
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
        let attempt_limit = remaining.min(settings.attempt_timeout);
        let attempt_deadline = Deadline::new(attempt_limit);
        let started = Instant::now();
        let response = tokio::time::timeout(
            attempt_limit,
            send_once(credentials.key(credential_index).clone(), attempt_deadline),
        )
        .await;
        let failure = match response {
            Ok(Ok((status, value))) => {
                attempts.push(ProviderAttempt {
                    provider: settings.provider,
                    target: settings.target,
                    disposition: AttemptDisposition::Succeeded,
                    error_kind: None,
                    http_status: Some(status),
                    duration_ms: duration_millis(started.elapsed()),
                    credential_index,
                    retry_count,
                    rotation_count,
                    message: String::new(),
                    model: settings.model.clone(),
                    transport: settings.transport,
                    endpoint_host: settings.endpoint_host.clone(),
                    breaker_event: settings.breaker_event,
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
            target: settings.target,
            disposition: AttemptDisposition::Failed,
            error_kind: Some(kind),
            http_status: failure.status,
            duration_ms: duration_millis(started.elapsed()),
            credential_index,
            retry_count,
            rotation_count,
            message: failure.message,
            model: settings.model.clone(),
            transport: settings.transport,
            endpoint_host: settings.endpoint_host.clone(),
            breaker_event: settings.breaker_event,
        });

        if kind.rotates_credential() && rotation_count + 1 < credentials.len() {
            rotation_count += 1;
            credential_index = credentials.rotated_index(selection.index, rotation_count);
            continue;
        }
        if kind.is_retryable()
            && retry_count.saturating_add(1) < settings.retry_policy.max_attempts()
        {
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

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::{ExecutionSettings, execute_v2};
    use crate::credentials::CredentialPool;
    use crate::net::{AttemptFailure, RetryPolicy};
    use crate::types::{AttemptErrorKind, AttemptTarget, Deadline};
    use std::time::Duration;

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .start_paused(true)
            .build()
            .expect("test runtime")
    }

    fn settings(
        retry_policy: RetryPolicy,
        deadline: Duration,
        attempt_timeout: Duration,
    ) -> ExecutionSettings {
        ExecutionSettings {
            provider: "test",
            target: AttemptTarget::seam("web_fetch"),
            retry_policy,
            deadline: Deadline::new(deadline),
            attempt_timeout,
            verbose: true,
            timeout_message: "test request timed out",
            model: None,
            transport: Some("test"),
            endpoint_host: None,
            breaker_event: None,
        }
    }

    fn network_failure() -> AttemptFailure {
        AttemptFailure {
            kind: AttemptErrorKind::Network,
            status: Some(503),
            message: "test failure".into(),
        }
    }

    #[test]
    fn attempt_timeout_retries_without_rotating_credentials() {
        runtime().block_on(async {
            let calls = Rc::new(RefCell::new(0));
            let outcome = execute_v2(
                &CredentialPool::new("test", vec!["first".into(), "second".into()]),
                settings(
                    RetryPolicy::new(2, 1.0, Duration::from_secs(10)),
                    Duration::from_secs(10),
                    Duration::from_secs(2),
                ),
                {
                    let calls = Rc::clone(&calls);
                    move |_, _| {
                        *calls.borrow_mut() += 1;
                        let call = *calls.borrow();
                        async move {
                            if call == 1 {
                                tokio::time::sleep(Duration::from_secs(3)).await;
                            }
                            Ok((200, "content"))
                        }
                    }
                },
            )
            .await
            .expect("retry succeeds");

            assert_eq!(
                outcome
                    .attempts
                    .iter()
                    .map(|attempt| (
                        attempt.error_kind,
                        attempt.retry_count,
                        attempt.rotation_count,
                    ))
                    .collect::<Vec<_>>(),
                vec![(Some(AttemptErrorKind::Timeout), 0, 0), (None, 1, 0),]
            );
            assert_eq!(
                outcome.attempts[0].credential_index,
                outcome.attempts[1].credential_index
            );
        });
    }

    #[test]
    fn retries_use_linear_backoff_capped_by_max_wait() {
        runtime().block_on(async {
            let started = tokio::time::Instant::now();
            let calls = Rc::new(RefCell::new(Vec::new()));
            let outcome = execute_v2(
                &CredentialPool::new("test", vec!["key".into()]),
                settings(
                    RetryPolicy::new(4, 1.5, Duration::from_secs(4)),
                    Duration::from_secs(20),
                    Duration::from_secs(10),
                ),
                {
                    let calls = Rc::clone(&calls);
                    move |_, _| {
                        calls.borrow_mut().push(started.elapsed());
                        let call = calls.borrow().len();
                        async move {
                            if call < 4 {
                                Err(network_failure())
                            } else {
                                Ok((200, "content"))
                            }
                        }
                    }
                },
            )
            .await
            .expect("final retry succeeds");

            assert_eq!(
                calls.borrow().as_slice(),
                [
                    Duration::ZERO,
                    Duration::from_millis(1_500),
                    Duration::from_millis(4_500),
                    Duration::from_millis(8_500),
                ]
            );
            assert_eq!(
                outcome.attempts.last().map(|attempt| attempt.retry_count),
                Some(3)
            );
        });
    }

    #[test]
    fn retry_stops_immediately_when_backoff_exceeds_remaining_deadline() {
        runtime().block_on(async {
            let started = tokio::time::Instant::now();
            let calls = Rc::new(RefCell::new(0));
            let error = execute_v2(
                &CredentialPool::new("test", vec!["key".into()]),
                settings(
                    RetryPolicy::new(2, 5.0, Duration::from_secs(10)),
                    Duration::from_secs(4),
                    Duration::from_secs(4),
                ),
                {
                    let calls = Rc::clone(&calls);
                    move |_, _| {
                        *calls.borrow_mut() += 1;
                        async { Err::<(u16, ()), _>(network_failure()) }
                    }
                },
            )
            .await
            .err()
            .expect("deadline cannot fit retry backoff");

            assert_eq!(
                (
                    error.kind,
                    error.attempts.len(),
                    *calls.borrow(),
                    started.elapsed()
                ),
                (AttemptErrorKind::Timeout, 1, 1, Duration::ZERO)
            );
            assert_eq!(
                error.attempts[0].error_kind,
                Some(AttemptErrorKind::Network)
            );
        });
    }

    #[test]
    fn attempt_timeout_consumes_the_shared_deadline_before_retry_backoff() {
        runtime().block_on(async {
            let calls = Rc::new(RefCell::new(0));
            let error = execute_v2(
                &CredentialPool::new("test", vec!["key".into()]),
                settings(
                    RetryPolicy::new(2, 1.0, Duration::from_secs(10)),
                    Duration::from_secs(3),
                    Duration::from_secs(2),
                ),
                {
                    let calls = Rc::clone(&calls);
                    move |_, _| {
                        *calls.borrow_mut() += 1;
                        async {
                            tokio::time::sleep(Duration::from_secs(3)).await;
                            Ok::<_, AttemptFailure>((200, ()))
                        }
                    }
                },
            )
            .await
            .err()
            .expect("retry backoff consumes the final second");

            assert_eq!(
                (error.kind, error.attempts.len(), *calls.borrow()),
                (AttemptErrorKind::Timeout, 1, 1)
            );
        });
    }
}
