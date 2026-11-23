use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::{Value, json};

use super::gamma::Quotes;
use super::quotes::{LiveQuotes, decode_last_trades, decode_prices, live_prices, open_markets};
use super::{PolymarketApi, decode_entry, decode_search, fetch_support, search_support};
use crate::config::PolymarketRouteRuntimeConfig;
use crate::fixture_server::{Fixture, Response};
use crate::net::{RetryPolicy, build_client};
use crate::providers::RouteContext;
use crate::providers::shared::Refusal;
// The stored bodies below read the sample store through this name.
use crate::samples;
use crate::types::{
    AttemptErrorKind, ContentDepth, Deadline, Platform, PlatformFetchOutcome, PlatformFetchRequest,
    PlatformItem, PlatformItemData, PlatformRef, PlatformSearchOptions, PlatformSearchOutcome,
    PlatformSearchRequest, PolymarketRef, PolymarketSearchOptions, ProviderError,
};

#[path = "../../../tests/support/polymarket_samples.rs"]
pub(super) mod stored;

use stored::{
    event, last_trades, market, no_match, placeholder, prices, resolved, search_page, tokens, with,
};

const EVENT: &str = "how-many-fed-rate-cuts-in-2026";
/// When the CLOB price read of a decoding test completed.
const READ_AT: &str = "2026-10-24T07:35:00Z";

/// The route against `url` for all three APIs, with one attempt per request, so a failure
/// response is terminal at once.
pub(super) fn route(url: &str) -> PolymarketApi {
    PolymarketApi::build(
        PolymarketRouteRuntimeConfig {
            url: url.to_owned(),
            clob_url: url.to_owned(),
            data_url: url.to_owned(),
            timeout_seconds: 5,
        },
        RouteContext {
            client: build_client(true).expect("build client"),
            retry_policy: RetryPolicy::new(1, 1.0, Duration::ZERO),
            deadline: Deadline::new(Duration::from_secs(10)),
        },
    )
}

pub(super) fn block_on<T>(future: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime")
        .block_on(future)
}

pub(super) fn json(body: &Value) -> Response {
    Response::json(200, &body.to_string())
}

/// The path of a recorded request.
pub(super) fn path(request: &str) -> String {
    let target = request.split_whitespace().nth(1).expect("request target");
    target.split('?').next().unwrap_or_default().to_owned()
}

pub(super) fn query(request: &str) -> BTreeMap<String, String> {
    let target = request.split_whitespace().nth(1).expect("request target");
    reqwest::Url::parse(&format!("http://fixture.test{target}"))
        .expect("request URL")
        .query_pairs()
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}

pub(super) fn pairs(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

fn request_json(request: &str) -> Value {
    let (_, body) = request.split_once("\r\n\r\n").expect("request body");
    serde_json::from_str(body).expect("request JSON")
}

pub(super) fn reference(input: &str) -> PolymarketRef {
    PolymarketRef::parse(input).expect("valid ref")
}

fn refused<T>(result: Result<T, Refusal>) -> Result<T, (AttemptErrorKind, String)> {
    result.map_err(|refusal| (refusal.kind, refusal.message))
}

fn search_request(query: &str, limit: u16, include_closed: bool) -> PlatformSearchRequest {
    PlatformSearchRequest {
        query: query.to_owned(),
        limit,
        options: PlatformSearchOptions::Polymarket(PolymarketSearchOptions { include_closed }),
        page: None,
    }
}

/// Searches with `request` against a fixture that answers `answers`, and returns the outcome
/// and the requests the fixture received.
fn search(
    request: &PlatformSearchRequest,
    answers: Vec<Response>,
) -> (Result<PlatformSearchOutcome, ProviderError>, Vec<String>) {
    let fixture = Fixture::start_sequence(answers);
    let outcome = block_on(route(&fixture.url).search(request));
    (outcome, fixture.finish_all())
}

/// Fetches `input` at `depth` against a fixture that answers `answers`, and returns the outcome
/// and the requests the fixture received.
fn fetch(
    input: &str,
    depth: ContentDepth,
    answers: Vec<Response>,
) -> (Result<PlatformFetchOutcome, ProviderError>, Vec<String>) {
    let fixture = Fixture::start_sequence(answers);
    let request = PlatformFetchRequest::new(
        PlatformRef::parse(Platform::Polymarket, input).expect("valid ref"),
        depth,
    );
    let outcome = block_on(route(&fixture.url).fetch(&request));
    (outcome, fixture.finish_all())
}

fn search_items(body: &Value) -> Result<(Value, usize, bool), (AttemptErrorKind, String)> {
    let (items, skipped, has_more) = refused(decode_search(&body.to_string()))?;
    let items = serde_json::to_value(items).expect("serialize items");
    Ok((items, skipped, has_more))
}

/// The item a fetch of `input` at `depth` builds from Gamma's answer `entry`, priced from the
/// CLOB's `(prices, last trades)` answers, or from Gamma's cache without them.
fn fetched_item(
    input: &str,
    entry: &Value,
    depth: ContentDepth,
    clob: Option<(&Value, &Value)>,
) -> PlatformItem {
    let entry =
        refused(decode_entry(&reference(input), &entry.to_string())).expect("the entry decodes");
    let quotes = match clob {
        None => Quotes::Cached,
        Some((prices, trades)) => {
            let live = LiveQuotes {
                tops: refused(decode_prices(&prices.to_string())).expect("prices decode"),
                last_trades: Some(
                    refused(decode_last_trades(&trades.to_string())).expect("last trades decode"),
                ),
                as_of: READ_AT.to_owned(),
            };
            live_prices(&open_markets(entry.markets()), live).0
        }
    };
    entry.into_item(depth, &quotes)
}

fn fetched(
    input: &str,
    entry: &Value,
    depth: ContentDepth,
    clob: Option<(&Value, &Value)>,
) -> Value {
    serde_json::to_value(fetched_item(input, entry, depth, clob)).expect("serialize item")
}

/// Six markets of one event: two-sided books, an empty book whose cached price is the 0.5
/// placeholder, a one-sided book, a resolved market, and a closed one.
fn fed_event() -> Value {
    event(
        EVENT,
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
fn a_search_result_is_its_event_with_its_most_probable_markets_priced_from_gamma() {
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
        search_items(&search_page(&[fed_event()], false)),
        Ok((
            json!([{
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
            }]),
            0,
            false
        ))
    );
}

#[test]
fn a_search_sends_its_words_size_and_page_and_filters_to_open_events_unless_closed_are_included() {
    let mut later_closed = search_request("fed rate", 3, true);
    later_closed.page = Some("2".to_owned());
    let cases = [
        (
            search_request("fed rate", 10, false),
            pairs(&[
                ("q", "fed rate"),
                ("limit_per_type", "10"),
                ("page", "1"),
                ("search_tags", "false"),
                ("search_profiles", "false"),
                ("events_status", "active"),
            ]),
        ),
        (
            later_closed,
            pairs(&[
                ("q", "fed rate"),
                ("limit_per_type", "3"),
                ("page", "2"),
                ("search_tags", "false"),
                ("search_profiles", "false"),
            ]),
        ),
    ];

    for (request, expected) in cases {
        let (outcome, requests) = search(&request, vec![json(&no_match())]);

        assert_eq!(
            (
                outcome.map(|_| ()).map_err(|error| error.message),
                requests
                    .iter()
                    .map(|sent| (path(sent), query(sent)))
                    .collect::<Vec<_>>()
            ),
            (Ok(()), vec![("/public-search".to_owned(), expected)]),
            "request: {request:?}"
        );
    }
}

#[test]
fn a_page_with_more_results_continues_at_the_next_page() {
    let next_pages = [true, false].map(|has_more| {
        let (outcome, _) = search(
            &search_request("fed rate", 10, false),
            vec![json(&search_page(&[fed_event()], has_more))],
        );
        outcome.expect("search succeeds").next_page
    });

    assert_eq!(next_pages, [Some("2".to_owned()), None]);
}

#[test]
fn a_search_nothing_matches_is_an_empty_page() {
    assert_eq!(search_items(&no_match()), Ok((json!([]), 0, false)));
}

#[test]
fn results_without_a_valid_slug_or_markets_are_skipped_with_one_diagnostic() {
    let mut without_markets = event("an-event-without-markets", &[]);
    without_markets
        .as_object_mut()
        .expect("event object")
        .remove("markets");
    let with_legacy_market_slug = event(
        "biden-approval-rating",
        &[market(
            "will-joe-bidens-approval-rating-be-49.0-or-higher-on-september-1",
            "49.0 or higher",
        )],
    );
    let page = search_page(
        &[
            with(fed_event(), &json!({"slug": "not a slug"})),
            fed_event(),
            without_markets,
            with_legacy_market_slug,
        ],
        false,
    );

    let (outcome, _) = search(&search_request("fed rate", 10, false), vec![json(&page)]);
    let outcome = outcome.expect("search succeeds");

    assert_eq!(
        (
            outcome
                .items
                .iter()
                .map(|item| item.reference.to_string())
                .collect::<Vec<_>>(),
            outcome.diagnostic,
        ),
        (
            vec!["polymarket:how-many-fed-rate-cuts-in-2026".to_owned()],
            Some("skipped 3 Polymarket search results that could not be read".to_owned()),
        )
    );
}

#[test]
fn a_page_whose_every_result_is_unreadable_is_a_runtime_failure() {
    let page = search_page(&[with(fed_event(), &json!({"slug": "not a slug"}))], false);

    assert_eq!(
        search_items(&page),
        Err((
            AttemptErrorKind::Runtime,
            "Polymarket Gamma API returned 1 search results and none could be read".to_owned()
        ))
    );
}

#[test]
fn a_result_with_an_undecodable_field_fails_the_page_as_runtime() {
    let broken = event(
        "broken-outcomes",
        &[with(
            market("will-1-fed-rate-cut-happen-in-2026", "1 (25 bps)"),
            &json!({"outcomes": "[\"Yes\", "}),
        )],
    );

    assert_eq!(
        search_items(&search_page(&[fed_event(), broken], false)),
        Err((
            AttemptErrorKind::Runtime,
            "Polymarket Gamma API returned polymarket:market/will-1-fed-rate-cut-happen-in-2026 with an undecodable `outcomes`: EOF while parsing a value at line 1 column 8".to_owned()
        ))
    );
}

#[test]
fn a_page_without_pagination_is_a_runtime_failure() {
    let mut page = search_page(&[fed_event()], false);
    page.as_object_mut()
        .expect("page object")
        .remove("pagination");

    let result = search_items(&page);

    assert!(
        matches!(result, Err((AttemptErrorKind::Runtime, _))),
        "{result:?}"
    );
}

#[test]
fn a_gamma_validation_error_is_a_forager_defect_not_a_user_error() {
    let (outcome, _) = search(
        &search_request("fed rate", 10, false),
        vec![Response::json(422, &stored::stored("gamma-error.json"))],
    );
    let error = outcome.map(|_| ()).expect_err("a validation error fails");

    assert_eq!(
        (error.kind, error.message.as_str()),
        (
            AttemptErrorKind::Runtime,
            "Polymarket Gamma API returned HTTP 422: query argument \"page\": must be less than or equal to 100"
        )
    );
}

#[test]
fn a_page_past_the_last_one_gamma_serves_is_refused_before_any_request() {
    let mut request = search_request("fed rate", 10, false);
    request.page = Some("101".to_owned());

    assert_eq!(
        search_support(&request),
        Err("polymarket_api cannot read past page 100 of a search; narrow the query".to_owned())
    );
}

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

/// The live books of the three open markets of [`derivation_event`].
fn live_books() -> Value {
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

fn outcome_price(name: &str, price: Value, basis: &str, bid: Value, ask: Value) -> Value {
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

/// The pricing of every market of [`derivation_event`], one row of the derivation each.
fn derived_prices() -> Vec<Value> {
    vec![
        priced(
            "Two-sided",
            "open",
            json!(0.962),
            [
                outcome_price("Yes", json!(0.962), "midpoint", json!(0.961), json!(0.963)),
                outcome_price("No", json!(0.038), "midpoint", json!(0.037), json!(0.039)),
            ],
            json!(0.002),
            json!(0.962),
        ),
        priced(
            "Empty book",
            "open",
            none(),
            [
                outcome_price("Yes", none(), "no_two_sided_book", none(), none()),
                outcome_price("No", none(), "no_two_sided_book", none(), none()),
            ],
            none(),
            none(),
        ),
        priced(
            "One-sided",
            "open",
            none(),
            [
                outcome_price("Yes", none(), "no_two_sided_book", json!(0.2), none()),
                outcome_price("No", none(), "no_two_sided_book", none(), json!(0.8)),
            ],
            none(),
            none(),
        ),
        priced(
            "Resolved",
            "resolved",
            json!(1.0),
            [
                outcome_price("Yes", json!(1.0), "resolution", none(), none()),
                outcome_price("No", json!(0.0), "resolution", none(), none()),
            ],
            none(),
            none(),
        ),
        priced(
            "Closed",
            "closed",
            none(),
            [
                outcome_price("Yes", none(), "not_trading", none(), none()),
                outcome_price("No", none(), "not_trading", none(), none()),
            ],
            none(),
            none(),
        ),
        priced(
            "Suspended",
            "suspended",
            none(),
            [
                outcome_price("Yes", none(), "not_trading", none(), none()),
                outcome_price("No", none(), "not_trading", none(), none()),
            ],
            none(),
            none(),
        ),
    ]
}

#[test]
fn each_outcome_is_priced_by_its_market_status_and_live_book() {
    let [two_yes, _] = tokens("two-sided");

    let item = fetched(
        &format!("polymarket:{EVENT}"),
        &derivation_event(),
        ContentDepth::Metadata,
        Some((&live_books(), &last_trades(&[(&two_yes, "0.962")]))),
    );

    assert_eq!(pricing(&item), derived_prices());
}

#[test]
fn a_fetch_reads_the_entry_then_the_books_and_last_trades_of_its_open_markets_alone() {
    let [two_yes, _] = tokens("two-sided");
    let open_tokens = ["two-sided", "empty-book", "one-sided"]
        .iter()
        .flat_map(|slug| tokens(slug))
        .collect::<Vec<_>>();

    let (outcome, requests) = fetch(
        &format!("polymarket:{EVENT}"),
        ContentDepth::Metadata,
        vec![
            json(&derivation_event()),
            json(&live_books()),
            json(&last_trades(&[(&two_yes, "0.962")])),
        ],
    );
    let item = serde_json::to_value(outcome.expect("fetch succeeds").item).expect("item");

    assert_eq!(
        (
            requests
                .iter()
                .map(|request| path(request))
                .collect::<Vec<_>>(),
            request_json(&requests[1]),
            request_json(&requests[2]),
            item["price_source"].clone(),
        ),
        (
            vec![
                format!("/events/slug/{EVENT}"),
                "/prices".to_owned(),
                "/last-trades-prices".to_owned()
            ],
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
            json!("clob"),
        )
    );
}

#[test]
fn live_prices_are_as_of_the_second_the_clob_read_them_in_utc() {
    let [yes, no] = tokens("two-sided");

    let (outcome, _) = fetch(
        &format!("polymarket:{EVENT}"),
        ContentDepth::Metadata,
        vec![
            json(&event(EVENT, &[market("two-sided", "Two-sided")])),
            json(&prices(&[
                (&yes, "0.961", "0.963"),
                (&no, "0.037", "0.039"),
            ])),
            json(&last_trades(&[])),
        ],
    );
    let item = serde_json::to_value(outcome.expect("fetch succeeds").item).expect("item");
    let prices_as_of = &item["prices_as_of"];

    assert!(
        prices_as_of
            .as_str()
            .is_some_and(|time| time.len() == 20 && time.ends_with('Z')),
        "{prices_as_of}"
    );
}

#[test]
fn an_event_carries_its_identity_and_scale() {
    let [yes, no] = tokens("two-sided");

    let mut item = fetched(
        &format!("https://polymarket.com/zh/event/{EVENT}?tid=1761200000000"),
        &event(EVENT, &[market("two-sided", "Two-sided")]),
        ContentDepth::Metadata,
        Some((
            &prices(&[(&yes, "0.961", "0.963"), (&no, "0.037", "0.039")]),
            &last_trades(&[]),
        )),
    );
    item["prices_as_of"].take();
    item["markets"][0]["outcomes"].take();

    assert_eq!(
        item,
        json!({
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
    let (outcome, _) = fetch(
        &format!("polymarket:{EVENT}"),
        ContentDepth::Metadata,
        vec![
            json(&derivation_event()),
            Response::json(500, &stored::stored("clob-error.json")),
        ],
    );
    let outcome = outcome.expect("the fetch prices from Gamma");
    let item = serde_json::to_value(&outcome.item).expect("item");

    assert_eq!(
        (
            item["price_source"].clone(),
            item["prices_as_of"].clone(),
            pricing(&item)[..3].to_vec(),
            outcome.diagnostic,
        ),
        (
            json!("gamma"),
            json!("2026-10-24T06:34:54Z"),
            vec![
                priced(
                    "Two-sided",
                    "open",
                    json!(0.962),
                    [
                        outcome_price("Yes", json!(0.962), "midpoint_cached", json!(0.961), json!(0.963)),
                        outcome_price("No", json!(0.038), "midpoint_cached", json!(0.037), json!(0.039)),
                    ],
                    json!(0.002),
                    none(),
                ),
                priced(
                    "Empty book",
                    "open",
                    none(),
                    [
                        outcome_price("Yes", none(), "no_two_sided_book", none(), none()),
                        outcome_price("No", none(), "no_two_sided_book", none(), none()),
                    ],
                    none(),
                    none(),
                ),
                priced(
                    "One-sided",
                    "open",
                    json!(0.962),
                    [
                        outcome_price("Yes", json!(0.962), "midpoint_cached", json!(0.961), json!(0.963)),
                        outcome_price("No", json!(0.038), "midpoint_cached", json!(0.037), json!(0.039)),
                    ],
                    json!(0.002),
                    none(),
                ),
            ],
            Some("Polymarket CLOB API unavailable (Polymarket CLOB API returned HTTP 500: internal); prices are Gamma's cached quotes, which may lag up to 5 minutes".to_owned()),
        )
    );
}

#[test]
fn inactive_placeholder_markets_are_suspended_and_never_reach_the_clob() {
    let [yes, no] = tokens("two-sided");

    let (outcome, requests) = fetch(
        &format!("polymarket:{EVENT}"),
        ContentDepth::Metadata,
        vec![
            json(&event(
                EVENT,
                &[
                    market("two-sided", "Two-sided"),
                    placeholder("will-person-a-win", "Person A"),
                ],
            )),
            json(&prices(&[
                (&yes, "0.961", "0.963"),
                (&no, "0.037", "0.039"),
            ])),
            json(&last_trades(&[])),
        ],
    );
    let outcome = outcome.expect("fetch succeeds");
    let item = serde_json::to_value(&outcome.item).expect("item");

    assert_eq!(
        (
            request_json(&requests[1]),
            pricing(&item)[1].clone(),
            outcome.diagnostic,
        ),
        (
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
                    outcome_price("Yes", none(), "not_trading", none(), none()),
                    outcome_price("No", none(), "not_trading", none(), none()),
                ],
                none(),
                none(),
            ),
            None,
        )
    );
}

#[test]
fn a_resolved_market_is_read_from_gamma_alone() {
    let slug = "will-donald-trump-win-the-2024-us-presidential-election";
    let resolved_market = with(
        resolved(slug, "Donald Trump", true),
        &json!({
            "events": [{
                "id": "903193",
                "slug": "presidential-election-winner-2024",
                "title": "Presidential Election Winner 2024"
            }],
            "updatedAt": "2024-11-07T15:23:00.123Z"
        }),
    );

    let (outcome, requests) = fetch(
        &format!("https://polymarket.com/event/presidential-election-winner-2024/{slug}"),
        ContentDepth::Metadata,
        vec![json(&resolved_market)],
    );
    let item = serde_json::to_value(outcome.expect("fetch succeeds").item).expect("item");

    assert_eq!(
        (
            requests
                .iter()
                .map(|request| path(request))
                .collect::<Vec<_>>(),
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
            .map(|field| item[field].clone()),
        ),
        (
            vec![format!("/markets/slug/{slug}")],
            [
                json!(format!("polymarket:market/{slug}")),
                json!("903193"),
                json!("616902"),
                json!(format!("0x{slug}")),
                json!("resolved"),
                json!(1.0),
                json!([
                    outcome_price("Yes", json!(1.0), "resolution", none(), none()),
                    outcome_price("No", json!(0.0), "resolution", none(), none()),
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
        )
    );
}

#[test]
fn outcomes_the_clob_leaves_out_have_no_price_and_failed_last_trades_are_null() {
    let [yes, _] = tokens("two-sided");

    let (outcome, _) = fetch(
        &format!("polymarket:{EVENT}"),
        ContentDepth::Metadata,
        vec![
            json(&event(EVENT, &[market("two-sided", "Two-sided")])),
            json(&prices(&[(&yes, "0.961", "0.963")])),
            Response::json(
                503,
                &with(
                    serde_json::from_str(&stored::stored("clob-error.json"))
                        .expect("stored CLOB error"),
                    &json!({"error": "busy"}),
                )
                .to_string(),
            ),
        ],
    );
    let outcome = outcome.expect("fetch succeeds");
    let item = serde_json::to_value(&outcome.item).expect("item");

    assert_eq!(
        (item["price_source"].clone(), pricing(&item), outcome.diagnostic),
        (
            json!("clob"),
            vec![priced(
                "Two-sided",
                "open",
                json!(0.962),
                [
                    outcome_price("Yes", json!(0.962), "midpoint", json!(0.961), json!(0.963)),
                    outcome_price("No", none(), "no_two_sided_book", none(), none()),
                ],
                json!(0.002),
                none(),
            )],
            Some("Polymarket CLOB API last trades unavailable (Polymarket CLOB API returned HTTP 503: busy); last_trade_price is null\nPolymarket CLOB API returned no quote for 1 outcome; it has no price".to_owned()),
        )
    );
}

#[test]
fn quotes_for_more_than_250_tokens_are_read_in_batches_of_500_entries() {
    let markets = (0..126)
        .map(|index| market(&format!("market-{index}"), &format!("Market {index}")))
        .collect::<Vec<_>>();

    let (outcome, requests) = fetch(
        &format!("polymarket:{EVENT}"),
        ContentDepth::Metadata,
        vec![
            json(&event(EVENT, &markets)),
            json(&prices(&[])),
            json(&prices(&[])),
            json(&last_trades(&[])),
        ],
    );

    assert_eq!(
        (
            outcome.map(|_| ()).map_err(|error| error.message),
            requests[1..]
                .iter()
                .map(|request| (
                    path(request),
                    request_json(request).as_array().map(Vec::len)
                ))
                .collect::<Vec<_>>(),
        ),
        (
            Ok(()),
            vec![
                ("/prices".to_owned(), Some(500)),
                ("/prices".to_owned(), Some(4)),
                ("/last-trades-prices".to_owned(), Some(252)),
            ]
        )
    );
}

#[test]
fn a_missing_slug_asks_the_caller_to_search_again() {
    let failures = ["polymarket:gone-event", "polymarket:market/gone-market"].map(|input| {
        let (outcome, _) = fetch(
            input,
            ContentDepth::Metadata,
            vec![Response::json(
                404,
                &stored::stored("gamma-error-not-found.json"),
            )],
        );
        let error = outcome.map(|_| ()).expect_err("a missing slug fails");
        (error.kind, error.message)
    });

    assert_eq!(
        failures,
        [
            (
                AttemptErrorKind::Parameter,
                "Polymarket event not found: polymarket:gone-event; slugs can change, search again"
                    .to_owned()
            ),
            (
                AttemptErrorKind::Parameter,
                "Polymarket market not found: polymarket:market/gone-market; slugs can change, search again"
                    .to_owned()
            ),
        ]
    );
}

#[test]
fn another_slug_or_a_broken_encoded_field_is_a_runtime_failure() {
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
    let answers = [
        event("another-event", &[market("two-sided", "Two-sided")]),
        event(EVENT, &[broken]),
        event(EVENT, &[short_of_tokens]),
        event(EVENT, &[resolved_short_of_tokens]),
    ];

    let failures = answers.map(|answer| {
        refused(decode_entry(
            &reference(&format!("polymarket:{EVENT}")),
            &answer.to_string(),
        ))
        .map(|_| ())
    });

    let short =
        "Polymarket Gamma API returned polymarket:market/two-sided with 2 outcomes but 1 tokens";
    assert_eq!(
        failures,
        [
            Err((
                AttemptErrorKind::Runtime,
                format!("Polymarket Gamma API answered polymarket:{EVENT} with `another-event`")
            )),
            Err((
                AttemptErrorKind::Runtime,
                "Polymarket Gamma API returned polymarket:market/two-sided with an undecodable `outcomes`: EOF while parsing a value at line 1 column 8".to_owned()
            )),
            Err((AttemptErrorKind::Runtime, short.to_owned())),
            Err((AttemptErrorKind::Runtime, short.to_owned())),
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

    let item = fetched(
        "polymarket:market/will-the-chiefs-win-super-bowl-55",
        &settled,
        ContentDepth::Metadata,
        None,
    );

    assert_eq!(
        (item["status"].clone(), item["outcomes"].clone()),
        (
            json!("resolved"),
            json!([
                outcome_price("Yes", json!(0.0), "resolution", none(), none()),
                outcome_price("No", json!(1.0), "resolution", none(), none()),
            ])
        )
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
    let [same_yes, same_no] = tokens("same-rules");
    let [own_yes, own_no] = tokens("own-rules");
    let quotes = prices(&[
        (&same_yes, "0.4", "0.5"),
        (&same_no, "0.5", "0.6"),
        (&own_yes, "0.4", "0.5"),
        (&own_no, "0.5", "0.6"),
    ]);

    let item = fetched_item(
        &format!("polymarket:{EVENT}"),
        &fetched_event,
        ContentDepth::Abstract,
        Some((&quotes, &last_trades(&[]))),
    );
    let PlatformItemData::Polymarket(data) = &item.data else {
        panic!("a Polymarket item")
    };
    let document = data.rules_document();
    let item = serde_json::to_value(&item).expect("item");

    assert_eq!(
        (
            item["depth"].clone(),
            item["rules"].clone(),
            item["markets"]
                .as_array()
                .expect("markets")
                .iter()
                .map(|market| market.get("rules").cloned())
                .collect::<Vec<_>>(),
            document,
        ),
        (
            json!("abstract"),
            json!("Event rules."),
            vec![None, Some(json!("Own rules."))],
            Some("Event rules.\n\n## Own\n\nOwn rules.".to_owned()),
        )
    );
}

#[test]
fn abstract_without_rules_is_a_quality_failure() {
    let (outcome, _) = fetch(
        "polymarket:market/two-sided",
        ContentDepth::Abstract,
        vec![json(&with(
            market("two-sided", "Two-sided"),
            &json!({"description": "  "}),
        ))],
    );
    let error = outcome
        .map(|_| ())
        .expect_err("a fetch without rules fails");

    assert_eq!(
        (error.kind, error.message.as_str()),
        (
            AttemptErrorKind::Quality,
            "polymarket:market/two-sided has no resolution rules"
        )
    );
}

#[test]
fn depths_beyond_the_resolution_rules_are_refused_before_any_request() {
    let request = PlatformFetchRequest::new(
        PlatformRef::parse(Platform::Polymarket, &format!("polymarket:{EVENT}"))
            .expect("valid ref"),
        ContentDepth::FullText,
    );

    assert_eq!(
        fetch_support(&request),
        Err(
            "polymarket_api cannot fetch at depth `full_text`; it reads metadata and the resolution rules (abstract)"
                .to_owned()
        )
    );
}
