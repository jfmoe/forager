//! Typed chain runner shared by every ordered fallback loop.
//!
//! Three levels of the search path perform the same bookkeeping over an
//! ordered step list: the provider chain inside each Capability Seam
//! (`engine`), the model chains (`classifier` and OpenAI-compatible), and the
//! OpenAI-compatible SSE→HTTP transport fallback. This module owns that
//! bookkeeping once: fallback-off head pinning and configured filtering,
//! budget slicing (ADR-0007: [`BudgetPolicy::PrimaryFirst`] for main search,
//! `SlicedEven` for auxiliary seams and the classifier stage cap), synthetic
//! skipped/unconfigured attempt records, attempt and diagnostic merging,
//! quality-gate marking, unconsumed-success semantics, and terminal
//! attribution through [`attempt_trace`]. Each call site keeps declaring its
//! own parameters through [`ChainSettings`]; per ADR-0008 steps run strictly
//! sequentially and no in-flight future is ever dropped.

use std::future::Future;

use crate::attempt_trace;
use crate::net::{combine_diagnostics, slice_budget};
use crate::providers::ProviderError;
use crate::types::{
    AttemptDisposition, AttemptErrorKind, AttemptTarget, Deadline, ProviderAttempt,
};

/// How the runner divides the remaining deadline across the remaining steps.
#[derive(Clone, Copy)]
pub(crate) enum BudgetPolicy {
    /// Main-search semantics (ADR-0007): each step may spend the entire
    /// remaining budget; fallback value concentrates in fast failures.
    PrimaryFirst,
    /// Auxiliary-seam semantics: split the remaining budget evenly across the
    /// remaining steps; a starved slice records a synthetic skip carrying
    /// `skipped_message` and the chain moves on.
    SlicedEven { skipped_message: &'static str },
}

/// How per-step diagnostics collapse into the outcome diagnostic.
#[derive(Clone, Copy)]
pub(crate) enum DiagnosticMerge {
    /// Provider chains: join every present diagnostic in step order.
    Join,
    /// Model and transport chains: the latest present diagnostic wins.
    LatestWins,
}

/// How an exhausted chain becomes a [`ProviderError`].
#[derive(Clone, Copy)]
pub(crate) enum TerminalPolicy {
    /// Provider chains: cross-provider attribution over the whole chain via
    /// [`attempt_trace::terminal_attempt`].
    ChainWide {
        verbose: bool,
        exhausted_message: &'static str,
    },
    /// Model chains: attribute to the most recent failed attempt via
    /// [`attempt_trace::last_failed`].
    Tail {
        verbose: bool,
        default_kind: AttemptErrorKind,
        exhausted_message: &'static str,
    },
    /// Transport chains: keep the last step's error with the merged attempts
    /// and diagnostic; the fallback fields cover a chain that never ran.
    LastError {
        verbose: bool,
        fallback_kind: AttemptErrorKind,
        fallback_message: &'static str,
    },
}

/// The identity a step would record, used for synthetic attempts.
pub(crate) struct StepIdentity {
    pub(crate) provider: &'static str,
    pub(crate) model: Option<String>,
    pub(crate) endpoint_host: Option<String>,
}

/// One ordered step: the run context plus its pre-evaluated gate record.
pub(crate) struct ChainStep<S> {
    pub(crate) context: S,
    /// `false` entries are dropped unless `fallback_off` pins the chain head;
    /// a pinned unconfigured head records an auth failure and ends the chain.
    pub(crate) configured: bool,
    /// Pre-run gate record (e.g. an open model breaker): a present value is
    /// appended before any step runs and the step is excluded from the loop.
    pub(crate) gate_attempt: Option<ProviderAttempt>,
}

/// Per-call-site chain parameters; the runner supplies the bookkeeping.
pub(crate) struct ChainSettings<'a, S> {
    pub(crate) seam: &'static str,
    pub(crate) budget_policy: BudgetPolicy,
    pub(crate) fallback_off: bool,
    pub(crate) diagnostic_merge: DiagnosticMerge,
    pub(crate) terminal: TerminalPolicy,
    /// Identity used for synthetic skipped/unconfigured attempts.
    pub(crate) identity: &'a (dyn Fn(&S) -> StepIdentity + Send + Sync),
    /// Whether the chain continues after a step failure. The transport
    /// fallback restricts this to timeout/network/runtime kinds; every other
    /// chain passes [`always_continue`].
    pub(crate) continue_on_failure: &'a (dyn Fn(&ProviderError) -> bool + Send + Sync),
}

/// Every chain except the transport fallback continues after any failure.
pub(crate) fn always_continue(_: &ProviderError) -> bool {
    true
}

/// A step that produced a value, with its attempts and optional diagnostic.
pub(crate) struct StepSuccess<T> {
    pub(crate) value: T,
    pub(crate) attempts: Vec<ProviderAttempt>,
    pub(crate) diagnostic: Option<String>,
}

/// A step whose value failed the caller's quality gate.
pub(crate) struct StepRejection {
    pub(crate) attempts: Vec<ProviderAttempt>,
    pub(crate) diagnostic: Option<String>,
    pub(crate) kind: AttemptErrorKind,
    pub(crate) message: String,
}

/// Typed acceptance for one step's outcome.
pub(crate) enum StepVerdict<T> {
    /// Accept the value: close out the chain successfully.
    Accepted(StepSuccess<T>),
    /// A legitimate empty result: hold it as the unconsumed success, returned
    /// only when no later step is accepted.
    LegitimateEmpty(StepSuccess<T>),
    /// The step succeeded but failed the quality gate: the runner marks its
    /// last attempt failed and continues the chain like a failure.
    QualityRejected(StepRejection),
    /// The step failed.
    Failed(ProviderError),
}

/// A chain that closed out with an accepted or unconsumed value.
pub(crate) struct ChainOutcome<T> {
    pub(crate) value: T,
    pub(crate) attempts: Vec<ProviderAttempt>,
    pub(crate) diagnostic: Option<String>,
}

/// Runs `steps` in order under `settings`, merging attempts and diagnostics
/// and closing out through the configured terminal policy.
pub(crate) async fn run_chain<S, T, Run, Fut>(
    steps: Vec<ChainStep<S>>,
    settings: ChainSettings<'_, S>,
    deadline: Deadline,
    mut run: Run,
) -> Result<ChainOutcome<T>, ProviderError>
where
    Run: FnMut(S, Deadline) -> Fut,
    Fut: Future<Output = StepVerdict<T>>,
{
    let steps = if settings.fallback_off {
        steps.into_iter().take(1).collect::<Vec<_>>()
    } else {
        steps
            .into_iter()
            .filter(|step| step.configured)
            .collect::<Vec<_>>()
    };
    let total = steps.len();
    // Gates are evaluated for every step before any step runs, so a gate skip
    // is recorded even when an earlier step would close out the chain; the
    // model breaker pre-pass relied on this ordering.
    let mut attempts = Vec::new();
    let mut runnable = Vec::new();
    for (index, step) in steps.into_iter().enumerate() {
        match step.gate_attempt {
            Some(attempt) => attempts.push(attempt),
            None => runnable.push((index, step)),
        }
    }
    let mut diagnostics = Vec::new();
    let mut unconsumed_success = None;
    let mut last_error = None;
    for (index, step) in runnable {
        if !step.configured {
            attempts.push(unconfigured_attempt(
                &(settings.identity)(&step.context),
                settings.seam,
            ));
            break;
        }
        let Some(remaining) = deadline.remaining() else {
            break;
        };
        let budget = match settings.budget_policy {
            BudgetPolicy::PrimaryFirst => remaining,
            BudgetPolicy::SlicedEven { skipped_message } => {
                let Some(budget) = slice_budget(remaining, total - index) else {
                    attempts.push(synthetic_attempt(
                        &(settings.identity)(&step.context),
                        settings.seam,
                        AttemptDisposition::Skipped,
                        None,
                        skipped_message.to_owned(),
                    ));
                    continue;
                };
                budget
            }
        };
        match run(step.context, Deadline::new(budget)).await {
            StepVerdict::Accepted(mut success) => {
                attempts.append(&mut success.attempts);
                diagnostics.push(success.diagnostic);
                return Ok(ChainOutcome {
                    value: success.value,
                    attempts,
                    diagnostic: merge_diagnostics(settings.diagnostic_merge, diagnostics),
                });
            }
            StepVerdict::LegitimateEmpty(mut success) => {
                attempts.append(&mut success.attempts);
                diagnostics.push(success.diagnostic);
                unconsumed_success = Some(success.value);
            }
            StepVerdict::QualityRejected(mut rejection) => {
                mark_last_attempt_failed(
                    &mut rejection.attempts,
                    rejection.kind,
                    rejection.message,
                );
                attempts.append(&mut rejection.attempts);
                diagnostics.push(rejection.diagnostic);
            }
            StepVerdict::Failed(mut error) => {
                attempts.append(&mut error.attempts);
                diagnostics.push(error.diagnostic.take());
                let continues = (settings.continue_on_failure)(&error);
                last_error = Some(error);
                if !continues {
                    break;
                }
            }
        }
    }
    let diagnostic = merge_diagnostics(settings.diagnostic_merge, diagnostics);
    if let Some(value) = unconsumed_success {
        return Ok(ChainOutcome {
            value,
            attempts,
            diagnostic,
        });
    }
    Err(terminal_error(
        settings.terminal,
        attempts,
        diagnostic,
        last_error,
    ))
}

fn unconfigured_attempt(identity: &StepIdentity, seam: &'static str) -> ProviderAttempt {
    let message = format!("{} has no configured credentials", identity.provider);
    synthetic_attempt(
        identity,
        seam,
        AttemptDisposition::Failed,
        Some(AttemptErrorKind::Auth),
        message,
    )
}

/// Marks a step's most recent attempt failed with a quality-gate kind and
/// message; a step that recorded no attempts is left untouched.
pub(crate) fn mark_last_attempt_failed(
    attempts: &mut [ProviderAttempt],
    kind: AttemptErrorKind,
    message: impl Into<String>,
) {
    if let Some(attempt) = attempts.last_mut() {
        attempt.disposition = AttemptDisposition::Failed;
        attempt.error_kind = Some(kind);
        attempt.message = message.into();
    }
}

/// Builds the terminal error of a provider chain: cross-provider attribution
/// over the whole chain, defaulting to a timeout with `exhausted_message`
/// when the chain records no attributable failure.
pub(crate) fn chain_wide_error(
    attempts: Vec<ProviderAttempt>,
    verbose: bool,
    diagnostic: Option<String>,
    exhausted_message: &str,
) -> ProviderError {
    let terminal = attempt_trace::terminal_attempt(&attempts);
    ProviderError {
        kind: terminal
            .and_then(|attempt| attempt.error_kind)
            .unwrap_or(AttemptErrorKind::Timeout),
        message: terminal.map_or_else(
            || exhausted_message.to_owned(),
            |attempt| attempt.message.clone(),
        ),
        attempts,
        verbose,
        diagnostic,
        redirected_library_id: None,
    }
}

fn terminal_error(
    policy: TerminalPolicy,
    attempts: Vec<ProviderAttempt>,
    diagnostic: Option<String>,
    last_error: Option<ProviderError>,
) -> ProviderError {
    match policy {
        TerminalPolicy::ChainWide {
            verbose,
            exhausted_message,
        } => chain_wide_error(attempts, verbose, diagnostic, exhausted_message),
        TerminalPolicy::Tail {
            verbose,
            default_kind,
            exhausted_message,
        } => {
            let terminal = attempt_trace::last_failed(&attempts);
            ProviderError {
                kind: terminal
                    .and_then(|attempt| attempt.error_kind)
                    .unwrap_or(default_kind),
                message: terminal.map_or_else(
                    || exhausted_message.to_owned(),
                    |attempt| attempt.message.clone(),
                ),
                attempts,
                verbose,
                diagnostic,
                redirected_library_id: None,
            }
        }
        TerminalPolicy::LastError {
            verbose,
            fallback_kind,
            fallback_message,
        } => match last_error {
            Some(mut error) => {
                error.attempts = attempts;
                error.diagnostic = diagnostic;
                error
            }
            None => ProviderError {
                kind: fallback_kind,
                message: fallback_message.to_owned(),
                attempts,
                verbose,
                diagnostic,
                redirected_library_id: None,
            },
        },
    }
}

fn merge_diagnostics(policy: DiagnosticMerge, diagnostics: Vec<Option<String>>) -> Option<String> {
    let mut present = diagnostics.into_iter().flatten();
    match policy {
        DiagnosticMerge::Join => combine_diagnostics(present),
        DiagnosticMerge::LatestWins => present.next_back(),
    }
}

fn synthetic_attempt(
    identity: &StepIdentity,
    seam: &'static str,
    disposition: AttemptDisposition,
    error_kind: Option<AttemptErrorKind>,
    message: String,
) -> ProviderAttempt {
    ProviderAttempt {
        provider: identity.provider,
        target: AttemptTarget::seam(seam),
        disposition,
        error_kind,
        http_status: None,
        duration_ms: 0,
        credential_index: 0,
        retry_count: 0,
        rotation_count: 0,
        message,
        model: identity.model.clone(),
        transport: None,
        endpoint_host: identity.endpoint_host.clone(),
        breaker_event: None,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::time::Duration;

    use super::{
        BudgetPolicy, ChainOutcome, ChainSettings, ChainStep, DiagnosticMerge, StepIdentity,
        StepRejection, StepSuccess, StepVerdict, TerminalPolicy, always_continue, chain_wide_error,
        run_chain,
    };
    use crate::providers::ProviderError;
    use crate::types::{
        AttemptDisposition, AttemptErrorKind, AttemptTarget, Deadline, ProviderAttempt,
    };

    #[derive(Default)]
    struct Script {
        ran: Vec<&'static str>,
        budgets: Vec<Duration>,
        verdicts: VecDeque<StepVerdict<&'static str>>,
    }

    fn step(name: &'static str) -> ChainStep<&'static str> {
        ChainStep {
            context: name,
            configured: true,
            gate_attempt: None,
        }
    }

    fn identity(name: &&'static str) -> StepIdentity {
        StepIdentity {
            provider: name,
            model: None,
            endpoint_host: None,
        }
    }

    fn settings() -> ChainSettings<'static, &'static str> {
        ChainSettings {
            seam: "main_search",
            budget_policy: BudgetPolicy::PrimaryFirst,
            fallback_off: false,
            diagnostic_merge: DiagnosticMerge::Join,
            terminal: TerminalPolicy::ChainWide {
                verbose: false,
                exhausted_message: "chain exhausted",
            },
            identity: &identity,
            continue_on_failure: &always_continue,
        }
    }

    fn drive(
        steps: Vec<ChainStep<&'static str>>,
        settings: ChainSettings<'_, &'static str>,
        deadline_seconds: u64,
        script: &RefCell<Script>,
    ) -> Result<ChainOutcome<&'static str>, ProviderError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("test runtime");
        runtime.block_on(run_chain(
            steps,
            settings,
            Deadline::new(Duration::from_secs(deadline_seconds)),
            |name, deadline| {
                let verdict = {
                    let mut script = script.borrow_mut();
                    script.ran.push(name);
                    script
                        .budgets
                        .push(deadline.remaining().unwrap_or_default());
                    script.verdicts.pop_front().expect("scripted verdict")
                };
                async move { verdict }
            },
        ))
    }

    fn attempt(provider: &'static str) -> ProviderAttempt {
        ProviderAttempt {
            provider,
            target: AttemptTarget::seam("main_search"),
            disposition: AttemptDisposition::Succeeded,
            error_kind: None,
            http_status: Some(200),
            duration_ms: 1,
            credential_index: 0,
            retry_count: 0,
            rotation_count: 0,
            message: String::new(),
            model: None,
            transport: None,
            endpoint_host: None,
            breaker_event: None,
        }
    }

    fn failed_attempt(
        provider: &'static str,
        kind: AttemptErrorKind,
        message: &str,
    ) -> ProviderAttempt {
        let mut attempt = attempt(provider);
        attempt.disposition = AttemptDisposition::Failed;
        attempt.error_kind = Some(kind);
        attempt.message = message.into();
        attempt
    }

    fn accepted(
        value: &'static str,
        provider: &'static str,
        diagnostic: Option<&str>,
    ) -> StepVerdict<&'static str> {
        StepVerdict::Accepted(StepSuccess {
            value,
            attempts: vec![attempt(provider)],
            diagnostic: diagnostic.map(str::to_owned),
        })
    }

    fn failed(
        provider: &'static str,
        kind: AttemptErrorKind,
        message: &str,
        diagnostic: Option<&str>,
    ) -> StepVerdict<&'static str> {
        StepVerdict::Failed(ProviderError {
            kind,
            message: message.into(),
            attempts: vec![failed_attempt(provider, kind, message)],
            verbose: false,
            diagnostic: diagnostic.map(str::to_owned),
            redirected_library_id: None,
        })
    }

    #[test]
    fn fallback_off_runs_only_the_chain_head() {
        let mut settings = settings();
        settings.fallback_off = true;
        let script = RefCell::new(Script {
            verdicts: VecDeque::from([accepted("head", "a", None)]),
            ..Script::default()
        });

        let outcome =
            drive(vec![step("a"), step("b")], settings, 60, &script).expect("chain outcome");

        assert_eq!(
            (outcome.value, script.borrow().ran.clone()),
            ("head", vec!["a"])
        );
    }

    #[test]
    fn fallback_off_records_an_unconfigured_head_as_an_auth_failure() {
        let mut settings = settings();
        settings.fallback_off = true;
        let script = RefCell::new(Script::default());
        let steps = vec![
            ChainStep {
                context: "a",
                configured: false,
                gate_attempt: None,
            },
            step("b"),
        ];

        let error = drive(steps, settings, 60, &script)
            .err()
            .expect("chain error");

        assert_eq!(
            (
                error.kind,
                error.message.as_str(),
                error.attempts.len(),
                error.attempts[0].disposition,
                script.borrow().ran.clone(),
            ),
            (
                AttemptErrorKind::Auth,
                "a has no configured credentials",
                1,
                AttemptDisposition::Failed,
                Vec::new(),
            )
        );
    }

    #[test]
    fn unconfigured_steps_are_dropped_silently_when_fallback_is_allowed() {
        let script = RefCell::new(Script {
            verdicts: VecDeque::from([
                failed("a", AttemptErrorKind::Network, "down", None),
                accepted("tail", "c", None),
            ]),
            ..Script::default()
        });
        let steps = vec![
            step("a"),
            ChainStep {
                context: "b",
                configured: false,
                gate_attempt: None,
            },
            step("c"),
        ];

        let outcome = drive(steps, settings(), 60, &script).expect("chain outcome");

        assert_eq!(
            (
                outcome.value,
                outcome
                    .attempts
                    .iter()
                    .map(|attempt| attempt.provider)
                    .collect::<Vec<_>>(),
                script.borrow().ran.clone(),
            ),
            ("tail", vec!["a", "c"], vec!["a", "c"])
        );
    }

    #[test]
    fn sliced_even_skips_starved_slices_but_gives_the_last_slot_the_full_remainder() {
        let mut settings = settings();
        settings.budget_policy = BudgetPolicy::SlicedEven {
            skipped_message: "skipped to preserve fallback deadline budget",
        };
        let script = RefCell::new(Script {
            verdicts: VecDeque::from([
                failed("b", AttemptErrorKind::Network, "down", None),
                accepted("tail", "c", None),
            ]),
            ..Script::default()
        });

        let outcome = drive(vec![step("a"), step("b"), step("c")], settings, 14, &script)
            .expect("chain outcome");
        let script = script.borrow();

        assert_eq!(
            (
                outcome.attempts[0].disposition,
                outcome.attempts[0].message.as_str(),
                script.ran.clone(),
                outcome.attempts.len(),
            ),
            (
                AttemptDisposition::Skipped,
                "skipped to preserve fallback deadline budget",
                vec!["b", "c"],
                3,
            )
        );
        assert!(
            script.budgets[0] > Duration::from_secs(6)
                && script.budgets[0] < Duration::from_secs(8),
            "second slot budget: {:?}",
            script.budgets[0]
        );
        assert!(
            script.budgets[1] > Duration::from_secs(13),
            "last slot budget: {:?}",
            script.budgets[1]
        );
    }

    #[test]
    fn primary_first_gives_every_step_the_full_remaining_budget() {
        let script = RefCell::new(Script {
            verdicts: VecDeque::from([
                failed("a", AttemptErrorKind::Network, "down", None),
                failed("b", AttemptErrorKind::Auth, "denied", None),
            ]),
            ..Script::default()
        });

        let error = drive(vec![step("a"), step("b")], settings(), 60, &script)
            .err()
            .expect("chain error");
        let budgets = script.borrow().budgets.clone();

        assert_eq!(error.attempts.len(), 2);
        assert!(
            budgets
                .iter()
                .all(|budget| *budget > Duration::from_secs(59)),
            "budgets: {budgets:?}"
        );
    }

    #[test]
    fn acceptance_stops_the_chain_and_joins_diagnostics_in_step_order() {
        let script = RefCell::new(Script {
            verdicts: VecDeque::from([
                failed("a", AttemptErrorKind::Network, "down", Some("first")),
                accepted("winner", "b", Some("second")),
            ]),
            ..Script::default()
        });

        let outcome = drive(
            vec![step("a"), step("b"), step("c")],
            settings(),
            60,
            &script,
        )
        .expect("chain outcome");

        assert_eq!(
            (
                outcome.value,
                outcome.diagnostic.as_deref(),
                outcome.attempts.len(),
                script.borrow().ran.clone(),
            ),
            ("winner", Some("first\nsecond"), 2, vec!["a", "b"])
        );
    }

    #[test]
    fn a_legitimate_empty_is_returned_with_every_attempt_when_no_step_is_accepted() {
        let script = RefCell::new(Script {
            verdicts: VecDeque::from([
                StepVerdict::LegitimateEmpty(StepSuccess {
                    value: "empty",
                    attempts: vec![attempt("a")],
                    diagnostic: Some("empty diagnostic".into()),
                }),
                failed(
                    "b",
                    AttemptErrorKind::Network,
                    "down",
                    Some("failure diagnostic"),
                ),
            ]),
            ..Script::default()
        });

        let outcome =
            drive(vec![step("a"), step("b")], settings(), 60, &script).expect("chain outcome");

        assert_eq!(
            (
                outcome.value,
                outcome.diagnostic.as_deref(),
                outcome
                    .attempts
                    .iter()
                    .map(|attempt| attempt.provider)
                    .collect::<Vec<_>>(),
            ),
            (
                "empty",
                Some("empty diagnostic\nfailure diagnostic"),
                vec!["a", "b"],
            )
        );
    }

    #[test]
    fn a_later_acceptance_wins_over_an_earlier_legitimate_empty() {
        let script = RefCell::new(Script {
            verdicts: VecDeque::from([
                StepVerdict::LegitimateEmpty(StepSuccess {
                    value: "empty",
                    attempts: vec![attempt("a")],
                    diagnostic: None,
                }),
                accepted("winner", "b", None),
            ]),
            ..Script::default()
        });

        let outcome =
            drive(vec![step("a"), step("b")], settings(), 60, &script).expect("chain outcome");

        assert_eq!(outcome.value, "winner");
    }

    #[test]
    fn a_quality_reject_marks_the_last_attempt_failed_and_continues_the_chain() {
        let script = RefCell::new(Script {
            verdicts: VecDeque::from([
                StepVerdict::QualityRejected(StepRejection {
                    attempts: vec![attempt("a")],
                    diagnostic: None,
                    kind: AttemptErrorKind::Quality,
                    message: "too thin".into(),
                }),
                accepted("winner", "b", None),
            ]),
            ..Script::default()
        });

        let outcome =
            drive(vec![step("a"), step("b")], settings(), 60, &script).expect("chain outcome");

        assert_eq!(
            (
                outcome.value,
                outcome.attempts[0].disposition,
                outcome.attempts[0].error_kind,
                outcome.attempts[0].message.as_str(),
            ),
            (
                "winner",
                AttemptDisposition::Failed,
                Some(AttemptErrorKind::Quality),
                "too thin",
            )
        );
    }

    #[test]
    fn a_quality_reject_becomes_the_chainwide_terminal_when_the_chain_exhausts() {
        let script = RefCell::new(Script {
            verdicts: VecDeque::from([StepVerdict::QualityRejected(StepRejection {
                attempts: vec![attempt("a")],
                diagnostic: None,
                kind: AttemptErrorKind::Quality,
                message: "too thin".into(),
            })]),
            ..Script::default()
        });

        let error = drive(vec![step("a")], settings(), 60, &script)
            .err()
            .expect("chain error");

        assert_eq!(
            (error.kind, error.message.as_str()),
            (AttemptErrorKind::Quality, "too thin")
        );
    }

    #[test]
    fn a_quality_reject_without_attempts_falls_back_to_the_terminal_defaults() {
        let script = RefCell::new(Script {
            verdicts: VecDeque::from([StepVerdict::QualityRejected(StepRejection {
                attempts: Vec::new(),
                diagnostic: None,
                kind: AttemptErrorKind::Quality,
                message: "too thin".into(),
            })]),
            ..Script::default()
        });

        let error = drive(vec![step("a")], settings(), 60, &script)
            .err()
            .expect("chain error");

        assert_eq!(
            (error.kind, error.message.as_str()),
            (AttemptErrorKind::Timeout, "chain exhausted")
        );
    }

    #[test]
    fn chainwide_terminal_defaults_to_timeout_with_the_exhausted_message_on_an_empty_chain() {
        let script = RefCell::new(Script::default());

        let error = drive(Vec::new(), settings(), 60, &script)
            .err()
            .expect("chain error");

        assert_eq!(
            (error.kind, error.message.as_str(), error.attempts.len()),
            (AttemptErrorKind::Timeout, "chain exhausted", 0)
        );
    }

    #[test]
    fn tail_terminal_attributes_to_the_most_recent_failed_attempt() {
        let mut settings = settings();
        settings.terminal = TerminalPolicy::Tail {
            verbose: true,
            default_kind: AttemptErrorKind::Runtime,
            exhausted_message: "no executable models",
        };
        let script = RefCell::new(Script {
            verdicts: VecDeque::from([
                failed("a", AttemptErrorKind::Network, "net down", None),
                failed("b", AttemptErrorKind::Auth, "denied", None),
            ]),
            ..Script::default()
        });

        let error = drive(vec![step("a"), step("b")], settings, 60, &script)
            .err()
            .expect("chain error");

        assert_eq!(
            (error.kind, error.message.as_str(), error.verbose),
            (AttemptErrorKind::Auth, "denied", true)
        );
    }

    #[test]
    fn tail_terminal_uses_its_defaults_when_no_attempt_failed() {
        let mut settings = settings();
        settings.terminal = TerminalPolicy::Tail {
            verbose: false,
            default_kind: AttemptErrorKind::Runtime,
            exhausted_message: "no executable models",
        };
        let script = RefCell::new(Script::default());

        let error = drive(Vec::new(), settings, 60, &script)
            .err()
            .expect("chain error");

        assert_eq!(
            (error.kind, error.message.as_str()),
            (AttemptErrorKind::Runtime, "no executable models")
        );
    }

    #[test]
    fn last_error_terminal_preserves_the_last_step_error_with_merged_records() {
        let mut settings = settings();
        settings.diagnostic_merge = DiagnosticMerge::LatestWins;
        settings.terminal = TerminalPolicy::LastError {
            verbose: true,
            fallback_kind: AttemptErrorKind::Timeout,
            fallback_message: "request failed",
        };
        let script = RefCell::new(Script {
            verdicts: VecDeque::from([
                failed(
                    "a",
                    AttemptErrorKind::Network,
                    "stream failed",
                    Some("stream diagnostic"),
                ),
                StepVerdict::Failed(ProviderError {
                    kind: AttemptErrorKind::Auth,
                    message: "http failed".into(),
                    attempts: vec![failed_attempt("b", AttemptErrorKind::Auth, "http failed")],
                    verbose: true,
                    diagnostic: None,
                    redirected_library_id: None,
                }),
            ]),
            ..Script::default()
        });

        let error = drive(vec![step("a"), step("b")], settings, 60, &script)
            .err()
            .expect("chain error");

        assert_eq!(
            (
                error.kind,
                error.message.as_str(),
                error.verbose,
                error.diagnostic.as_deref(),
                error
                    .attempts
                    .iter()
                    .map(|attempt| attempt.provider)
                    .collect::<Vec<_>>(),
            ),
            (
                AttemptErrorKind::Auth,
                "http failed",
                true,
                Some("stream diagnostic"),
                vec!["a", "b"],
            )
        );
    }

    #[test]
    fn the_failure_gate_stops_the_chain_before_non_continuable_kinds() {
        let mut settings = settings();
        settings.terminal = TerminalPolicy::LastError {
            verbose: false,
            fallback_kind: AttemptErrorKind::Timeout,
            fallback_message: "request failed",
        };
        settings.continue_on_failure =
            &|error: &ProviderError| error.kind == AttemptErrorKind::Network;
        let script = RefCell::new(Script {
            verdicts: VecDeque::from([failed("a", AttemptErrorKind::Auth, "denied", None)]),
            ..Script::default()
        });

        let error = drive(vec![step("a"), step("b")], settings, 60, &script)
            .err()
            .expect("chain error");

        assert_eq!(
            (
                error.kind,
                error.attempts.len(),
                script.borrow().ran.clone()
            ),
            (AttemptErrorKind::Auth, 1, vec!["a"])
        );
    }

    #[test]
    fn gate_attempts_are_recorded_before_any_step_runs() {
        let mut gated_a = step("a");
        gated_a.gate_attempt = Some({
            let mut attempt = attempt("a");
            attempt.disposition = AttemptDisposition::Skipped;
            attempt.message = "gate a".into();
            attempt
        });
        let mut gated_c = step("c");
        gated_c.gate_attempt = Some({
            let mut attempt = attempt("c");
            attempt.disposition = AttemptDisposition::Skipped;
            attempt.message = "gate c".into();
            attempt
        });
        let script = RefCell::new(Script {
            verdicts: VecDeque::from([accepted("winner", "b", None)]),
            ..Script::default()
        });

        let outcome = drive(vec![gated_a, step("b"), gated_c], settings(), 60, &script)
            .expect("chain outcome");

        assert_eq!(
            (
                outcome
                    .attempts
                    .iter()
                    .map(|attempt| attempt.message.as_str())
                    .collect::<Vec<_>>(),
                script.borrow().ran.clone(),
            ),
            (vec!["gate a", "gate c", ""], vec!["b"])
        );
    }

    #[test]
    fn latest_wins_diagnostic_merge_keeps_the_last_present_value() {
        let mut settings = settings();
        settings.diagnostic_merge = DiagnosticMerge::LatestWins;
        let script = RefCell::new(Script {
            verdicts: VecDeque::from([
                failed("a", AttemptErrorKind::Network, "down", Some("first")),
                accepted("winner", "b", None),
            ]),
            ..Script::default()
        });

        let outcome =
            drive(vec![step("a"), step("b")], settings, 60, &script).expect("chain outcome");

        assert_eq!(outcome.diagnostic.as_deref(), Some("first"));
    }

    #[test]
    fn chain_wide_error_uses_kind_and_message_from_the_same_terminal_attempt() {
        let attempts = vec![
            failed_attempt("tavily", AttemptErrorKind::Quality, "thin content"),
            failed_attempt("jina", AttemptErrorKind::Network, "connection reset"),
        ];

        let error = chain_wide_error(attempts, false, None, "web fetch deadline elapsed");

        assert_eq!(
            (error.kind, error.message.as_str()),
            (AttemptErrorKind::Quality, "thin content")
        );
    }
}
