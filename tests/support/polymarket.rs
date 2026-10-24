//! Polymarket fixtures: configuration for the endpoints of `polymarket_api`, and Gamma, CLOB,
//! and Data API records in the shapes the APIs returned on 2026-10-24, shared by the `platform
//! polymarket` tests.

use std::process::Output;

use serde_json::{Value, json};

use super::{Response, RunEnvironment};

/// Points `polymarket_api` at the two fixture servers with one attempt per request, so a
/// failure response is terminal at once.
pub(crate) fn config(gamma: &str, clob: &str) -> String {
    format!(
        "[providers.polymarket_api]\nurl = \"{gamma}\"\nclob_url = \"{clob}\"\n\n[retry]\nmax_attempts = 1\n"
    )
}

/// Points `polymarket_api` at a Gamma and a Data API fixture with one attempt per request; the
/// CLOB keeps its default, which history never reaches.
pub(crate) fn history_config(gamma: &str, data: &str) -> String {
    format!(
        "[providers.polymarket_api]\nurl = \"{gamma}\"\ndata_url = \"{data}\"\n\n[retry]\nmax_attempts = 1\n"
    )
}

/// A Data API `GET /v2/prices-history` page of `(timestamp, price, resolution_seconds)` points;
/// `next_cursor` marks that another page follows.
pub(crate) fn price_page(points: &[(i64, f64, i64)], next_cursor: Option<&str>) -> Response {
    json(&json!({
        "data": points
            .iter()
            .map(|(timestamp, price, resolution)| {
                json!({"timestamp": timestamp, "price": price, "resolution_seconds": resolution})
            })
            .collect::<Vec<_>>(),
        "pagination": {
            "limit": 10_000,
            "offset": 0,
            "has_more": next_cursor.is_some(),
            "next_cursor": next_cursor
        }
    }))
}

pub(crate) fn json(body: &Value) -> Response {
    Response::json(200, &body.to_string())
}

/// Runs `forager platform polymarket <arguments>`.
pub(crate) fn run(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "polymarket"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

/// The two outcome tokens of a market, named after its slug.
pub(crate) fn tokens(slug: &str) -> [String; 2] {
    [format!("{slug}-yes"), format!("{slug}-no")]
}

/// An open Yes/No market accepting orders, with Gamma's cached book of 0.961/0.963.
pub(crate) fn market(slug: &str, label: &str) -> Value {
    let [yes, no] = tokens(slug);
    json!({
        "id": "616902",
        "question": format!("Will {label} happen?"),
        "conditionId": format!("0x{slug}"),
        "slug": slug,
        "endDate": "2027-01-01T04:59:00Z",
        "startDate": "2025-09-29T22:24:45.988997Z",
        "liquidity": "654503.09367",
        "description": "Event rules.",
        "outcomes": "[\"Yes\", \"No\"]",
        "outcomePrices": "[\"0.962\", \"0.038\"]",
        "volume": "9041795.83",
        "active": true,
        "closed": false,
        "groupItemTitle": label,
        "questionID": "0xquestion",
        "volumeNum": 9_041_795.83,
        "liquidityNum": 654_503.093_67,
        "clobTokenIds": serde_json::to_string(&[yes, no]).expect("encode tokens"),
        "umaResolutionStatuses": "[]",
        "acceptingOrders": true,
        "negRisk": true,
        "bestBid": 0.961,
        "bestAsk": 0.963,
        "spread": 0.002,
        "lastTradePrice": 0.961,
        "oneDayPriceChange": -0.002,
        "oneWeekPriceChange": -0.006,
        "oneMonthPriceChange": 0.01,
        "updatedAt": "2026-10-24T06:34:38.245985Z",
        "version": "v1"
    })
}

/// Sets fields of a market or event record.
pub(crate) fn with(mut record: Value, fields: &Value) -> Value {
    for (key, value) in fields.as_object().expect("fields object") {
        record[key] = value.clone();
    }
    record
}

/// A resolved market whose first outcome won, with the leftover book Gamma keeps.
pub(crate) fn resolved(slug: &str, label: &str, won: bool) -> Value {
    with(
        market(slug, label),
        &json!({
            "closed": true,
            "acceptingOrders": false,
            "umaResolutionStatus": "resolved",
            "outcomePrices": if won { "[\"1\", \"0\"]" } else { "[\"0\", \"1\"]" },
            "bestBid": 0.997,
            "bestAsk": 0.998,
            "lastTradePrice": 1
        }),
    )
}

/// A neg-risk event holding `markets`.
pub(crate) fn event(slug: &str, markets: &[Value]) -> Value {
    json!({
        "id": "51456",
        "ticker": slug,
        "slug": slug,
        "title": "How many Fed rate cuts in 2026?",
        "description": "Event rules.",
        "startDate": "2025-09-29T22:24:45.988997Z",
        "endDate": "2027-01-01T04:59:00Z",
        "active": true,
        "closed": false,
        "liquidity": 3_993_492.103_83,
        "volume": 54_744_246.257_398_99,
        "volume24hr": 55_418.871_714,
        "negRisk": true,
        "updatedAt": "2026-10-24T06:34:54.385458Z",
        "markets": markets,
        "version": "v1"
    })
}

/// A CLOB `POST /prices` answer: each token's best bid (BUY) and best ask (SELL).
pub(crate) fn prices(quotes: &[(&str, &str, &str)]) -> Response {
    let body = quotes
        .iter()
        .map(|(token, bid, ask)| ((*token).to_owned(), json!({"BUY": bid, "SELL": ask})))
        .collect::<serde_json::Map<_, _>>();
    json(&Value::Object(body))
}

/// A CLOB `POST /last-trades-prices` answer.
pub(crate) fn last_trades(trades: &[(&str, &str)]) -> Response {
    json(&Value::Array(
        trades
            .iter()
            .map(|(token, price)| json!({"price": price, "side": "BUY", "token_id": token}))
            .collect(),
    ))
}
