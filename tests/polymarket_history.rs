mod support;

use serde_json::json;

use support::hn::{payload, stderr};
use support::path;
use support::polymarket::{history_config, json as body, run};
use support::polymarket_samples::{resolved, stored};
use support::{Fixture, Response, RunEnvironment};

const WINNER: &str = "will-donald-trump-win-the-2024-us-presidential-election";
const OPEN: &str = "will-no-fed-rate-cuts-happen-in-2026";

#[test]
fn a_history_prints_the_price_series_of_the_market() {
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
    let history = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            path(&gamma_request),
            path(&data_request),
            [
                &history["platform"],
                &history["provider"],
                &history["market"]["ref"],
            ],
            history["points"].as_array().map(Vec::len),
        ),
        (
            Some(0),
            format!("/markets/slug/{WINNER}"),
            "/v2/prices-history".to_owned(),
            [
                &json!("polymarket"),
                &json!("polymarket_api"),
                &json!(format!("polymarket:market/{WINNER}")),
            ],
            Some(2),
        ),
        "stderr: {}",
        stderr(&output)
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
