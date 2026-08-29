use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::{Value, json};

use crate::config::LogLevel;
use crate::types::{AttemptDisposition, AttemptTarget, ProviderAttempt};

pub(crate) fn render(level: LogLevel, attempts: &[ProviderAttempt]) -> Option<String> {
    if attempts.is_empty() || matches!(level, LogLevel::Error | LogLevel::Warn | LogLevel::Info) {
        return None;
    }

    let mut summary = bounded_attempt_summary(attempts);
    let summary = summary
        .as_object_mut()
        .expect("attempt summary is always an object");
    summary.insert(
        "retry".into(),
        Value::Bool(attempts.iter().any(|attempt| attempt.retry_count > 0)),
    );
    summary.insert(
        "credential_rotation".into(),
        Value::Bool(attempts.iter().any(|attempt| attempt.rotation_count > 0)),
    );
    summary.insert("fallback".into(), Value::Bool(fallback_occurred(attempts)));
    summary.insert(
        "breaker_event".into(),
        Value::Bool(
            attempts
                .iter()
                .any(|attempt| attempt.breaker_event.is_some()),
        ),
    );

    let mut lines = vec![format!(
        "forager attempts: {}",
        Value::Object(summary.clone())
    )];
    if level == LogLevel::Trace {
        lines.extend(attempts.iter().map(|attempt| {
            format!(
                "forager attempt: {}",
                serde_json::to_string(&SafeAttempt::from(attempt))
                    .expect("safe attempt fields are serializable")
            )
        }));
    }
    Some(lines.join("\n"))
}

fn fallback_occurred(attempts: &[ProviderAttempt]) -> bool {
    let mut identity_by_target = BTreeMap::new();
    for attempt in attempts {
        let identity = (attempt.provider, attempt.model.as_deref());
        if let Some(first_identity) = identity_by_target.get(&attempt.target) {
            if *first_identity != identity {
                return true;
            }
        } else {
            identity_by_target.insert(attempt.target, identity);
        }
    }
    false
}

#[must_use]
pub fn bounded_attempt_summary(attempts: &[ProviderAttempt]) -> Value {
    let mut by_kind = BTreeMap::new();
    for attempt in attempts {
        if let Some(kind) = attempt.error_kind {
            *by_kind.entry(kind.as_str()).or_insert(0) += 1;
        }
    }
    let by_kind_count = by_kind.len();
    let by_kind = by_kind.into_iter().take(8).collect::<BTreeMap<_, _>>();
    let provider_set = attempts
        .iter()
        .map(|attempt| attempt.provider)
        .collect::<BTreeSet<_>>();
    let providers = provider_set.iter().take(8).copied().collect::<Vec<_>>();
    let by_kind_truncated = by_kind_count > by_kind.len();
    let providers_truncated = provider_set.len() > providers.len();

    json!({
        "total": attempts.len(),
        "by_kind": by_kind,
        "by_kind_truncated": by_kind_truncated,
        "providers": providers,
        "providers_truncated": providers_truncated,
        "truncated": by_kind_truncated || providers_truncated,
    })
}

#[derive(Serialize)]
struct SafeAttempt {
    provider: &'static str,
    #[serde(flatten)]
    target: AttemptTarget,
    disposition: AttemptDisposition,
    error_kind: Option<&'static str>,
    http_status: Option<u16>,
    duration_ms: u64,
    credential_index: usize,
    retry_count: usize,
    rotation_count: usize,
    transport: Option<&'static str>,
    breaker_event: Option<&'static str>,
}

impl From<&ProviderAttempt> for SafeAttempt {
    fn from(attempt: &ProviderAttempt) -> Self {
        Self {
            provider: attempt.provider,
            target: attempt.target,
            disposition: attempt.disposition,
            error_kind: attempt
                .error_kind
                .map(crate::types::AttemptErrorKind::as_str),
            http_status: attempt.http_status,
            duration_ms: attempt.duration_ms,
            credential_index: attempt.credential_index,
            retry_count: attempt.retry_count,
            rotation_count: attempt.rotation_count,
            transport: attempt.transport,
            breaker_event: attempt.breaker_event,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::LogLevel;
    use crate::types::{AttemptDisposition, AttemptErrorKind, AttemptTarget, ProviderAttempt};

    use super::render;

    #[test]
    fn log_levels_project_only_the_configured_attempt_detail() {
        let attempts = [attempt(AttemptErrorKind::Network)];

        let cases = [
            (LogLevel::Error, 0),
            (LogLevel::Warn, 0),
            (LogLevel::Info, 0),
            (LogLevel::Debug, 1),
            (LogLevel::Trace, 2),
        ];

        for (level, expected_lines) in cases {
            assert_eq!(
                render(level, &attempts)
                    .as_deref()
                    .map_or(0, |output| output.lines().count()),
                expected_lines,
                "unexpected projection for {level:?}"
            );
        }
        assert_eq!(render(LogLevel::Trace, &[]), None);
    }

    #[test]
    fn summary_marks_same_provider_model_change_as_fallback() {
        let first = attempt(AttemptErrorKind::Runtime);
        let mut second = attempt(AttemptErrorKind::Network);
        second.model = Some("fallback-model".into());

        let output = render(LogLevel::Debug, &[first, second]).expect("debug projection");
        let summary: serde_json::Value = serde_json::from_str(
            output
                .strip_prefix("forager attempts: ")
                .expect("summary prefix"),
        )
        .expect("summary JSON");

        assert_eq!(&summary["fallback"], &serde_json::json!(true));
    }

    #[test]
    fn summary_does_not_mark_a_seam_transition_as_fallback() {
        let mut classifier = attempt(AttemptErrorKind::Runtime);
        classifier.provider = "classifier";
        classifier.target = AttemptTarget::seam("classifier");
        classifier.model = Some("classifier-model".into());
        let main_search = attempt(AttemptErrorKind::Network);

        let output = render(LogLevel::Debug, &[classifier, main_search]).expect("debug projection");
        let summary: serde_json::Value = serde_json::from_str(
            output
                .strip_prefix("forager attempts: ")
                .expect("summary prefix"),
        )
        .expect("summary JSON");

        assert_eq!(&summary["fallback"], &serde_json::json!(false));
    }

    #[test]
    fn trace_projection_contains_only_safe_attempt_fields() {
        let mut first = attempt(AttemptErrorKind::Auth);
        first.message = "credential-canary at https://example.test/?token=url-secret".into();
        first.model = Some("model-canary".into());
        first.endpoint_host = Some("endpoint-canary.example".into());
        first.rotation_count = 1;
        let mut second = attempt(AttemptErrorKind::Network);
        second.provider = "fallback";
        second.retry_count = 1;
        second.breaker_event = Some("open");

        let output = render(LogLevel::Trace, &[first, second]).expect("trace projection");
        let mut lines = output.lines();
        let summary: serde_json::Value = serde_json::from_str(
            lines
                .next()
                .expect("summary")
                .strip_prefix("forager attempts: ")
                .expect("summary prefix"),
        )
        .expect("summary JSON");
        let detail = lines
            .map(|line| {
                serde_json::from_str::<serde_json::Value>(
                    line.strip_prefix("forager attempt: ")
                        .expect("attempt prefix"),
                )
                .expect("attempt JSON")
            })
            .collect::<Vec<_>>();

        assert_eq!(
            (
                &summary["total"],
                &summary["by_kind"],
                &summary["providers"],
                &summary["retry"],
                &summary["credential_rotation"],
                &summary["fallback"],
                &summary["breaker_event"],
            ),
            (
                &serde_json::json!(2),
                &serde_json::json!({"auth": 1, "network": 1}),
                &serde_json::json!(["fallback", "fixture"]),
                &serde_json::json!(true),
                &serde_json::json!(true),
                &serde_json::json!(true),
                &serde_json::json!(true),
            )
        );
        assert_eq!(
            detail[0]
                .as_object()
                .expect("attempt object")
                .keys()
                .map(String::as_str)
                .collect::<std::collections::BTreeSet<_>>(),
            [
                "breaker_event",
                "credential_index",
                "disposition",
                "duration_ms",
                "error_kind",
                "http_status",
                "provider",
                "retry_count",
                "rotation_count",
                "seam",
                "transport",
            ]
            .into_iter()
            .collect()
        );
        for canary in [
            "credential-canary",
            "model-canary",
            "endpoint-canary",
            "url-secret",
        ] {
            assert!(!output.contains(canary), "trace leaked {canary}");
        }
    }

    fn attempt(kind: AttemptErrorKind) -> ProviderAttempt {
        ProviderAttempt {
            provider: "fixture",
            target: AttemptTarget::seam("main_search"),
            disposition: AttemptDisposition::Failed,
            error_kind: Some(kind),
            http_status: Some(503),
            duration_ms: 12,
            credential_index: 0,
            retry_count: 0,
            rotation_count: 0,
            message: "failure".into(),
            model: Some("model".into()),
            transport: Some("sse"),
            endpoint_host: Some("example.test".into()),
            breaker_event: None,
        }
    }
}
