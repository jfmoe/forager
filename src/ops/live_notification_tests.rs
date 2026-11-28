use serde_json::{Value, json};

use super::decide;
use crate::live::record::PastRun;

/// A recorded run of `tier` at day `day` of the month, scheduled or not, with `checks` as the
/// record stores them.
fn run_of(tier: &str, day: u32, scheduled: bool, checks: Value) -> PastRun {
    let mut record = json!({
        "recorded_at": format!("2026-11-{day:02}T06:00:00Z"),
        "scheduled": scheduled,
        "tiers": [tier],
    });
    record["checks"] = checks;
    serde_json::from_value(record).expect("a run record")
}

fn run(day: u32, scheduled: bool, checks: Value) -> PastRun {
    run_of("metered", day, scheduled, checks)
}

fn scheduled(day: u32, checks: Value) -> PastRun {
    run(day, true, checks)
}

fn passed(id: &str) -> Value {
    json!({"id": id, "status": "passed", "attempts": 1})
}

/// A confirmed failure after its rerun.
fn failed(id: &str, error_kind: &str) -> Value {
    json!({
        "id": id, "status": "failed", "attempts": 2, "rerun": true, "confirmed": true,
        "error_kind": error_kind, "http_status": 502,
    })
}

/// Each provider's action, as the report names it, when `current` follows `history`.
fn actions(history: &[PastRun], current: &PastRun) -> Vec<(String, String)> {
    decide(history, current)
        .into_iter()
        .map(|notification| {
            let action = serde_json::to_value(notification.action).expect("serialize an action");
            (
                notification.provider,
                action.as_str().expect("an action name").to_owned(),
            )
        })
        .collect()
}

fn one(provider: &str, action: &str) -> Vec<(String, String)> {
    vec![(provider.to_owned(), action.to_owned())]
}

const NONE: Vec<(String, String)> = Vec::new();

#[test]
fn a_new_confirmed_failure_opens_its_providers_issue() {
    let history = [scheduled(
        1,
        json!([passed("exa:search"), passed("jina:fetch")]),
    )];
    let current = scheduled(
        2,
        json!([failed("exa:search", "network"), passed("jina:fetch")]),
    );

    assert_eq!(actions(&history, &current), one("exa", "open"));
}

#[test]
fn the_same_failure_again_changes_nothing() {
    let history = [scheduled(1, json!([failed("exa:search", "network")]))];
    let current = scheduled(2, json!([failed("exa:search", "network")]));

    assert_eq!(actions(&history, &current), NONE);
}

#[test]
fn a_second_failing_check_or_a_new_error_kind_updates_the_issue() {
    let history = [scheduled(
        1,
        json!([failed("exa:search", "network"), passed("exa:fetch")]),
    )];
    let second = scheduled(
        2,
        json!([failed("exa:search", "network"), failed("exa:fetch", "auth")]),
    );
    let new_kind = scheduled(
        2,
        json!([failed("exa:search", "contract"), passed("exa:fetch")]),
    );

    assert_eq!(
        (actions(&history, &second), actions(&history, &new_kind)),
        (one("exa", "update"), one("exa", "update"))
    );
}

#[test]
fn a_partial_recovery_updates_and_a_full_recovery_closes_the_issue() {
    let history = [scheduled(
        1,
        json!([failed("exa:search", "network"), failed("exa:fetch", "auth")]),
    )];
    let partial = scheduled(
        2,
        json!([passed("exa:search"), failed("exa:fetch", "auth")]),
    );
    let full = scheduled(2, json!([passed("exa:search"), passed("exa:fetch")]));

    assert_eq!(
        (actions(&history, &partial), actions(&history, &full)),
        (one("exa", "update"), one("exa", "close"))
    );
}

#[test]
fn an_unconfigured_unselected_or_not_started_check_changes_nothing() {
    let history = [scheduled(
        1,
        json!([failed("exa:search", "network"), passed("exa:fetch")]),
    )];
    // `exa:fetch` is unselected: the record has no entry for it.
    let current = scheduled(
        2,
        json!([
            {"id": "exa:search", "status": "unconfigured", "attempts": 0},
            {"id": "jina:fetch", "status": "not_started", "cause": "deadline", "attempts": 0},
        ]),
    );

    assert_eq!(actions(&history, &current), NONE);
}

#[test]
fn a_failure_left_unconfirmed_neither_opens_nor_closes_an_issue_nor_serves_as_a_baseline() {
    let unconfirmed = json!({
        "id": "exa:search", "status": "failed", "attempts": 1, "confirmed": false,
        "error_kind": "timeout",
    });
    let after_a_pass = actions(
        &[scheduled(1, json!([passed("exa:search")]))],
        &scheduled(2, json!([unconfirmed])),
    );
    let after_a_failure = actions(
        &[scheduled(1, json!([failed("exa:search", "network")]))],
        &scheduled(2, json!([unconfirmed])),
    );
    // A baseline left unconfirmed is skipped, so the confirmed failure before it still counts.
    let past_one = actions(
        &[
            scheduled(1, json!([failed("exa:search", "network")])),
            scheduled(2, json!([unconfirmed])),
        ],
        &scheduled(3, json!([failed("exa:search", "network")])),
    );

    assert_eq!(
        (after_a_pass, after_a_failure, past_one),
        (NONE, NONE, NONE)
    );
}

#[test]
fn a_failure_that_passes_its_rerun_is_a_pass() {
    let rerun_passed =
        json!({"id": "exa:search", "status": "passed", "attempts": 2, "rerun": true});
    let without_issue = actions(
        &[scheduled(1, json!([passed("exa:search")]))],
        &scheduled(2, json!([rerun_passed])),
    );
    let with_issue = actions(
        &[scheduled(1, json!([failed("exa:search", "network")]))],
        &scheduled(2, json!([rerun_passed])),
    );

    assert_eq!((without_issue, with_issue), (NONE, one("exa", "close")));
}

#[test]
fn an_authentication_failure_confirmed_without_a_rerun_opens_the_issue() {
    let history = [scheduled(1, json!([passed("serpapi:search")]))];
    let current = scheduled(
        2,
        json!([{
            "id": "serpapi:search", "status": "failed", "attempts": 1, "confirmed": true,
            "error_kind": "auth", "http_status": 401,
        }]),
    );

    assert_eq!(actions(&history, &current), one("serpapi", "open"));
}

#[test]
fn a_run_that_was_not_scheduled_neither_decides_nor_serves_as_a_baseline() {
    let by_hand = actions(
        &[scheduled(1, json!([passed("exa:search")]))],
        &run(2, false, json!([failed("exa:search", "network")])),
    );
    // The failure by hand on day 2 is no baseline, so day 3's failure is new.
    let after_by_hand = actions(
        &[
            scheduled(1, json!([passed("exa:search")])),
            run(2, false, json!([failed("exa:search", "network")])),
        ],
        &scheduled(3, json!([failed("exa:search", "network")])),
    );

    assert_eq!((by_hand, after_by_hand), (NONE, one("exa", "open")));
}

#[test]
fn without_an_earlier_record_a_failure_opens_and_a_pass_closes_nothing() {
    let failure = actions(&[], &scheduled(1, json!([failed("exa:search", "network")])));
    let pass = actions(&[], &scheduled(1, json!([passed("exa:search")])));

    assert_eq!((failure, pass), (one("exa", "open"), NONE));
}

#[test]
fn a_run_of_another_tier_leaves_the_checks_it_did_not_run_failing() {
    // The metered run failed the keyed fetch; the daily free run passes only the anonymous one.
    let history = [scheduled(
        1,
        json!([failed("jina:fetch", "auth"), passed("jina:fetch:anonymous")]),
    )];
    let free = run_of("free", 2, true, json!([passed("jina:fetch:anonymous")]));

    assert_eq!(actions(&history, &free), NONE);
}

#[test]
fn repeated_identical_input_yields_no_action() {
    // Deciding again over records that already hold the run, as after the run was recorded.
    let current = || {
        scheduled(
            2,
            json!([failed("exa:search", "network"), passed("jina:fetch")]),
        )
    };
    let history = [scheduled(1, json!([passed("exa:search")])), current()];

    assert_eq!(actions(&history, &current()), NONE);
}

#[test]
fn each_provider_is_decided_on_its_own_checks() {
    let history = [scheduled(
        1,
        json!([failed("exa:search", "network"), passed("jina:fetch")]),
    )];
    let current = scheduled(
        2,
        json!([passed("exa:search"), failed("jina:fetch", "timeout")]),
    );

    assert_eq!(
        actions(&history, &current),
        [one("exa", "close"), one("jina", "open")].concat()
    );
}
