mod support;

use std::collections::BTreeMap;

use serde_json::{Value, json};

use support::hn::{path, payload, query, stderr};
use support::polymarket::{config, json as body, run};
use support::polymarket_samples::{event, market, resolved, search_page, with};
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
fn results_are_events_with_their_most_probable_markets_priced_from_gamma() {
    let gamma = Fixture::start_sequence(vec![page(&[fed_event()], false)]);
    let environment = RunEnvironment::new(&config(&gamma.url, "http://127.0.0.1:9"));

    let output = run(&environment, &["search", "fed rate"]);
    let request = gamma.finish();

    assert_eq!(
        (output.status.code(), path(&request), query(&request)),
        (
            Some(0),
            "/public-search".to_owned(),
            parameters(&[
                ("q", "fed rate"),
                ("limit_per_type", "10"),
                ("page", "1"),
                ("search_tags", "false"),
                ("search_profiles", "false"),
                ("events_status", "active"),
            ])
        ),
        "stderr: {}",
        stderr(&output)
    );
    let market_summary =
        |slug: &str, label: &str, probability: Value, basis: &str, status: &str| {
            json!({
                "ref": format!("polymarket:market/{slug}"),
                "label": label,
                "probability": probability,
                "basis": basis,
                "status": status
            })
        };
    assert_eq!(
        payload(&output),
        json!({
            "platform": "polymarket",
            "provider": "polymarket_api",
            "items": [{
                "ref": "polymarket:how-many-fed-rate-cuts-in-2026",
                "url": "https://polymarket.com/event/how-many-fed-rate-cuts-in-2026",
                "depth": "metadata",
                "title": "How many Fed rate cuts in 2026?",
                "authors": [],
                "published": "2025-09-29T22:24:45Z",
                "event_id": "51456",
                "status": "open",
                "end_date": "2027-01-01T04:59:00Z",
                "volume": 54_744_246.257_398_99,
                "volume_24h": 55_418.871_714,
                "liquidity": 3_993_492.103_83,
                "neg_risk": true,
                "market_count": 6,
                "price_source": "gamma",
                "top_markets": [
                    market_summary(
                        "will-no-fed-rate-cuts-happen-in-2026",
                        "Will 0 (0 bps) happen?",
                        json!(0.962),
                        "midpoint_cached",
                        "open"
                    ),
                    market_summary(
                        "will-1-fed-rate-cut-happen-in-2026",
                        "1 (25 bps)",
                        json!(0.0135),
                        "midpoint_cached",
                        "open"
                    ),
                    market_summary(
                        "will-the-fed-cut-in-january",
                        "January cut",
                        json!(0.0),
                        "resolution",
                        "resolved"
                    ),
                    market_summary(
                        "will-9-fed-rate-cuts-happen-in-2026",
                        "9 (225 bps)",
                        Value::Null,
                        "no_two_sided_book",
                        "open"
                    ),
                    market_summary(
                        "will-8-fed-rate-cuts-happen-in-2026",
                        "8 (200 bps)",
                        Value::Null,
                        "no_two_sided_book",
                        "open"
                    ),
                ]
            }],
            "next_cursor": null
        })
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
