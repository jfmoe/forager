use chrono::NaiveDate;
use serde_json::{Value, json};

use super::decode_page;
use crate::fixture_server::{Fixture, Response, block_on, ok_json, pairs, path, query};
use crate::polymarket_api_samples::{market, resolved, tokens};
use crate::providers::polymarket_api::tests::{event, reference, route};
use crate::samples::{sample, sample_json, with};
use crate::types::{
    AttemptErrorKind, PolymarketBucket, PolymarketHistoryOutcome, PolymarketHistoryRequest,
    PolymarketInterval, PolymarketWindow, ProviderError,
};

/// The stored Data API price history page holding `(timestamp, price, resolution_seconds)`
/// points; `next_cursor` marks that another page follows.
fn price_page(points: &[(i64, f64, i64)], next_cursor: Option<&str>) -> Value {
    let stored = sample_json("polymarket_api/price-history.json");
    let point = &stored["data"][0];
    let data = points
        .iter()
        .map(|(timestamp, price, resolution)| {
            with(
                point.clone(),
                &json!({"timestamp": timestamp, "price": price, "resolution_seconds": resolution}),
            )
        })
        .collect::<Vec<_>>();
    let mut page = with(stored, &json!({"data": data}));
    page["pagination"]["has_more"] = json!(next_cursor.is_some());
    page["pagination"]["next_cursor"] = json!(next_cursor);
    page
}

const WINNER: &str = "will-donald-trump-win-the-2024-us-presidential-election";
const OPEN: &str = "will-no-fed-rate-cuts-happen-in-2026";

fn interval(interval: PolymarketInterval) -> PolymarketWindow {
    PolymarketWindow::Interval(interval)
}

fn day(day: &str) -> NaiveDate {
    day.parse().expect("a day")
}

fn request(
    market: &str,
    outcome: Option<&str>,
    window: PolymarketWindow,
    bucket: Option<PolymarketBucket>,
) -> PolymarketHistoryRequest {
    PolymarketHistoryRequest::new(
        reference(market),
        outcome.map(str::to_owned),
        window,
        bucket,
    )
    .expect("a valid history request")
}

/// Reads `request`'s history against a fixture that answers `answers`, and returns the outcome
/// and the requests the fixture received.
fn history(
    request: &PolymarketHistoryRequest,
    answers: Vec<Response>,
) -> (Result<PolymarketHistoryOutcome, ProviderError>, Vec<String>) {
    let fixture = Fixture::start_sequence(answers);
    let outcome = block_on(route(&fixture.url).history(request));
    (outcome, fixture.finish_all())
}

fn points(outcome: &PolymarketHistoryOutcome) -> Vec<(String, f64)> {
    outcome
        .points
        .iter()
        .map(|point| (point.t.clone(), point.price))
        .collect()
}

#[test]
fn a_resolved_market_lists_its_daily_prices_and_gives_its_payout_as_the_settlement() {
    let [yes, _] = tokens(WINNER);

    let (outcome, requests) = history(
        &request(
            &format!("polymarket:market/{WINNER}"),
            None,
            interval(PolymarketInterval::Max),
            None,
        ),
        vec![
            ok_json(&resolved(WINNER, "Donald Trump", true)),
            Response::json(200, &sample("polymarket_api/price-history.json")),
        ],
    );
    let outcome = outcome.expect("history succeeds");

    assert_eq!(
        (
            requests
                .iter()
                .map(|request| (path(request), query(request)))
                .collect::<Vec<_>>(),
            serde_json::to_value(&outcome.market).expect("market"),
            outcome.outcome.as_str(),
            points(&outcome),
            serde_json::to_value(&outcome.settlement).expect("settlement"),
        ),
        (
            vec![
                (format!("/markets/slug/{WINNER}"), pairs(&[])),
                (
                    "/v2/prices-history".to_owned(),
                    pairs(&[
                        ("token_id", yes.as_str()),
                        ("interval", "max"),
                        ("bucket_seconds", "86400"),
                    ])
                ),
            ],
            json!({
                "ref": format!("polymarket:market/{WINNER}"),
                "url": format!("https://polymarket.com/market/{WINNER}"),
                "question": "Will Donald Trump happen?",
                "status": "resolved"
            }),
            "Yes",
            vec![
                ("2024-11-05T00:00:00Z".to_owned(), 0.5855),
                ("2024-11-06T00:00:00Z".to_owned(), 0.58),
            ],
            json!({"t": "2024-11-07T00:00:00Z", "price": 1.0}),
        )
    );
}

#[test]
fn both_requests_are_attempts_of_the_polymarket_history_operation() {
    let (outcome, _) = history(
        &request(
            &format!("polymarket:market/{WINNER}"),
            None,
            interval(PolymarketInterval::Month),
            None,
        ),
        vec![
            ok_json(&resolved(WINNER, "Donald Trump", true)),
            ok_json(&price_page(&[], None)),
        ],
    );
    let targets = outcome
        .expect("history succeeds")
        .attempts
        .iter()
        .map(|attempt| {
            let attempt = serde_json::to_value(attempt).expect("attempt");
            (
                attempt["provider"].clone(),
                attempt["platform"].clone(),
                attempt["operation"].clone(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        targets,
        vec![
            (
                json!("polymarket_api"),
                json!("polymarket"),
                json!("history")
            );
            2
        ]
    );
}

#[test]
fn each_interval_asks_the_data_api_for_itself_in_its_default_bucket() {
    let [yes, _] = tokens(OPEN);

    let sent = [
        PolymarketInterval::Day,
        PolymarketInterval::Week,
        PolymarketInterval::Month,
        PolymarketInterval::Max,
    ]
    .map(|window| {
        let (outcome, requests) = history(
            &request(
                &format!("polymarket:market/{OPEN}"),
                None,
                interval(window),
                None,
            ),
            vec![
                ok_json(&market(OPEN, "No cuts")),
                ok_json(&price_page(&[], None)),
            ],
        );
        outcome.expect("history succeeds");
        query(&requests[1])
    });

    assert_eq!(
        sent,
        [
            ("1d", "300"),
            ("1w", "3600"),
            ("1m", "86400"),
            ("max", "86400")
        ]
        .map(|(window, bucket)| pairs(&[
            ("token_id", yes.as_str()),
            ("interval", window),
            ("bucket_seconds", bucket),
        ]))
    );
}

#[test]
fn an_open_market_keeps_its_latest_tick_in_a_month_of_daily_prices_and_has_no_settlement() {
    let [yes, _] = tokens(OPEN);

    let (outcome, requests) = history(
        &request(
            &format!("polymarket:market/{OPEN}"),
            None,
            interval(PolymarketInterval::Month),
            None,
        ),
        vec![
            ok_json(&market(OPEN, "No cuts")),
            ok_json(&price_page(
                &[
                    (1_792_627_200, 0.9695, 86_400),
                    (1_792_713_600, 0.9645, 86_400),
                    (1_792_827_258, 0.9625, 0),
                ],
                None,
            )),
        ],
    );
    let outcome = outcome.expect("history succeeds");

    assert_eq!(
        (
            query(&requests[1]),
            serde_json::to_value(outcome.market.status).expect("status"),
            outcome.settlement.is_none(),
            points(&outcome),
        ),
        (
            pairs(&[
                ("token_id", yes.as_str()),
                ("interval", "1m"),
                ("bucket_seconds", "86400"),
            ]),
            json!("open"),
            true,
            vec![
                ("2026-10-22T00:00:00Z".to_owned(), 0.9695),
                ("2026-10-23T00:00:00Z".to_owned(), 0.9645),
                ("2026-10-24T07:34:18Z".to_owned(), 0.9625),
            ],
        )
    );
}

#[test]
fn an_event_with_one_market_stands_for_that_market() {
    let slug = "another-fed-rate-hike-in-2026";
    let [yes, _] = tokens(slug);

    let (outcome, requests) = history(
        &request(
            &format!("https://polymarket.com/event/{slug}"),
            None,
            interval(PolymarketInterval::Month),
            None,
        ),
        vec![
            ok_json(&event(slug, &[market(slug, "Hike")])),
            ok_json(&price_page(&[], None)),
        ],
    );

    assert_eq!(
        (
            path(&requests[0]),
            query(&requests[1])["token_id"].clone(),
            outcome
                .expect("history succeeds")
                .market
                .reference
                .to_string(),
        ),
        (
            format!("/events/slug/{slug}"),
            yes,
            format!("polymarket:market/{slug}"),
        )
    );
}

#[test]
fn the_outcome_is_matched_by_name_in_any_letter_case() {
    let [_, no] = tokens(OPEN);

    let (outcome, requests) = history(
        &request(
            &format!("polymarket:market/{OPEN}"),
            Some("NO"),
            interval(PolymarketInterval::Month),
            None,
        ),
        vec![
            ok_json(&market(OPEN, "No cuts")),
            ok_json(&price_page(&[(1_792_627_200, 0.0305, 86_400)], None)),
        ],
    );

    assert_eq!(
        (
            query(&requests[1])["token_id"].clone(),
            outcome.expect("history succeeds").outcome,
        ),
        (no, "No".to_owned())
    );
}

#[test]
fn an_event_with_several_markets_names_the_first_ten_and_reads_no_history() {
    let markets = (1..=12)
        .map(|index| market(&format!("cut-{index}"), &format!("{index} cuts")))
        .collect::<Vec<_>>();
    let listed = (1..=10)
        .map(|index| format!("polymarket:market/cut-{index} ({index} cuts)"))
        .collect::<Vec<_>>()
        .join(", ");

    let (outcome, requests) = history(
        &request(
            "polymarket:how-many-fed-rate-cuts-in-2026",
            None,
            interval(PolymarketInterval::Month),
            None,
        ),
        vec![ok_json(&event("how-many-fed-rate-cuts-in-2026", &markets))],
    );
    let error = outcome
        .map(|_| ())
        .expect_err("an event of 12 markets fails");

    assert_eq!(
        (error.kind, error.message, error.diagnostic, requests.len()),
        (
            AttemptErrorKind::Parameter,
            "polymarket:how-many-fed-rate-cuts-in-2026 has 12 markets and history reads one; pass a market ref, such as one listed on stderr".to_owned(),
            Some(format!("markets: {listed}, and 2 more")),
            1,
        )
    );
}

#[test]
fn an_unknown_outcome_lists_the_outcomes_of_the_market() {
    let (outcome, requests) = history(
        &request(
            &format!("polymarket:market/{OPEN}"),
            Some("Maybe"),
            interval(PolymarketInterval::Month),
            None,
        ),
        vec![ok_json(&market(OPEN, "No cuts"))],
    );
    let error = outcome.map(|_| ()).expect_err("an unknown outcome fails");

    assert_eq!(
        (error.message, requests.len()),
        (
            format!("polymarket:market/{OPEN} has no outcome `Maybe`; its outcomes are Yes, No"),
            1
        )
    );
}

#[test]
fn an_explicit_range_asks_for_whole_utc_days_in_hourly_buckets() {
    let [yes, _] = tokens(OPEN);
    let window = PolymarketWindow::Range {
        from: day("2026-10-09"),
        to: day("2026-10-23"),
    };

    let (outcome, requests) = history(
        &request(&format!("polymarket:market/{OPEN}"), None, window, None),
        vec![
            ok_json(&market(OPEN, "No cuts")),
            ok_json(&price_page(&[(1_791_504_000, 0.9555, 3_600)], None)),
        ],
    );

    assert_eq!(
        (
            outcome.map(|_| ()).map_err(|error| error.message),
            query(&requests[1]),
            serde_json::to_value(window).expect("window"),
        ),
        (
            Ok(()),
            pairs(&[
                ("token_id", yes.as_str()),
                ("start", "1791504000"),
                ("end", "1792800000"),
                ("bucket_seconds", "3600"),
            ]),
            json!({"interval": null, "from": "2026-10-09", "to": "2026-10-23"}),
        )
    );
}

#[test]
fn a_continued_series_resends_its_query_with_the_cursor_and_settles_on_the_last_page() {
    let [yes, _] = tokens(WINNER);

    let (outcome, requests) = history(
        &request(
            &format!("polymarket:market/{WINNER}"),
            None,
            interval(PolymarketInterval::Max),
            None,
        ),
        vec![
            ok_json(&resolved(WINNER, "Donald Trump", true)),
            ok_json(&price_page(
                &[(1_730_764_800, 0.5855, 86_400)],
                Some("page-2"),
            )),
            ok_json(&price_page(
                &[(1_730_851_200, 0.58, 86_400), (1_730_937_600, 1.0, 0)],
                None,
            )),
        ],
    );
    let outcome = outcome.expect("history succeeds");

    assert_eq!(
        (
            requests[1..]
                .iter()
                .map(|request| query(request))
                .collect::<Vec<_>>(),
            points(&outcome),
            serde_json::to_value(&outcome.settlement).expect("settlement"),
        ),
        (
            vec![
                pairs(&[
                    ("token_id", yes.as_str()),
                    ("interval", "max"),
                    ("bucket_seconds", "86400"),
                ]),
                pairs(&[
                    ("token_id", yes.as_str()),
                    ("interval", "max"),
                    ("bucket_seconds", "86400"),
                    ("cursor", "page-2"),
                ]),
            ],
            vec![
                ("2024-11-05T00:00:00Z".to_owned(), 0.5855),
                ("2024-11-06T00:00:00Z".to_owned(), 0.58),
            ],
            json!({"t": "2024-11-07T00:00:00Z", "price": 1.0}),
        )
    );
}

#[test]
fn a_series_longer_than_five_pages_is_cut_with_a_diagnostic() {
    let mut answers = vec![ok_json(&market(OPEN, "No cuts"))];
    answers.extend((0..5).map(|page| {
        ok_json(&price_page(
            &[(1_792_627_200 + page * 300, 0.9695, 300)],
            Some(&format!("page-{}", page + 2)),
        ))
    }));

    let (outcome, requests) = history(
        &request(
            &format!("polymarket:market/{OPEN}"),
            None,
            interval(PolymarketInterval::Day),
            None,
        ),
        answers,
    );
    let outcome = outcome.expect("history succeeds");

    assert_eq!(
        (requests.len() - 1, outcome.points.len(), outcome.diagnostic),
        (
            5,
            5,
            Some("Polymarket price history has more than 5 pages; listed the first 5 points; pass a larger --bucket to list the whole window".to_owned()),
        )
    );
}

#[test]
fn a_bucket_past_its_retention_is_an_empty_series_not_a_failure() {
    let (outcome, requests) = history(
        &request(
            &format!("polymarket:market/{WINNER}"),
            None,
            interval(PolymarketInterval::Max),
            Some(PolymarketBucket::OneHour),
        ),
        vec![
            ok_json(&resolved(WINNER, "Donald Trump", true)),
            ok_json(&price_page(&[], None)),
        ],
    );
    let outcome = outcome.expect("history succeeds");

    assert_eq!(
        (
            query(&requests[1])["bucket_seconds"].clone(),
            outcome.points.len(),
            outcome.settlement.is_none(),
        ),
        ("3600".to_owned(), 0, true)
    );
}

#[test]
fn a_data_api_refusal_fails_the_history_with_the_upstream_reason() {
    let (outcome, _) = history(
        &request(
            &format!("polymarket:market/{OPEN}"),
            None,
            interval(PolymarketInterval::Month),
            None,
        ),
        vec![
            ok_json(&market(OPEN, "No cuts")),
            Response::json(400, &sample("polymarket_api/data-error.json")),
        ],
    );
    let error = outcome.map(|_| ()).expect_err("a refused history fails");

    assert_eq!(
        (error.kind, error.message.as_str()),
        (
            AttemptErrorKind::Parameter,
            "Polymarket Data API returned HTTP 400: bucket_seconds must be at least 600 for this interval"
        )
    );
}

#[test]
fn prices_written_as_decimal_strings_are_read_as_numbers() {
    let mut page = sample_json("polymarket_api/price-history.json");
    page["data"] = json!([with(
        page["data"][0].clone(),
        &json!({"timestamp": 1_792_627_200, "price": "0.9695"}),
    )]);

    let page = decode_page(&page.to_string()).map_err(|refusal| refusal.message);

    assert_eq!(
        page.map(|page| {
            page.points
                .into_iter()
                .map(|point| {
                    let price = point.price;
                    (point.output().t, price)
                })
                .collect::<Vec<_>>()
        }),
        Ok(vec![("2026-10-22T00:00:00Z".to_owned(), 0.9695)])
    );
}
