mod support;

use serde_json::{Value, json};

use support::hn::{path, payload, stderr};
use support::polymarket::{
    config, event, json as body, last_trades, market, placeholder, prices, resolved, run, tokens,
    with,
};
use support::{Fixture, Response, RunEnvironment, request_json};

const EVENT: &str = "how-many-fed-rate-cuts-in-2026";

/// Five markets, one per row of the price derivation: a two-sided live book, an empty book
/// whose cached price is the 0.5 placeholder, a one-sided book, a resolved market with a
/// leftover book, a closed market, and a suspended one.
fn derivation_event() -> Value {
    event(
        EVENT,
        &[
            market("two-sided", "Two-sided"),
            with(
                market("empty-book", "Empty book"),
                &json!({"outcomePrices": "[\"0.5\", \"0.5\"]", "bestBid": null, "bestAsk": 1}),
            ),
            market("one-sided", "One-sided"),
            resolved("resolved-yes", "Resolved", true),
            with(
                market("closed-unresolved", "Closed"),
                &json!({"closed": true, "acceptingOrders": false}),
            ),
            with(
                market("suspended", "Suspended"),
                &json!({"acceptingOrders": false}),
            ),
        ],
    )
}

/// The live books of the three open markets.
fn live_books() -> Response {
    let [two_yes, two_no] = tokens("two-sided");
    let [empty_yes, empty_no] = tokens("empty-book");
    let [one_yes, one_no] = tokens("one-sided");
    prices(&[
        (&two_yes, "0.961", "0.963"),
        (&two_no, "0.037", "0.039"),
        (&empty_yes, "0", "1"),
        (&empty_no, "0", "1"),
        (&one_yes, "0.2", "1"),
        (&one_no, "0", "0.8"),
    ])
}

fn object<const N: usize>(fields: [(&str, Value); N]) -> Value {
    Value::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect(),
    )
}

fn outcome(name: &str, price: Value, basis: &str, bid: Value, ask: Value) -> Value {
    object([
        ("name", json!(name)),
        ("price", price),
        ("basis", json!(basis)),
        ("best_bid", bid),
        ("best_ask", ask),
    ])
}

/// The pricing fields of every market of a fetched event, by label.
fn pricing(fetched: &Value) -> Vec<Value> {
    fetched["markets"]
        .as_array()
        .expect("markets")
        .iter()
        .map(|market| {
            json!({
                "label": market["label"],
                "status": market["status"],
                "probability": market["probability"],
                "outcomes": market["outcomes"],
                "spread": market["spread"],
                "last_trade_price": market["last_trade_price"]
            })
        })
        .collect()
}

fn priced(
    label: &str,
    status: &str,
    probability: Value,
    outcomes: [Value; 2],
    spread: Value,
    last_trade: Value,
) -> Value {
    object([
        ("label", json!(label)),
        ("status", json!(status)),
        ("probability", probability),
        ("outcomes", Value::Array(outcomes.into())),
        ("spread", spread),
        ("last_trade_price", last_trade),
    ])
}

fn none() -> Value {
    Value::Null
}

/// The pricing of every market of `derivation_event`, one row of the derivation each.
fn derived_prices() -> Vec<Value> {
    vec![
        priced(
            "Two-sided",
            "open",
            json!(0.962),
            [
                outcome("Yes", json!(0.962), "midpoint", json!(0.961), json!(0.963)),
                outcome("No", json!(0.038), "midpoint", json!(0.037), json!(0.039)),
            ],
            json!(0.002),
            json!(0.962),
        ),
        priced(
            "Empty book",
            "open",
            none(),
            [
                outcome("Yes", none(), "no_two_sided_book", none(), none()),
                outcome("No", none(), "no_two_sided_book", none(), none()),
            ],
            none(),
            none(),
        ),
        priced(
            "One-sided",
            "open",
            none(),
            [
                outcome("Yes", none(), "no_two_sided_book", json!(0.2), none()),
                outcome("No", none(), "no_two_sided_book", none(), json!(0.8)),
            ],
            none(),
            none(),
        ),
        priced(
            "Resolved",
            "resolved",
            json!(1.0),
            [
                outcome("Yes", json!(1.0), "resolution", none(), none()),
                outcome("No", json!(0.0), "resolution", none(), none()),
            ],
            none(),
            none(),
        ),
        priced(
            "Closed",
            "closed",
            none(),
            [
                outcome("Yes", none(), "not_trading", none(), none()),
                outcome("No", none(), "not_trading", none(), none()),
            ],
            none(),
            none(),
        ),
        priced(
            "Suspended",
            "suspended",
            none(),
            [
                outcome("Yes", none(), "not_trading", none(), none()),
                outcome("No", none(), "not_trading", none(), none()),
            ],
            none(),
            none(),
        ),
    ]
}

#[test]
fn each_outcome_is_priced_by_its_market_status_and_live_book() {
    let gamma = Fixture::start_sequence(vec![body(&derivation_event())]);
    let [two_yes, _] = tokens("two-sided");
    let clob = Fixture::start_sequence(vec![live_books(), last_trades(&[(&two_yes, "0.962")])]);
    let environment = RunEnvironment::new(&config(&gamma.url, &clob.url));

    let output = run(&environment, &["fetch", &format!("polymarket:{EVENT}")]);
    let gamma_request = gamma.finish();
    let clob_requests = clob.finish_all();
    let fetched = payload(&output);
    let open_tokens = ["two-sided", "empty-book", "one-sided"]
        .iter()
        .flat_map(|slug| tokens(slug))
        .collect::<Vec<_>>();

    assert_eq!(
        (
            output.status.code(),
            path(&gamma_request),
            clob_requests
                .iter()
                .map(|request| path(request))
                .collect::<Vec<_>>(),
            request_json(&clob_requests[0]),
            request_json(&clob_requests[1]),
        ),
        (
            Some(0),
            format!("/events/slug/{EVENT}"),
            vec!["/prices".to_owned(), "/last-trades-prices".to_owned()],
            Value::Array(
                open_tokens
                    .iter()
                    .flat_map(|token| ["BUY", "SELL"]
                        .map(|side| json!({"token_id": token, "side": side})))
                    .collect()
            ),
            Value::Array(
                open_tokens
                    .iter()
                    .map(|token| json!({"token_id": token}))
                    .collect()
            ),
        ),
        "stderr: {}",
        stderr(&output)
    );
    assert_eq!(pricing(&fetched), derived_prices());
}

#[test]
fn an_event_carries_its_identity_scale_and_when_its_prices_were_read() {
    let gamma = Fixture::start_sequence(vec![body(&event(
        EVENT,
        &[market("two-sided", "Two-sided")],
    ))]);
    let [yes, no] = tokens("two-sided");
    let clob = Fixture::start_sequence(vec![
        prices(&[(&yes, "0.961", "0.963"), (&no, "0.037", "0.039")]),
        last_trades(&[]),
    ]);
    let environment = RunEnvironment::new(&config(&gamma.url, &clob.url));

    let output = run(
        &environment,
        &[
            "fetch",
            &format!("https://polymarket.com/zh/event/{EVENT}?tid=1761200000000"),
        ],
    );
    gamma.finish();
    clob.finish_all();
    let mut fetched = payload(&output);
    let prices_as_of = fetched["prices_as_of"].take();
    fetched["markets"][0]["outcomes"].take();

    assert!(
        prices_as_of
            .as_str()
            .is_some_and(|time| time.len() == 20 && time.ends_with('Z')),
        "{prices_as_of}"
    );
    assert_eq!(
        fetched,
        json!({
            "platform": "polymarket",
            "provider": "polymarket_api",
            "ref": format!("polymarket:{EVENT}"),
            "url": format!("https://polymarket.com/event/{EVENT}"),
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
            "market_count": 1,
            "price_source": "clob",
            "prices_as_of": null,
            "markets": [{
                "ref": "polymarket:market/two-sided",
                "url": "https://polymarket.com/market/two-sided",
                "market_id": "616902",
                "condition_id": "0xtwo-sided",
                "question": "Will Two-sided happen?",
                "label": "Two-sided",
                "status": "open",
                "end_date": "2027-01-01T04:59:00Z",
                "probability": 0.962,
                "outcomes": null,
                "spread": 0.002,
                "last_trade_price": null,
                "one_day_change": -0.002,
                "one_week_change": -0.006,
                "one_month_change": 0.01,
                "volume": 9_041_795.83,
                "liquidity": 654_503.093_67
            }]
        })
    );
}

#[test]
fn when_the_clob_fails_open_markets_fall_back_to_gamma_cached_quotes_without_a_last_trade() {
    let gamma = Fixture::start_sequence(vec![body(&derivation_event())]);
    let clob = Fixture::start_sequence(vec![Response::json(500, r#"{"error":"internal"}"#)]);
    let environment = RunEnvironment::new(&config(&gamma.url, &clob.url));

    let output = run(&environment, &["fetch", &format!("polymarket:{EVENT}")]);
    gamma.finish();
    clob.finish_all();
    let fetched = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            fetched["price_source"].clone(),
            fetched["prices_as_of"].clone(),
            pricing(&fetched)[..3].to_vec(),
            stderr(&output).trim().to_owned(),
        ),
        (
            Some(0),
            json!("gamma"),
            json!("2026-10-24T06:34:54Z"),
            vec![
                priced(
                    "Two-sided",
                    "open",
                    json!(0.962),
                    [
                        outcome("Yes", json!(0.962), "midpoint_cached", json!(0.961), json!(0.963)),
                        outcome("No", json!(0.038), "midpoint_cached", json!(0.037), json!(0.039)),
                    ],
                    json!(0.002),
                    none(),
                ),
                priced(
                    "Empty book",
                    "open",
                    none(),
                    [
                        outcome("Yes", none(), "no_two_sided_book", none(), none()),
                        outcome("No", none(), "no_two_sided_book", none(), none()),
                    ],
                    none(),
                    none(),
                ),
                priced(
                    "One-sided",
                    "open",
                    json!(0.962),
                    [
                        outcome("Yes", json!(0.962), "midpoint_cached", json!(0.961), json!(0.963)),
                        outcome("No", json!(0.038), "midpoint_cached", json!(0.037), json!(0.039)),
                    ],
                    json!(0.002),
                    none(),
                ),
            ],
            "Polymarket CLOB API unavailable (Polymarket CLOB API returned HTTP 500: internal); prices are Gamma's cached quotes, which may lag up to 5 minutes".to_owned(),
        )
    );
}

#[test]
fn inactive_placeholder_markets_are_suspended_and_never_reach_the_clob() {
    let gamma = Fixture::start_sequence(vec![body(&event(
        EVENT,
        &[
            market("two-sided", "Two-sided"),
            placeholder("will-person-a-win", "Person A"),
        ],
    ))]);
    let [yes, no] = tokens("two-sided");
    let clob = Fixture::start_sequence(vec![
        prices(&[(&yes, "0.961", "0.963"), (&no, "0.037", "0.039")]),
        last_trades(&[]),
    ]);
    let environment = RunEnvironment::new(&config(&gamma.url, &clob.url));

    let output = run(&environment, &["fetch", &format!("polymarket:{EVENT}")]);
    gamma.finish();
    let clob_requests = clob.finish_all();
    let fetched = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            request_json(&clob_requests[0]),
            pricing(&fetched)[1].clone(),
            stderr(&output).trim().to_owned(),
        ),
        (
            Some(0),
            json!([
                {"token_id": yes, "side": "BUY"},
                {"token_id": yes, "side": "SELL"},
                {"token_id": no, "side": "BUY"},
                {"token_id": no, "side": "SELL"},
            ]),
            priced(
                "Person A",
                "suspended",
                none(),
                [
                    outcome("Yes", none(), "not_trading", none(), none()),
                    outcome("No", none(), "not_trading", none(), none()),
                ],
                none(),
                none(),
            ),
            String::new(),
        )
    );
}

#[test]
fn a_resolved_market_is_read_from_gamma_alone() {
    let resolved_market = with(
        resolved(
            "will-donald-trump-win-the-2024-us-presidential-election",
            "Donald Trump",
            true,
        ),
        &json!({
            "events": [{
                "id": "903193",
                "slug": "presidential-election-winner-2024",
                "title": "Presidential Election Winner 2024"
            }],
            "updatedAt": "2024-11-07T15:23:00.123Z"
        }),
    );
    let gamma = Fixture::start_sequence(vec![body(&resolved_market)]);
    let clob = Fixture::start_sequence(Vec::new());
    let environment = RunEnvironment::new(&config(&gamma.url, &clob.url));

    let output = run(
        &environment,
        &[
            "fetch",
            "https://polymarket.com/event/presidential-election-winner-2024/will-donald-trump-win-the-2024-us-presidential-election",
        ],
    );
    let request = gamma.finish();
    clob.finish_all();
    let fetched = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            path(&request),
            [
                "ref",
                "event_id",
                "market_id",
                "condition_id",
                "status",
                "probability",
                "outcomes",
                "event",
                "price_source",
                "prices_as_of",
                "last_trade_price",
            ]
            .map(|field| fetched[field].clone()),
        ),
        (
            Some(0),
            "/markets/slug/will-donald-trump-win-the-2024-us-presidential-election".to_owned(),
            [
                json!("polymarket:market/will-donald-trump-win-the-2024-us-presidential-election"),
                json!("903193"),
                json!("616902"),
                json!("0xwill-donald-trump-win-the-2024-us-presidential-election"),
                json!("resolved"),
                json!(1.0),
                json!([
                    outcome("Yes", json!(1.0), "resolution", none(), none()),
                    outcome("No", json!(0.0), "resolution", none(), none()),
                ]),
                json!({
                    "ref": "polymarket:presidential-election-winner-2024",
                    "url": "https://polymarket.com/event/presidential-election-winner-2024",
                    "title": "Presidential Election Winner 2024"
                }),
                json!("gamma"),
                json!("2024-11-07T15:23:00Z"),
                none(),
            ]
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn outcomes_the_clob_leaves_out_have_no_price_and_failed_last_trades_are_null() {
    let gamma = Fixture::start_sequence(vec![body(&event(
        EVENT,
        &[market("two-sided", "Two-sided")],
    ))]);
    let [yes, _] = tokens("two-sided");
    let clob = Fixture::start_sequence(vec![
        prices(&[(&yes, "0.961", "0.963")]),
        Response::json(503, r#"{"error":"busy"}"#),
    ]);
    let environment = RunEnvironment::new(&config(&gamma.url, &clob.url));

    let output = run(&environment, &["fetch", &format!("polymarket:{EVENT}")]);
    gamma.finish();
    clob.finish_all();
    let fetched = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            fetched["price_source"].clone(),
            pricing(&fetched),
            stderr(&output).trim().to_owned(),
        ),
        (
            Some(0),
            json!("clob"),
            vec![priced(
                "Two-sided",
                "open",
                json!(0.962),
                [
                    outcome("Yes", json!(0.962), "midpoint", json!(0.961), json!(0.963)),
                    outcome("No", none(), "no_two_sided_book", none(), none()),
                ],
                json!(0.002),
                none(),
            )],
            "Polymarket CLOB API last trades unavailable (Polymarket CLOB API returned HTTP 503: busy); last_trade_price is null\nPolymarket CLOB API returned no quote for 1 outcome; it has no price".to_owned(),
        )
    );
}

#[test]
fn quotes_for_more_than_250_tokens_are_read_in_batches_of_500_entries() {
    let markets = (0..126)
        .map(|index| market(&format!("market-{index}"), &format!("Market {index}")))
        .collect::<Vec<_>>();
    let gamma = Fixture::start_sequence(vec![body(&event(EVENT, &markets))]);
    let clob = Fixture::start_sequence(vec![prices(&[]), prices(&[]), last_trades(&[])]);
    let environment = RunEnvironment::new(&config(&gamma.url, &clob.url));

    let output = run(&environment, &["fetch", &format!("polymarket:{EVENT}")]);
    gamma.finish();
    let requests = clob.finish_all();

    assert_eq!(
        (
            output.status.code(),
            requests
                .iter()
                .map(|request| (
                    path(request),
                    request_json(request).as_array().map(Vec::len)
                ))
                .collect::<Vec<_>>(),
        ),
        (
            Some(0),
            vec![
                ("/prices".to_owned(), Some(500)),
                ("/prices".to_owned(), Some(4)),
                ("/last-trades-prices".to_owned(), Some(252)),
            ]
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_missing_slug_asks_the_caller_to_search_again() {
    let gamma = Fixture::start_sequence(vec![
        Response::json(
            404,
            r#"{"type":"not found error","error":"slug not found"}"#,
        ),
        Response::json(
            404,
            r#"{"type":"not found error","error":"slug not found"}"#,
        ),
    ]);
    let clob = Fixture::start_sequence(Vec::new());
    let environment = RunEnvironment::new(&config(&gamma.url, &clob.url));

    let failures = ["polymarket:gone-event", "polymarket:market/gone-market"].map(|reference| {
        let failure = payload(&run(&environment, &["fetch", reference]));
        (failure["error_kind"].clone(), failure["message"].clone())
    });
    gamma.finish_all();
    clob.finish_all();

    assert_eq!(
        failures,
        [
            (
                json!("parameter"),
                json!(
                    "Polymarket event not found: polymarket:gone-event; slugs can change, search again"
                )
            ),
            (
                json!("parameter"),
                json!(
                    "Polymarket market not found: polymarket:market/gone-market; slugs can change, search again"
                )
            ),
        ]
    );
}

#[test]
fn another_slug_or_a_broken_encoded_field_fails_as_runtime() {
    let broken = with(
        market("two-sided", "Two-sided"),
        &json!({"outcomes": "[\"Yes\", "}),
    );
    let short_of_tokens = with(
        market("two-sided", "Two-sided"),
        &json!({"clobTokenIds": "[\"one\"]"}),
    );
    let resolved_short_of_tokens = with(
        resolved("two-sided", "Two-sided", true),
        &json!({"clobTokenIds": "[\"one\"]"}),
    );
    let gamma = Fixture::start_sequence(vec![
        body(&event("another-event", &[market("two-sided", "Two-sided")])),
        body(&event(EVENT, &[broken])),
        body(&event(EVENT, &[short_of_tokens])),
        body(&event(EVENT, &[resolved_short_of_tokens])),
    ]);
    let clob = Fixture::start_sequence(Vec::new());
    let environment = RunEnvironment::new(&config(&gamma.url, &clob.url));

    let outputs = [(); 4].map(|()| run(&environment, &["fetch", &format!("polymarket:{EVENT}")]));
    gamma.finish_all();
    clob.finish_all();

    assert_eq!(
        outputs.map(|output| {
            let failure = payload(&output);
            (
                output.status.code(),
                failure["error_kind"].clone(),
                failure["message"].clone(),
            )
        }),
        [
            (
                Some(4),
                json!("runtime"),
                json!(format!(
                    "Polymarket Gamma API answered polymarket:{EVENT} with `another-event`"
                ))
            ),
            (
                Some(4),
                json!("runtime"),
                json!(
                    "Polymarket Gamma API returned polymarket:market/two-sided with an undecodable `outcomes`: EOF while parsing a value at line 1 column 8"
                )
            ),
            (
                Some(4),
                json!("runtime"),
                json!(
                    "Polymarket Gamma API returned polymarket:market/two-sided with 2 outcomes but 1 tokens"
                )
            ),
            (
                Some(4),
                json!("runtime"),
                json!(
                    "Polymarket Gamma API returned polymarket:market/two-sided with 2 outcomes but 1 tokens"
                )
            ),
        ]
    );
}

/// Gamma lists some long-settled markets without tokens, and the price fields may be numbers
/// as well as decimal strings.
#[test]
fn a_settled_market_without_tokens_reads_its_payouts_written_as_numbers() {
    let mut settled = with(
        resolved("will-the-chiefs-win-super-bowl-55", "Chiefs", false),
        &json!({"outcomePrices": "[0, 1]"}),
    );
    settled
        .as_object_mut()
        .expect("market object")
        .remove("clobTokenIds");
    let gamma = Fixture::start_sequence(vec![body(&settled)]);
    let clob = Fixture::start_sequence(Vec::new());
    let environment = RunEnvironment::new(&config(&gamma.url, &clob.url));

    let output = run(
        &environment,
        &[
            "fetch",
            "polymarket:market/will-the-chiefs-win-super-bowl-55",
        ],
    );
    gamma.finish();
    clob.finish_all();
    let fetched = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            fetched["status"].clone(),
            fetched["outcomes"].clone()
        ),
        (
            Some(0),
            json!("resolved"),
            json!([
                outcome("Yes", json!(0.0), "resolution", none(), none()),
                outcome("No", json!(1.0), "resolution", none(), none()),
            ])
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn abstract_inlines_the_event_rules_and_only_market_rules_that_differ() {
    let fetched_event = event(
        EVENT,
        &[
            with(
                market("same-rules", "Same"),
                &json!({"description": "Event rules."}),
            ),
            with(
                market("own-rules", "Own"),
                &json!({"description": "Own rules."}),
            ),
        ],
    );
    let gamma = Fixture::start_sequence(vec![body(&fetched_event), body(&fetched_event)]);
    let quotes = || {
        let [same_yes, same_no] = tokens("same-rules");
        let [own_yes, own_no] = tokens("own-rules");
        prices(&[
            (&same_yes, "0.4", "0.5"),
            (&same_no, "0.5", "0.6"),
            (&own_yes, "0.4", "0.5"),
            (&own_no, "0.5", "0.6"),
        ])
    };
    let clob =
        Fixture::start_sequence(vec![quotes(), last_trades(&[]), quotes(), last_trades(&[])]);
    let environment = RunEnvironment::new(&config(&gamma.url, &clob.url));

    let json_output = run(
        &environment,
        &[
            "fetch",
            &format!("polymarket:{EVENT}"),
            "--depth",
            "abstract",
        ],
    );
    let content_output = run(
        &environment,
        &[
            "fetch",
            &format!("polymarket:{EVENT}"),
            "--depth",
            "abstract",
            "--format",
            "content",
        ],
    );
    gamma.finish_all();
    clob.finish_all();
    let fetched = payload(&json_output);

    assert_eq!(
        (
            fetched["depth"].clone(),
            fetched["rules"].clone(),
            fetched["markets"]
                .as_array()
                .expect("markets")
                .iter()
                .map(|market| market.get("rules").cloned())
                .collect::<Vec<_>>(),
            content_output.status.code(),
            String::from_utf8_lossy(&content_output.stdout)
                .trim()
                .to_owned(),
        ),
        (
            json!("abstract"),
            json!("Event rules."),
            vec![None, Some(json!("Own rules."))],
            Some(0),
            "Event rules.\n\n## Own\n\nOwn rules.".to_owned(),
        )
    );
}

#[test]
fn abstract_without_rules_fails_as_quality() {
    let gamma = Fixture::start_sequence(vec![body(&with(
        market("two-sided", "Two-sided"),
        &json!({"description": "  "}),
    ))]);
    let clob = Fixture::start_sequence(Vec::new());
    let environment = RunEnvironment::new(&config(&gamma.url, &clob.url));

    let output = run(
        &environment,
        &[
            "fetch",
            "polymarket:market/two-sided",
            "--depth",
            "abstract",
        ],
    );
    gamma.finish();
    clob.finish_all();
    let failure = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            failure["error_kind"].clone(),
            failure["message"].clone()
        ),
        (
            Some(5),
            json!("quality"),
            json!("polymarket:market/two-sided has no resolution rules")
        )
    );
}

#[test]
fn an_empty_order_is_a_configuration_error_before_any_request() {
    let gamma = Fixture::start_sequence(Vec::new());
    let clob = Fixture::start_sequence(Vec::new());
    let environment = RunEnvironment::new(&format!(
        "{}\n[platforms.polymarket]\norder = []\n",
        config(&gamma.url, &clob.url)
    ));

    let output = run(&environment, &["fetch", &format!("polymarket:{EVENT}")]);
    gamma.finish_all();
    clob.finish_all();

    assert_eq!(
        (output.status.code(), stderr(&output).trim().to_owned()),
        (
            Some(3),
            "config_error: platforms.polymarket.order has no configured route for polymarket fetch"
                .to_owned()
        )
    );
}

#[test]
fn unsupported_depths_and_unrecognized_refs_fail_before_any_request() {
    let gamma = Fixture::start_sequence(Vec::new());
    let clob = Fixture::start_sequence(Vec::new());
    let environment = RunEnvironment::new(&config(&gamma.url, &clob.url));

    let codes = [
        run(
            &environment,
            &[
                "fetch",
                &format!("polymarket:{EVENT}"),
                "--depth",
                "full_text",
            ],
        ),
        run(
            &environment,
            &[
                "fetch",
                "https://polymarket.us/event/fedfnds-ondt-2027-12-31",
            ],
        ),
        run(&environment, &["fetch", "polymarket:51456"]),
        run(
            &environment,
            &["fetch", "https://polymarket.com/sports/nfl"],
        ),
    ]
    .map(|output| output.status.code());
    gamma.finish_all();
    clob.finish_all();

    assert_eq!(codes, [Some(2); 4]);
}
