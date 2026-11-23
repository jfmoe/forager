mod support;

use std::collections::BTreeMap;

use serde_json::json;

use support::hn::{path, payload, query, stderr};
use support::polymarket::{history_config, json as body, run};
use support::polymarket_samples::{resolved, stored, tokens};
use support::{Fixture, Response, RunEnvironment};

const WINNER: &str = "will-donald-trump-win-the-2024-us-presidential-election";
const OPEN: &str = "will-no-fed-rate-cuts-happen-in-2026";

fn params<const N: usize>(pairs: [(&str, &str); N]) -> BTreeMap<String, String> {
    pairs
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect()
}

#[test]
fn a_resolved_market_lists_its_daily_prices_and_gives_its_payout_as_the_settlement() {
    let gamma = Fixture::start_sequence(vec![body(&resolved(WINNER, "Donald Trump", true))]);
    let data = Fixture::start_sequence(vec![Response::json(200, &stored("price-history.json"))]);
    let environment = RunEnvironment::new(&history_config(&gamma.url, &data.url));

    let output = run(
        &environment,
        &[
            "history",
            &format!("polymarket:market/{WINNER}"),
            "--interval",
            "max",
        ],
    );
    let gamma_request = gamma.finish();
    let data_request = data.finish();
    let [yes, _] = tokens(WINNER);

    assert_eq!(
        (
            output.status.code(),
            path(&gamma_request),
            path(&data_request),
            query(&data_request),
        ),
        (
            Some(0),
            format!("/markets/slug/{WINNER}"),
            "/v2/prices-history".to_owned(),
            params([
                ("token_id", yes.as_str()),
                ("interval", "max"),
                ("bucket_seconds", "86400"),
            ]),
        ),
        "stderr: {}",
        stderr(&output)
    );
    assert_eq!(
        payload(&output),
        json!({
            "platform": "polymarket",
            "provider": "polymarket_api",
            "market": {
                "ref": format!("polymarket:market/{WINNER}"),
                "url": format!("https://polymarket.com/market/{WINNER}"),
                "question": "Will Donald Trump happen?",
                "status": "resolved"
            },
            "outcome": "Yes",
            "interval": "max",
            "bucket_seconds": 86_400,
            "points": [
                {"t": "2024-11-05T00:00:00Z", "price": 0.5855},
                {"t": "2024-11-06T00:00:00Z", "price": 0.58}
            ],
            "settlement": {"t": "2024-11-07T00:00:00Z", "price": 1.0}
        })
    );
}

#[test]
fn an_empty_order_is_a_configuration_error_before_any_request() {
    let gamma = Fixture::start_sequence(Vec::new());
    let data = Fixture::start_sequence(Vec::new());
    let environment = RunEnvironment::new(&format!(
        "{}\n[platforms.polymarket]\norder = []\n",
        history_config(&gamma.url, &data.url)
    ));

    let output = run(
        &environment,
        &["history", &format!("polymarket:market/{OPEN}")],
    );
    gamma.finish_all();
    data.finish_all();

    assert_eq!(
        (output.status.code(), stderr(&output).trim().to_owned()),
        (
            Some(3),
            "config_error: platforms.polymarket.order has no configured route for polymarket history".to_owned()
        )
    );
}
