use serde_json::json;

use crate::support::path;
use crate::support::polymarket::{history_config, run};
use crate::support::polymarket_samples::{resolved, stored};
use crate::support::{Fixture, Response, RunEnvironment, ok_json, payload, stderr};

const WINNER: &str = "will-donald-trump-win-the-2024-us-presidential-election";

#[test]
fn a_history_prints_the_price_series_of_the_market() {
    let gamma = Fixture::start_sequence(vec![ok_json(&resolved(WINNER, "Donald Trump", true))]);
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
