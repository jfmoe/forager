mod support;

use std::collections::BTreeMap;

use serde_json::{Value, json};

use support::hn::{path, payload, query, stderr};
use support::polymarket::{config, json as body, run};
use support::polymarket_samples::{event, market, resolved, search_page, stored, with};
use support::{Fixture, Response, RunEnvironment};

/// A search page as Gamma returns it: the events, and whether more pages follow.
fn page(events: &[Value], has_more: bool) -> Response {
    body(&search_page(events, has_more))
}

fn parameters(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

/// Six markets of one event: two-sided books, an empty book whose cached price is the 0.5
/// placeholder, a one-sided book, a resolved market, and a closed one.
fn fed_event() -> Value {
    event(
        "how-many-fed-rate-cuts-in-2026",
        &[
            with(
                market("will-1-fed-rate-cut-happen-in-2026", "1 (25 bps)"),
                &json!({"outcomePrices": "[\"0.0135\", \"0.9865\"]", "bestBid": 0.013, "bestAsk": 0.014}),
            ),
            with(
                market("will-no-fed-rate-cuts-happen-in-2026", "0 (0 bps)"),
                &json!({"groupItemTitle": ""}),
            ),
            with(
                market("will-9-fed-rate-cuts-happen-in-2026", "9 (225 bps)"),
                &json!({"outcomePrices": "[\"0.5\", \"0.5\"]", "bestBid": null, "bestAsk": 1}),
            ),
            with(
                market("will-8-fed-rate-cuts-happen-in-2026", "8 (200 bps)"),
                &json!({"bestBid": 0.2, "bestAsk": null}),
            ),
            resolved("will-the-fed-cut-in-january", "January cut", false),
            with(
                market("will-2-fed-rate-cuts-happen-in-2026", "2 (50 bps)"),
                &json!({"closed": true, "acceptingOrders": false}),
            ),
        ],
    )
}

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
fn include_closed_drops_the_status_filter_and_its_cursor_resumes_the_next_page() {
    let gamma = Fixture::start_sequence(vec![
        page(&[fed_event()], true),
        page(&[fed_event()], false),
    ]);
    let environment = RunEnvironment::new(&config(&gamma.url, "http://127.0.0.1:9"));

    let first = run(
        &environment,
        &["search", "fed rate", "--include-closed", "--limit", "3"],
    );
    let next_cursor = payload(&first)["next_cursor"]
        .as_str()
        .expect("a page with more results issues a cursor")
        .to_owned();
    let second = run(&environment, &["search", "--cursor", &next_cursor]);
    let requests = gamma.finish_all();

    assert_eq!(
        (
            first.status.code(),
            second.status.code(),
            payload(&second)["next_cursor"].clone(),
            requests
                .iter()
                .map(|request| query(request))
                .collect::<Vec<_>>(),
        ),
        (
            Some(0),
            Some(0),
            Value::Null,
            ["1", "2"]
                .map(|page| parameters(&[
                    ("q", "fed rate"),
                    ("limit_per_type", "3"),
                    ("page", page),
                    ("search_tags", "false"),
                    ("search_profiles", "false"),
                ]))
                .to_vec(),
        ),
        "stderr: {}",
        stderr(&second)
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
