use serde_json::json;

use crate::support::path;
use crate::support::polymarket::{config, run};
use crate::support::polymarket_samples::stored;
use crate::support::{Fixture, Response, RunEnvironment, payload, stderr};

#[test]
fn a_search_prints_the_matching_events_as_platform_items() {
    let gamma = Fixture::start_sequence(vec![Response::json(200, &stored("search.json"))]);
    let environment = RunEnvironment::new(&config(&gamma.url, "http://127.0.0.1:9"));

    let output = run(&environment, &["search", "fed rate"]);
    let request = gamma.finish();
    let items = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            path(&request),
            [
                &items["platform"],
                &items["provider"],
                &items["items"][0]["ref"]
            ],
        ),
        (
            Some(0),
            "/public-search".to_owned(),
            [
                &json!("polymarket"),
                &json!("polymarket_api"),
                &json!("polymarket:how-many-fed-rate-cuts-in-2026"),
            ],
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn an_empty_order_is_a_configuration_error_before_any_request() {
    let gamma = Fixture::start_sequence(Vec::new());
    let environment = RunEnvironment::new(&format!(
        "{}\n[platforms.polymarket]\norder = []\n",
        config(&gamma.url, "http://127.0.0.1:9")
    ));

    let output = run(&environment, &["search", "fed rate"]);
    gamma.finish_all();

    assert_eq!(
        (output.status.code(), stderr(&output).trim().to_owned()),
        (
            Some(3),
            "config_error: platforms.polymarket.order has no configured route for polymarket search".to_owned()
        )
    );
}
