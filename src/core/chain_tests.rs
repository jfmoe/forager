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

    let outcome = drive(vec![step("a"), step("b")], settings, 60, &script).expect("chain outcome");

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

    let outcome =
        drive(vec![step("a"), step("b"), step("c")], settings, 14, &script).expect("chain outcome");
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
        script.budgets[0] > Duration::from_secs(6) && script.budgets[0] < Duration::from_secs(8),
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
                attempts: vec![attempt("a-first"), attempt("a-last")],
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
            outcome.attempts[1].disposition,
            outcome.attempts[1].error_kind,
            outcome.attempts[1].message.as_str(),
        ),
        (
            "winner",
            AttemptDisposition::Succeeded,
            None,
            "",
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
    settings.continue_on_failure = &|error: &ProviderError| error.kind == AttemptErrorKind::Network;
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

    let outcome =
        drive(vec![gated_a, step("b"), gated_c], settings(), 60, &script).expect("chain outcome");

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

    let outcome = drive(vec![step("a"), step("b")], settings, 60, &script).expect("chain outcome");

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
