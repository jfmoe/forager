//! Derived facts over a provider attempt chain.
//!
//! This module is the single owner of every read-only fact derived from a
//! `&[ProviderAttempt]` chain: cross-provider terminal attribution
//! ([`terminal_attempt`] / [`terminal_kind`]), in-chain tail attribution
//! ([`last_failed`]), successful-provider extraction
//! ([`successful_provider`]), both fallback definitions
//! ([`provider_fallback_used`] / [`identity_fallback_occurred`]), and the
//! bounded attempt summary ([`bounded_attempt_summary`]). Consumers — the
//! Search Result Journal, the stderr attempt log, the Research Recovery
//! Manifest, and exit-code attribution — read projections from here instead
//! of re-interpreting the raw chain.
//!
//! The two fallback definitions deliberately differ:
//! [`provider_fallback_used`] answers "did we leave the primary provider"
//! for the Recovery Manifest, while [`identity_fallback_occurred`] answers
//! "did any degradation happen, including a same-provider model switch" for
//! the stderr attempt log. On a same-provider model-switch chain they
//! legitimately disagree; neither answer is a bug.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde_json::{Value, json};

use crate::types::{AttemptDisposition, AttemptErrorKind, ProviderAttempt};

/// Returns the terminal attempt for cross-provider attribution: the final
/// failed attempt per provider, then the maximum by
/// `(error_priority, chain index)` so a later provider wins on kind ties.
pub(crate) fn terminal_attempt(attempts: &[ProviderAttempt]) -> Option<&ProviderAttempt> {
    let mut final_providers = HashSet::new();
    attempts
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, attempt)| final_providers.insert(attempt.provider))
        .filter(|(_, attempt)| attempt.disposition == AttemptDisposition::Failed)
        .filter_map(|(index, attempt)| attempt.error_kind.map(|kind| (index, attempt, kind)))
        .max_by_key(|(index, _, kind)| (error_priority(*kind), *index))
        .map(|(_, attempt, _)| attempt)
}

/// Returns the [`AttemptErrorKind`] attributed by [`terminal_attempt`],
/// defaulting to [`AttemptErrorKind::Timeout`] when the chain records no
/// attributable failure.
pub(crate) fn terminal_kind(attempts: &[ProviderAttempt]) -> AttemptErrorKind {
    terminal_attempt(attempts)
        .and_then(|attempt| attempt.error_kind)
        .unwrap_or(AttemptErrorKind::Timeout)
}

// The match stays exhaustive on purpose: adding an AttemptErrorKind variant
// must force a compile-time decision about its attribution priority.
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

/// Returns the most recent failed attempt, used to close out a single
/// provider's internal model chain (OpenAI-compatible and classifier).
pub(crate) fn last_failed(attempts: &[ProviderAttempt]) -> Option<&ProviderAttempt> {
    attempts
        .iter()
        .rev()
        .find(|attempt| attempt.disposition == AttemptDisposition::Failed)
}

/// Returns the provider of the last successful attempt within `seam`, or
/// `None` when the seam recorded no success.
pub(crate) fn successful_provider(
    attempts: &[ProviderAttempt],
    seam: &str,
) -> Option<&'static str> {
    attempts
        .iter()
        .rev()
        .find(|attempt| {
            attempt.target.seam_name() == Some(seam)
                && attempt.disposition == AttemptDisposition::Succeeded
        })
        .map(|attempt| attempt.provider)
}

/// Returns whether at least two distinct providers ran within one
/// [`AttemptTarget::Seam`](crate::types::AttemptTarget::Seam).
///
/// This is the Research Recovery Manifest `fallback.used` semantics: "did we
/// leave the primary provider". A same-provider model switch does not count
/// here; contrast [`identity_fallback_occurred`].
pub(crate) fn provider_fallback_used(attempts: &[ProviderAttempt]) -> bool {
    let mut providers_by_seam = HashMap::<&str, HashSet<&str>>::new();
    for attempt in attempts {
        if let Some(seam) = attempt.target.seam_name() {
            providers_by_seam
                .entry(seam)
                .or_default()
                .insert(attempt.provider);
        }
    }
    providers_by_seam
        .values()
        .any(|providers| providers.len() > 1)
}

/// Returns whether the `(provider, model)` identity changed within the same
/// [`AttemptTarget`](crate::types::AttemptTarget).
///
/// This is the stderr attempt log `fallback` semantics: "did any degradation
/// happen, including a same-provider model switch". Contrast
/// [`provider_fallback_used`], which only observes cross-provider movement
/// within a seam.
pub(crate) fn identity_fallback_occurred(attempts: &[ProviderAttempt]) -> bool {
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

/// Bounded JSON summary of the attempt chain: total count, per-kind counts
/// and the provider set, each capped at 8 entries with explicit truncation
/// flags.
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

#[cfg(test)]
mod tests {
    use super::{
        bounded_attempt_summary, identity_fallback_occurred, last_failed, provider_fallback_used,
        successful_provider, terminal_attempt, terminal_kind,
    };
    use crate::types::{AttemptDisposition, AttemptErrorKind, AttemptTarget, ProviderAttempt};

    const ALL_KINDS: [AttemptErrorKind; 9] = [
        AttemptErrorKind::Network,
        AttemptErrorKind::Timeout,
        AttemptErrorKind::RateLimited,
        AttemptErrorKind::QuotaExhausted,
        AttemptErrorKind::Auth,
        AttemptErrorKind::Parameter,
        AttemptErrorKind::Runtime,
        AttemptErrorKind::Quality,
        AttemptErrorKind::Evidence,
    ];

    fn attempt(provider: &'static str, kind: AttemptErrorKind) -> ProviderAttempt {
        ProviderAttempt {
            provider,
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

    fn succeeded(provider: &'static str, seam: &'static str) -> ProviderAttempt {
        let mut attempt = attempt(provider, AttemptErrorKind::Network);
        attempt.target = AttemptTarget::seam(seam);
        attempt.disposition = AttemptDisposition::Succeeded;
        attempt.error_kind = None;
        attempt
    }

    #[test]
    fn terminal_kind_exhausts_final_kind_pairs_and_ignores_history() {
        for first in ALL_KINDS {
            for second in ALL_KINDS {
                let expected = ALL_KINDS
                    .iter()
                    .rposition(|kind| *kind == first || *kind == second)
                    .map(|index| ALL_KINDS[index])
                    .expect("pair contains a specified kind");
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
        let mut first = attempt("xai", AttemptErrorKind::Auth);
        first.message = "first".into();
        let mut second = attempt("openai_compatible", AttemptErrorKind::Auth);
        second.message = "second".into();
        let attempts = vec![first, second];

        assert_eq!(
            terminal_attempt(&attempts).map(|attempt| attempt.message.as_str()),
            Some("second")
        );
    }

    #[test]
    fn last_failed_returns_the_most_recent_failed_attempt() {
        let first = attempt("fixture", AttemptErrorKind::Network);
        let mut second = attempt("fixture", AttemptErrorKind::Auth);
        second.message = "latest failure".into();
        let third = succeeded("fixture", "main_search");

        assert_eq!(
            last_failed(&[first, second, third]).map(|attempt| attempt.message.as_str()),
            Some("latest failure")
        );
    }

    #[test]
    fn successful_provider_returns_the_last_success_within_the_seam() {
        let failed = attempt("tavily", AttemptErrorKind::Network);
        let first_success = succeeded("exa", "main_search");
        let last_success = succeeded("jina", "main_search");

        assert_eq!(
            successful_provider(&[failed, first_success, last_success], "main_search"),
            Some("jina")
        );
    }

    #[test]
    fn successful_provider_ignores_successes_in_other_seams() {
        let success = succeeded("exa", "vertical_search");

        assert_eq!(successful_provider(&[success], "main_search"), None);
    }

    #[test]
    fn successful_provider_returns_none_when_the_seam_has_no_success() {
        let failed = attempt("tavily", AttemptErrorKind::Network);

        assert_eq!(successful_provider(&[failed], "main_search"), None);
    }

    #[test]
    fn provider_fallback_used_detects_two_providers_within_one_seam() {
        let first = attempt("tavily", AttemptErrorKind::Network);
        let second = attempt("jina", AttemptErrorKind::Network);

        assert!(provider_fallback_used(&[first, second]));
    }

    #[test]
    fn provider_fallback_used_ignores_a_same_provider_model_switch() {
        let first = attempt("fixture", AttemptErrorKind::Runtime);
        let mut second = attempt("fixture", AttemptErrorKind::Network);
        second.model = Some("fallback-model".into());

        assert!(!provider_fallback_used(&[first, second]));
    }

    #[test]
    fn provider_fallback_used_ignores_operation_targets() {
        let mut first = attempt("tavily", AttemptErrorKind::Network);
        first.target = AttemptTarget::operation("context7_resolve");
        let mut second = attempt("jina", AttemptErrorKind::Network);
        second.target = AttemptTarget::operation("context7_resolve");

        assert!(!provider_fallback_used(&[first, second]));
    }

    #[test]
    fn identity_fallback_occurred_detects_a_same_target_model_change() {
        let first = attempt("fixture", AttemptErrorKind::Runtime);
        let mut second = attempt("fixture", AttemptErrorKind::Network);
        second.model = Some("fallback-model".into());

        assert!(identity_fallback_occurred(&[first, second]));
    }

    #[test]
    fn identity_fallback_occurred_ignores_a_seam_transition() {
        let mut classifier = attempt("classifier", AttemptErrorKind::Runtime);
        classifier.target = AttemptTarget::seam("classifier");
        classifier.model = Some("classifier-model".into());
        let main_search = attempt("fixture", AttemptErrorKind::Network);

        assert!(!identity_fallback_occurred(&[classifier, main_search]));
    }

    #[test]
    fn fallback_definitions_diverge_on_a_same_provider_model_switch() {
        let first = attempt("fixture", AttemptErrorKind::Runtime);
        let mut second = attempt("fixture", AttemptErrorKind::Network);
        second.model = Some("fallback-model".into());
        let attempts = [first, second];

        assert!(!provider_fallback_used(&attempts));
        assert!(identity_fallback_occurred(&attempts));
    }

    #[test]
    fn bounded_attempt_summary_preserves_the_by_kind_boundary() {
        for (case, kind_count, truncated) in
            [("exactly_eight", 8, false), ("one_above_eight", 9, true)]
        {
            let attempts = ALL_KINDS[..kind_count]
                .iter()
                .map(|&kind| attempt("fixture", kind))
                .collect::<Vec<_>>();
            let summary = bounded_attempt_summary(&attempts);

            assert_eq!(
                (
                    summary["by_kind"]
                        .as_object()
                        .map_or(0, serde_json::Map::len),
                    summary["by_kind_truncated"].as_bool(),
                    summary["truncated"].as_bool(),
                ),
                (8, Some(truncated), Some(truncated)),
                "case={case}"
            );
        }
    }

    #[test]
    fn bounded_attempt_summary_preserves_the_provider_boundary() {
        let providers = ["p0", "p1", "p2", "p3", "p4", "p5", "p6", "p7"];
        for (case, extra_provider, truncated) in [
            ("exactly_eight", None, false),
            ("one_above_eight", Some("p8"), true),
        ] {
            let attempts = providers
                .into_iter()
                .chain(extra_provider)
                .map(|provider| attempt(provider, AttemptErrorKind::Network))
                .collect::<Vec<_>>();
            let summary = bounded_attempt_summary(&attempts);

            assert_eq!(
                (
                    summary["providers"].as_array().map_or(0, Vec::len),
                    summary["providers_truncated"].as_bool(),
                    summary["truncated"].as_bool(),
                ),
                (8, Some(truncated), Some(truncated)),
                "case={case}"
            );
        }
    }
}
