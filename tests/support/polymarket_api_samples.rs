//! Gamma market bodies derived from the stored samples in `tests/samples/polymarket_api/`,
//! shared by the command contract tests and, through the crate root, the in-crate tests. A
//! variant changes only what its test is about, so every other field stays as the sample has it.

use serde_json::{Value, json};

use super::samples::{sample_json, with, without};

/// The two outcome tokens of a derived market, named after its slug.
pub(crate) fn tokens(slug: &str) -> [String; 2] {
    [format!("{slug}-yes"), format!("{slug}-no")]
}

/// The stored open Yes/No market, with Gamma's cached book of 0.961/0.963, as an event lists
/// it: renamed `slug`, asking whether `label` happens, labelled `label`, and with the tokens of
/// [`tokens`].
pub(crate) fn market(slug: &str, label: &str) -> Value {
    let [yes, no] = tokens(slug);
    let market = with(
        sample_json("polymarket_api/market.json"),
        &json!({
            "slug": slug,
            "question": format!("Will {label} happen?"),
            "groupItemTitle": label,
            "conditionId": format!("0x{slug}"),
            "clobTokenIds": serde_json::to_string(&[yes, no]).expect("encode tokens"),
        }),
    );
    without(market, &["events"])
}

/// [`market`] resolved, its first outcome won or lost, with the leftover book Gamma keeps.
pub(crate) fn resolved(slug: &str, label: &str, won: bool) -> Value {
    with(
        market(slug, label),
        &json!({
            "closed": true,
            "acceptingOrders": false,
            "umaResolutionStatus": "resolved",
            "outcomePrices": if won { "[\"1\", \"0\"]" } else { "[\"0\", \"1\"]" },
            "bestBid": 0.997,
            "bestAsk": 0.998
        }),
    )
}
