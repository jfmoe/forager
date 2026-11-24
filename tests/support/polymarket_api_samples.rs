//! Gamma, CLOB and Data API bodies derived from the stored samples in
//! `tests/samples/polymarket_api/`, shared by the command contract tests and, through the crate
//! root, the in-crate tests. A variant changes only what its test is about, so every other field
//! stays as the sample has it.

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

/// The stored neg-risk event renamed `slug`, holding `markets`.
pub(crate) fn event(slug: &str, markets: &[Value]) -> Value {
    with(
        sample_json("polymarket_api/event.json"),
        &json!({"slug": slug, "markets": markets}),
    )
}

/// The stored search page holding `events`; `has_more` says whether another page follows.
pub(crate) fn search_page(events: &[Value], has_more: bool) -> Value {
    let mut page = with(
        sample_json("polymarket_api/search.json"),
        &json!({"events": events}),
    );
    page["pagination"]["hasMore"] = json!(has_more);
    page
}

/// The stored search page as Gamma answers a search nothing matches: without `events`.
pub(crate) fn no_match() -> Value {
    without(sample_json("polymarket_api/search.json"), &["events"])
}

/// A CLOB prices answer in the stored answer's shape: each token's best bid and best ask.
pub(crate) fn prices(quotes: &[(&str, &str, &str)]) -> Value {
    let stored = sample_json("polymarket_api/prices.json");
    let sides = stored
        .as_object()
        .and_then(|answer| answer.values().next())
        .expect("a stored quote");
    Value::Object(
        quotes
            .iter()
            .map(|(token, bid, ask)| {
                let quote = with(sides.clone(), &json!({"BUY": bid, "SELL": ask}));
                ((*token).to_owned(), quote)
            })
            .collect(),
    )
}

/// A CLOB last trades answer in the stored answer's shape: each token's last trade price.
pub(crate) fn last_trades(trades: &[(&str, &str)]) -> Value {
    let stored = sample_json("polymarket_api/last-trades.json");
    let trade = &stored[0];
    Value::Array(
        trades
            .iter()
            .map(|(token, price)| with(trade.clone(), &json!({"token_id": token, "price": price})))
            .collect(),
    )
}

/// The stored Data API price history page holding `(timestamp, price, resolution_seconds)`
/// points; `next_cursor` marks that another page follows.
pub(crate) fn price_page(points: &[(i64, f64, i64)], next_cursor: Option<&str>) -> Value {
    let stored = sample_json("polymarket_api/price-history.json");
    let point = &stored["data"][0];
    let data = points
        .iter()
        .map(|(timestamp, price, resolution)| {
            with(
                point.clone(),
                &json!({"timestamp": timestamp, "price": price, "resolution_seconds": resolution}),
            )
        })
        .collect::<Vec<_>>();
    let mut page = with(stored, &json!({"data": data}));
    page["pagination"]["has_more"] = json!(next_cursor.is_some());
    page["pagination"]["next_cursor"] = json!(next_cursor);
    page
}
