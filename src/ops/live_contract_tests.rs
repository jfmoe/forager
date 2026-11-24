//! The contract rules against forager's own output, so each payload is written here in the shape
//! the command prints.

use serde_json::{Value, json};

use super::violation;
use crate::catalog::LiveContract;

const WINNER: &str = "polymarket:winner";

fn fetched(anonymous: &[bool]) -> Value {
    json!({
        "content": "Page body",
        "provider_attempts": anonymous
            .iter()
            .map(|anonymous| json!({"provider": "jina", "anonymous": anonymous, "error_kind": null}))
            .collect::<Vec<_>>(),
    })
}

#[test]
fn a_keyed_fetch_fails_when_any_attempt_was_anonymous_and_an_anonymous_one_when_any_sent_a_key() {
    let cases = [
        (false, &[false][..]),
        (false, &[false, true]),
        (true, &[true]),
        (true, &[false, true]),
    ];

    assert_eq!(
        cases.map(|(anonymous, attempts)| violation(
            LiveContract::Fetch { anonymous },
            &fetched(attempts)
        )
        .is_none()),
        [true, false, true, false]
    );
}

/// A resolved event whose winning market is `winner`, read in `attempts` provider attempts.
fn resolved_event(winner: &Value, attempts: usize) -> Value {
    json!({
        "ref": "polymarket:event",
        "markets": [
            {
                "ref": "polymarket:loser",
                "status": "resolved",
                "outcomes": [{"name": "Yes", "price": 0.0, "basis": "resolution"}],
            },
            winner,
        ],
        "provider_attempts": vec![json!({"provider": "polymarket_api", "error_kind": null}); attempts],
    })
}

fn winner(status: &str, name: &str, price: f64, basis: &str) -> Value {
    json!({
        "ref": WINNER,
        "status": status,
        "outcomes": [
            {"name": name, "price": price, "basis": basis},
            {"name": "No", "price": 0.0, "basis": basis},
        ],
    })
}

#[test]
fn a_resolved_event_needs_one_attempt_and_its_winner_resolved_yes_at_one_by_resolution() {
    let contract = LiveContract::PolymarketResolved { winner: WINNER };
    let cases = [
        resolved_event(&winner("resolved", "Yes", 1.0, "resolution"), 1),
        resolved_event(&winner("resolved", "Yes", 1.0, "resolution"), 2),
        resolved_event(&winner("open", "Yes", 1.0, "resolution"), 1),
        resolved_event(&winner("resolved", "No", 1.0, "resolution"), 1),
        resolved_event(&winner("resolved", "Yes", 0.0, "resolution"), 1),
        resolved_event(&winner("resolved", "Yes", 1.0, "last_trade"), 1),
    ];

    assert_eq!(
        cases
            .iter()
            .map(|payload| violation(contract, payload).is_none())
            .collect::<Vec<_>>(),
        [true, false, false, false, false, false]
    );
}

fn live_event(price_source: &str, statuses: &[&str]) -> Value {
    json!({
        "ref": "polymarket:event",
        "price_source": price_source,
        "prices_as_of": "2026-11-24T00:00:00Z",
        "markets": statuses
            .iter()
            .map(|status| json!({
                "status": status,
                "outcomes": [{"name": "Yes", "price": 0.4, "basis": "midpoint"}],
            }))
            .collect::<Vec<_>>(),
    })
}

#[test]
fn a_live_event_needs_clob_prices_unless_none_of_its_markets_is_open() {
    let cases = [
        live_event("clob", &["open", "resolved"]),
        live_event("gamma", &["open", "resolved"]),
        live_event("gamma", &["resolved", "closed"]),
    ];

    assert_eq!(
        cases
            .iter()
            .map(|payload| violation(LiveContract::PolymarketLive, payload).is_none())
            .collect::<Vec<_>>(),
        [true, false, true]
    );
}

/// A note with `count` comments, the first with `first_replies` replies.
fn note_comments(count: usize, first_replies: usize) -> Value {
    let comments = (0..count)
        .map(|index| {
            let replies = if index == 0 { first_replies } else { 1 };
            json!({"text": format!("comment {index}"), "replies": vec![json!({"text": "reply"}); replies]})
        })
        .collect::<Vec<_>>();
    json!({"note": "xiaohongshu:note", "comments": comments})
}

#[test]
fn xiaohongshu_comments_need_more_than_ten_comments_and_an_expanded_reply_thread() {
    let cases = [
        note_comments(11, 2),
        note_comments(10, 2),
        note_comments(11, 1),
    ];

    assert_eq!(
        cases
            .iter()
            .map(|payload| violation(LiveContract::XiaohongshuComments, payload))
            .collect::<Vec<_>>(),
        [
            None,
            Some("the note has 10 or fewer comments, so the second comment page was not read"),
            Some("no returned comment had more replies to expand"),
        ]
    );
}

#[test]
fn a_runtime_class_attempt_fails_a_contract_the_output_otherwise_meets() {
    let mut answered = json!({
        "answer": "Rust 1.91 is the latest stable release.",
        "provider_attempts": [{"provider": "xai", "error_kind": null}],
    });
    let holds = violation(LiveContract::Answer, &answered);
    answered["provider_attempts"]
        .as_array_mut()
        .expect("attempts")
        .insert(0, json!({"provider": "openai", "error_kind": "runtime"}));

    assert_eq!(
        (holds, violation(LiveContract::Answer, &answered)),
        (
            None,
            Some("the output includes a runtime-class provider attempt")
        )
    );
}
