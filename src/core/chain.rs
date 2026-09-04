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
    /// Never combine with [`BudgetPolicy::SlicedEven`]: the slice divisor
    /// counts gate-excluded steps.
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

struct PreparedChain<S> {
    total: usize,
    attempts: Vec<ProviderAttempt>,
    runnable: Vec<(usize, ChainStep<S>)>,
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
    // SlicedEven slices by `total - index`, which counts gate-excluded steps;
    // every current call site pairs gates with PrimaryFirst, so reject the
    // combination loudly instead of silently slicing conservatively.
    debug_assert!(
        matches!(settings.budget_policy, BudgetPolicy::PrimaryFirst)
            || steps.iter().all(|step| step.gate_attempt.is_none()),
        "gate_attempt steps distort SlicedEven budget slicing"
    );
    let PreparedChain {
        total,
        mut attempts,
        runnable,
    } = prepare_steps(steps, settings.fallback_off);
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

fn prepare_steps<S>(steps: Vec<ChainStep<S>>, fallback_off: bool) -> PreparedChain<S> {
    let steps = if fallback_off {
        steps.into_iter().take(1).collect::<Vec<_>>()
    } else {
        steps
            .into_iter()
            .filter(|step| step.configured)
            .collect::<Vec<_>>()
    };
    let total = steps.len();
    // Gate attempts are recorded before the chain runs, preserving the model
    // breaker pre-pass ordering even when an earlier runnable step succeeds.
    let mut attempts = Vec::new();
    let mut runnable = Vec::new();
    for (index, step) in steps.into_iter().enumerate() {
        match step.gate_attempt {
            Some(attempt) => attempts.push(attempt),
            None => runnable.push((index, step)),
        }
    }
    PreparedChain {
        total,
        attempts,
        runnable,
    }
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
#[path = "chain_tests.rs"]
mod tests;
