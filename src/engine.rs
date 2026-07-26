use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;

use crate::config::{self, MainSearchRuntimeConfig, WebFetchRuntimeConfig};
use crate::net::RetryPolicy;
use crate::providers::{self, FetchRequest, ProviderError, SearchRequest};
use crate::types::{
    AttemptErrorKind, DENSITY_MAX_CHARS, DENSITY_MAX_UNIQUE_LINES, Deadline, FetchOutcome,
    MIN_FETCH_CONTENT_CHARS, MIN_USEFUL_SLICE_SECONDS, ProviderAttempt, SearchOutcome,
};

pub(crate) async fn search(
    request: SearchRequest,
    config: MainSearchRuntimeConfig,
    fallback: &str,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    model_breakers: Arc<providers::ModelBreakers>,
) -> Result<SearchOutcome, ProviderError> {
    let executable = if fallback == "off" {
        config.backends.iter().take(1).cloned().collect::<Vec<_>>()
    } else {
        config
            .backends
            .iter()
            .filter(|backend| backend_is_configured(backend, &config))
            .cloned()
            .collect::<Vec<_>>()
    };
    let mut attempts = Vec::new();
    let mut diagnostics = Vec::new();

    for (index, backend) in executable.iter().enumerate() {
        if !backend_is_configured(backend, &config) {
            attempts.push(unconfigured_attempt(backend));
            break;
        }
        let remaining_slots = executable.len() - index;
        let Some(remaining) = deadline.remaining() else {
            break;
        };
        let Some(budget) = provider_budget(remaining, remaining_slots) else {
            attempts.push(skipped_attempt(backend, "main_search"));
            continue;
        };
        let provider_config = config
            .provider(backend)
            .expect("validated main search backend")
            .clone();
        let result = providers::build_main_search(
            backend,
            provider_config,
            client.clone(),
            retry_policy,
            Deadline::new(budget),
            model_breakers.clone(),
        )
        .search(request.clone())
        .await;
        match result {
            Ok(mut outcome) => {
                if let Some(diagnostic) = outcome.diagnostic.take() {
                    diagnostics.push(diagnostic);
                }
                attempts.extend(outcome.attempts);
                outcome.attempts = attempts;
                outcome.diagnostic = combine_diagnostics(diagnostics);
                return Ok(outcome);
            }
            Err(error) => {
                if let Some(diagnostic) = error.diagnostic {
                    diagnostics.push(diagnostic);
                }
                attempts.extend(error.attempts);
            }
        }
    }
    let terminal = terminal_attempt(&attempts);
    let kind = terminal
        .and_then(|attempt| attempt.error_kind)
        .unwrap_or(AttemptErrorKind::Timeout);
    let message = terminal.map_or_else(
        || "main search deadline elapsed".into(),
        |attempt| attempt.message.clone(),
    );
    Err(ProviderError {
        kind,
        message,
        attempts,
        verbose: request.verbose,
        diagnostic: combine_diagnostics(diagnostics),
        redirected_library_id: None,
    })
}

fn backend_is_configured(backend: &str, config: &MainSearchRuntimeConfig) -> bool {
    config
        .provider(backend)
        .is_some_and(config::MainSearchProviderConfig::configured)
}

fn unconfigured_attempt(backend: &str) -> ProviderAttempt {
    ProviderAttempt {
        provider: providers::registration_by_name(backend)
            .expect("validated main search backend")
            .name,
        seam: "main_search",
        error_kind: Some(AttemptErrorKind::Auth),
        http_status: None,
        duration_ms: 0,
        credential_index: 0,
        retry_count: 0,
        rotation_count: 0,
        message: format!("{backend} has no configured credentials"),
        model: None,
        transport: None,
        endpoint_host: None,
        breaker_event: None,
    }
}

pub(crate) async fn fetch(
    request: FetchRequest,
    config: WebFetchRuntimeConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Result<FetchOutcome, ProviderError> {
    let executable = config
        .order
        .iter()
        .filter(|provider| {
            config
                .provider(provider)
                .is_some_and(|provider| !provider.keys.is_empty())
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut attempts = Vec::new();
    let mut diagnostics = Vec::new();

    for (index, provider_name) in executable.iter().enumerate() {
        let remaining_slots = executable.len() - index;
        let Some(remaining) = deadline.remaining() else {
            break;
        };
        let Some(provider_budget) = provider_budget(remaining, remaining_slots) else {
            attempts.push(skipped_attempt(provider_name, "web_fetch"));
            continue;
        };
        let provider_config = config
            .provider(provider_name)
            .expect("validated fetch provider")
            .clone();
        let provider = providers::build_web_fetch(
            provider_name,
            provider_config,
            client.clone(),
            retry_policy,
            Deadline::new(provider_budget),
        );
        match provider.fetch(&request).await {
            Ok(mut outcome) => {
                if let Some(diagnostic) = outcome.diagnostic.take() {
                    diagnostics.push(diagnostic);
                }
                if is_thin(&outcome.content, &request.url) {
                    let character_count = outcome.content.chars().count();
                    if let Some(attempt) = outcome.attempts.last_mut() {
                        attempt.error_kind = Some(AttemptErrorKind::Quality);
                        attempt.message =
                            format!("extracted content is too thin ({character_count} characters)");
                    }
                    attempts.extend(outcome.attempts);
                    continue;
                }
                attempts.extend(outcome.attempts);
                return Ok(FetchOutcome {
                    provider: outcome.provider,
                    url: config::redact_url(&request.url),
                    content: outcome.content,
                    attempts: if request.verbose {
                        attempts
                    } else {
                        Vec::new()
                    },
                    diagnostic: combine_diagnostics(diagnostics),
                });
            }
            Err(error) => {
                if let Some(diagnostic) = error.diagnostic {
                    diagnostics.push(diagnostic);
                }
                attempts.extend(error.attempts);
            }
        }
    }

    let kind = terminal_kind(&attempts);
    let message = attempts
        .last()
        .map(|attempt| attempt.message.clone())
        .unwrap_or_else(|| "web fetch deadline elapsed".into());
    Err(ProviderError {
        kind,
        message,
        attempts,
        verbose: request.verbose,
        diagnostic: combine_diagnostics(diagnostics),
        redirected_library_id: None,
    })
}

fn is_thin(content: &str, url: &str) -> bool {
    let content = content.trim();
    let character_count = content.chars().count();
    if character_count < MIN_FETCH_CONTENT_CHARS {
        return true;
    }
    if is_pdf(url) {
        return false;
    }
    let unique_lines = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<HashSet<_>>()
        .len();
    unique_lines <= DENSITY_MAX_UNIQUE_LINES && character_count < DENSITY_MAX_CHARS
}

fn is_pdf(url: &str) -> bool {
    url.split(['?', '#'])
        .next()
        .is_some_and(|path| path.to_ascii_lowercase().ends_with(".pdf"))
}

fn skipped_attempt(provider: &str, seam: &'static str) -> ProviderAttempt {
    let provider = providers::registration_by_name(provider)
        .expect("validated fetch provider")
        .name;
    ProviderAttempt {
        provider,
        seam,
        error_kind: Some(AttemptErrorKind::Timeout),
        http_status: None,
        duration_ms: 0,
        credential_index: 0,
        retry_count: 0,
        rotation_count: 0,
        message: "skipped to preserve fallback deadline budget".into(),
        model: None,
        transport: None,
        endpoint_host: None,
        breaker_event: None,
    }
}

fn terminal_kind(attempts: &[ProviderAttempt]) -> AttemptErrorKind {
    terminal_attempt(attempts)
        .and_then(|attempt| attempt.error_kind)
        .unwrap_or(AttemptErrorKind::Timeout)
}

fn terminal_attempt(attempts: &[ProviderAttempt]) -> Option<&ProviderAttempt> {
    let mut final_providers = HashSet::new();
    attempts
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, attempt)| final_providers.insert(attempt.provider))
        .filter(|(_, attempt)| attempt.error_kind.is_some())
        .max_by_key(|(index, attempt)| {
            (
                error_priority(attempt.error_kind.expect("filtered error kind")),
                *index,
            )
        })
        .map(|(_, attempt)| attempt)
}

fn provider_budget(remaining: Duration, remaining_slots: usize) -> Option<Duration> {
    if remaining_slots == 1 {
        return Some(remaining);
    }
    let slice = remaining / u32::try_from(remaining_slots).unwrap_or(u32::MAX);
    (slice >= Duration::from_secs(MIN_USEFUL_SLICE_SECONDS)).then_some(slice)
}

fn error_priority(kind: AttemptErrorKind) -> u8 {
    match kind {
        AttemptErrorKind::Network => 0,
        AttemptErrorKind::Timeout => 1,
        AttemptErrorKind::RateLimited => 2,
        AttemptErrorKind::QuotaExhausted => 3,
        AttemptErrorKind::Auth => 4,
        AttemptErrorKind::Parameter => 5,
        AttemptErrorKind::Runtime => 6,
        AttemptErrorKind::Quality => 7,
        AttemptErrorKind::Evidence => 8,
    }
}

fn combine_diagnostics(diagnostics: Vec<String>) -> Option<String> {
    (!diagnostics.is_empty()).then(|| diagnostics.join("\n"))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{error_priority, provider_budget, terminal_attempt, terminal_kind};
    use crate::types::{AttemptErrorKind, ProviderAttempt};

    #[test]
    fn provider_budget_preserves_reachable_fallback_slots_at_every_boundary() {
        for (remaining, slots, expected) in [
            (0, 1, Some(Duration::ZERO)),
            (4, 1, Some(Duration::from_secs(4))),
            (9, 2, None),
            (10, 2, Some(Duration::from_secs(5))),
            (11, 2, Some(Duration::from_millis(5500))),
            (14, 3, None),
            (15, 3, Some(Duration::from_secs(5))),
            (16, 3, Some(Duration::from_nanos(5_333_333_333))),
        ] {
            assert_eq!(
                provider_budget(Duration::from_secs(remaining), slots),
                expected,
                "remaining={remaining}, slots={slots}"
            );
        }
    }

    #[test]
    fn terminal_kind_exhausts_final_kind_pairs_and_ignores_history() {
        for first in ALL_KINDS {
            for second in ALL_KINDS {
                let expected = if error_priority(first) >= error_priority(second) {
                    first
                } else {
                    second
                };
                assert_eq!(
                    terminal_kind(&[
                        attempt("tavily", AttemptErrorKind::Evidence),
                        attempt("tavily", first),
                        attempt("jina", AttemptErrorKind::Evidence),
                        attempt("jina", second),
                    ]),
                    expected,
                    "first={first:?}, second={second:?}"
                );
            }
        }
    }

    #[test]
    fn terminal_attempt_uses_the_later_provider_when_kinds_match() {
        let attempts = [
            attempt_with_message("xai", AttemptErrorKind::Auth, "first"),
            attempt_with_message("openai_compatible", AttemptErrorKind::Auth, "second"),
        ];

        assert_eq!(
            terminal_attempt(&attempts).map(|attempt| attempt.message.as_str()),
            Some("second")
        );
    }

    const ALL_KINDS: [AttemptErrorKind; 9] = [
        AttemptErrorKind::Auth,
        AttemptErrorKind::RateLimited,
        AttemptErrorKind::QuotaExhausted,
        AttemptErrorKind::Parameter,
        AttemptErrorKind::Timeout,
        AttemptErrorKind::Network,
        AttemptErrorKind::Quality,
        AttemptErrorKind::Evidence,
        AttemptErrorKind::Runtime,
    ];

    fn attempt(provider: &'static str, kind: AttemptErrorKind) -> ProviderAttempt {
        attempt_with_message(provider, kind, "")
    }

    fn attempt_with_message(
        provider: &'static str,
        kind: AttemptErrorKind,
        message: &str,
    ) -> ProviderAttempt {
        ProviderAttempt {
            provider,
            seam: "web_fetch",
            error_kind: Some(kind),
            http_status: None,
            duration_ms: 0,
            credential_index: 0,
            retry_count: 0,
            rotation_count: 0,
            message: message.into(),
            model: None,
            transport: None,
            endpoint_host: None,
            breaker_event: None,
        }
    }
}
