//! Polymarket fixtures for the command contract tests: configuration for the endpoints of
//! `polymarket_api`, and its upstream answers, derived from the stored samples by
//! [`super::polymarket_samples`].

use std::process::Output;

use serde_json::Value;

use super::polymarket_samples;
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

pub(crate) fn json(body: &Value) -> Response {
    Response::json(200, &body.to_string())
}

/// Runs `forager platform polymarket <arguments>`.
pub(crate) fn run(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "polymarket"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

/// A CLOB `POST /prices` answer: each token's best bid and best ask.
pub(crate) fn prices(quotes: &[(&str, &str, &str)]) -> Response {
    json(&polymarket_samples::prices(quotes))
}

/// A CLOB `POST /last-trades-prices` answer.
pub(crate) fn last_trades(trades: &[(&str, &str)]) -> Response {
    json(&polymarket_samples::last_trades(trades))
}

/// A Data API `GET /v2/prices-history` page of `(timestamp, price, resolution_seconds)` points;
/// `next_cursor` marks that another page follows.
pub(crate) fn price_page(points: &[(i64, f64, i64)], next_cursor: Option<&str>) -> Response {
    json(&polymarket_samples::price_page(points, next_cursor))
}
