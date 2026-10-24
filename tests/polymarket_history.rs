mod support;

use std::collections::BTreeMap;

use serde_json::{Value, json};

use support::hn::{path, payload, query, stderr};
use support::polymarket::{
    event, history_config, json as body, market, price_page, resolved, run, tokens,
};
use support::{Fixture, Response, RunEnvironment};

const WINNER: &str = "will-donald-trump-win-the-2024-us-presidential-election";
const OPEN: &str = "will-no-fed-rate-cuts-happen-in-2026";

/// Runs a history against fixtures that expect no request at all.
fn refused(arguments: &[&str]) -> (Option<i32>, String) {
    let gamma = Fixture::start_sequence(Vec::new());
    let data = Fixture::start_sequence(Vec::new());
    let environment = RunEnvironment::new(&history_config(&gamma.url, &data.url));
    let mut command = vec!["history"];
    command.extend_from_slice(arguments);
    let output = run(&environment, &command);
    gamma.finish_all();
    data.finish_all();
    (output.status.code(), stderr(&output))
}

fn points(history: &Value) -> Vec<(Value, Value)> {
    history["points"]
        .as_array()
        .expect("points")
        .iter()
        .map(|point| (point["t"].clone(), point["price"].clone()))
        .collect()
}

fn params<const N: usize>(pairs: [(&str, &str); N]) -> BTreeMap<String, String> {
    pairs
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect()
}

#[test]
fn a_resolved_market_lists_its_daily_prices_and_gives_its_payout_as_the_settlement() {
    let gamma = Fixture::start_sequence(vec![body(&resolved(WINNER, "Donald Trump", true))]);
    let data = Fixture::start_sequence(vec![price_page(
        &[
            (1_730_764_800, 0.5855, 86_400),
            (1_730_851_200, 0.58, 86_400),
            (1_730_937_600, 1.0, 0),
        ],
        None,
    )]);
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
fn verbose_records_both_requests_as_polymarket_history_attempts() {
    let gamma = Fixture::start_sequence(vec![body(&resolved(WINNER, "Donald Trump", true))]);
    let data = Fixture::start_sequence(vec![price_page(&[], None)]);
    let environment = RunEnvironment::new(&history_config(&gamma.url, &data.url));

    let output = run(
        &environment,
        &[
            "history",
            &format!("polymarket:market/{WINNER}"),
            "--verbose",
        ],
    );
    gamma.finish();
    data.finish();
    let targets = payload(&output)["provider_attempts"]
        .as_array()
        .expect("provider attempts")
        .iter()
        .map(|attempt| {
            (
                attempt["provider"].clone(),
                attempt["platform"].clone(),
                attempt["operation"].clone(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        (output.status.code(), targets),
        (
            Some(0),
            vec![
                (
                    json!("polymarket_api"),
                    json!("polymarket"),
                    json!("history")
                );
                2
            ]
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn an_open_market_keeps_its_latest_tick_in_a_month_of_daily_prices_and_has_no_settlement() {
    let gamma = Fixture::start_sequence(vec![body(&market(OPEN, "No cuts"))]);
    let data = Fixture::start_sequence(vec![price_page(
        &[
            (1_792_627_200, 0.9695, 86_400),
            (1_792_713_600, 0.9645, 86_400),
            (1_792_827_258, 0.9625, 0),
        ],
        None,
    )]);
    let environment = RunEnvironment::new(&history_config(&gamma.url, &data.url));

    let output = run(
        &environment,
        &["history", &format!("polymarket:market/{OPEN}")],
    );
    gamma.finish();
    let data_request = data.finish();
    let history = payload(&output);
    let [yes, _] = tokens(OPEN);

    assert_eq!(
        (
            output.status.code(),
            query(&data_request),
            [
                &history["market"]["status"],
                &history["interval"],
                &history["bucket_seconds"],
                &history["settlement"],
            ],
            points(&history),
        ),
        (
            Some(0),
            params([
                ("token_id", yes.as_str()),
                ("interval", "1m"),
                ("bucket_seconds", "86400"),
            ]),
            [&json!("open"), &json!("1m"), &json!(86_400), &Value::Null],
            vec![
                (json!("2026-10-22T00:00:00Z"), json!(0.9695)),
                (json!("2026-10-23T00:00:00Z"), json!(0.9645)),
                (json!("2026-10-24T07:34:18Z"), json!(0.9625)),
            ],
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn an_event_with_one_market_stands_for_that_market() {
    let slug = "another-fed-rate-hike-in-2026";
    let gamma = Fixture::start_sequence(vec![body(&event(slug, &[market(slug, "Hike")]))]);
    let data = Fixture::start_sequence(vec![price_page(&[], None)]);
    let environment = RunEnvironment::new(&history_config(&gamma.url, &data.url));

    let output = run(
        &environment,
        &["history", &format!("https://polymarket.com/event/{slug}")],
    );
    let gamma_request = gamma.finish();
    let data_request = data.finish();
    let [yes, _] = tokens(slug);

    assert_eq!(
        (
            output.status.code(),
            path(&gamma_request),
            query(&data_request)["token_id"].clone(),
            payload(&output)["market"]["ref"].clone(),
        ),
        (
            Some(0),
            format!("/events/slug/{slug}"),
            yes,
            json!(format!("polymarket:market/{slug}")),
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn the_outcome_is_matched_by_name_in_any_letter_case() {
    let gamma = Fixture::start_sequence(vec![body(&market(OPEN, "No cuts"))]);
    let data = Fixture::start_sequence(vec![price_page(&[(1_792_627_200, 0.0305, 86_400)], None)]);
    let environment = RunEnvironment::new(&history_config(&gamma.url, &data.url));

    let output = run(
        &environment,
        &[
            "history",
            &format!("polymarket:market/{OPEN}"),
            "--outcome",
            "NO",
        ],
    );
    gamma.finish();
    let data_request = data.finish();
    let [_, no] = tokens(OPEN);

    assert_eq!(
        (
            output.status.code(),
            query(&data_request)["token_id"].clone(),
            payload(&output)["outcome"].clone(),
        ),
        (Some(0), no, json!("No")),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn an_event_with_several_markets_names_the_first_ten_and_reads_no_history() {
    let markets = (1..=12)
        .map(|index| market(&format!("cut-{index}"), &format!("{index} cuts")))
        .collect::<Vec<_>>();
    let gamma = Fixture::start_sequence(vec![body(&event(
        "how-many-fed-rate-cuts-in-2026",
        &markets,
    ))]);
    let data = Fixture::start_sequence(Vec::new());
    let environment = RunEnvironment::new(&history_config(&gamma.url, &data.url));

    let output = run(
        &environment,
        &["history", "polymarket:how-many-fed-rate-cuts-in-2026"],
    );
    gamma.finish();
    data.finish_all();
    let listed = (1..=10)
        .map(|index| format!("polymarket:market/cut-{index} ({index} cuts)"))
        .collect::<Vec<_>>()
        .join(", ");

    let failure = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            failure["error_kind"].clone(),
            failure["message"].clone(),
            stderr(&output).trim().to_owned(),
        ),
        (
            Some(4),
            json!("parameter"),
            json!(
                "polymarket:how-many-fed-rate-cuts-in-2026 has 12 markets and history reads one; pass a market ref, such as one listed on stderr"
            ),
            format!("markets: {listed}, and 2 more"),
        )
    );
}

#[test]
fn an_unknown_outcome_lists_the_outcomes_of_the_market() {
    let gamma = Fixture::start_sequence(vec![body(&market(OPEN, "No cuts"))]);
    let data = Fixture::start_sequence(Vec::new());
    let environment = RunEnvironment::new(&history_config(&gamma.url, &data.url));

    let output = run(
        &environment,
        &[
            "history",
            &format!("polymarket:market/{OPEN}"),
            "--outcome",
            "Maybe",
        ],
    );
    gamma.finish();
    data.finish_all();

    assert_eq!(
        (output.status.code(), payload(&output)["message"].clone()),
        (
            Some(4),
            json!(format!(
                "polymarket:market/{OPEN} has no outcome `Maybe`; its outcomes are Yes, No"
            )),
        )
    );
}

#[test]
fn an_explicit_range_asks_for_whole_utc_days_in_hourly_buckets() {
    let gamma = Fixture::start_sequence(vec![body(&market(OPEN, "No cuts"))]);
    let data = Fixture::start_sequence(vec![price_page(&[(1_791_504_000, 0.9555, 3_600)], None)]);
    let environment = RunEnvironment::new(&history_config(&gamma.url, &data.url));

    let output = run(
        &environment,
        &[
            "history",
            &format!("polymarket:market/{OPEN}"),
            "--from",
            "2026-10-09",
            "--to",
            "2026-10-23",
        ],
    );
    gamma.finish();
    let data_request = data.finish();
    let history = payload(&output);
    let [yes, _] = tokens(OPEN);

    assert_eq!(
        (
            output.status.code(),
            query(&data_request),
            [
                &history["interval"],
                &history["from"],
                &history["to"],
                &history["bucket_seconds"],
            ],
        ),
        (
            Some(0),
            params([
                ("token_id", yes.as_str()),
                ("start", "1791504000"),
                ("end", "1792800000"),
                ("bucket_seconds", "3600"),
            ]),
            [
                &Value::Null,
                &json!("2026-10-09"),
                &json!("2026-10-23"),
                &json!(3_600),
            ],
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_continued_series_resends_its_query_with_the_cursor_and_settles_on_the_last_page() {
    let gamma = Fixture::start_sequence(vec![body(&resolved(WINNER, "Donald Trump", true))]);
    let data = Fixture::start_sequence(vec![
        price_page(&[(1_730_764_800, 0.5855, 86_400)], Some("page-2")),
        price_page(
            &[(1_730_851_200, 0.58, 86_400), (1_730_937_600, 1.0, 0)],
            None,
        ),
    ]);
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
    gamma.finish();
    let data_requests = data.finish_all();
    let history = payload(&output);
    let [yes, _] = tokens(WINNER);

    assert_eq!(
        (
            output.status.code(),
            data_requests
                .iter()
                .map(|request| query(request))
                .collect::<Vec<_>>(),
            points(&history),
            history["settlement"].clone(),
        ),
        (
            Some(0),
            vec![
                params([
                    ("token_id", yes.as_str()),
                    ("interval", "max"),
                    ("bucket_seconds", "86400"),
                ]),
                params([
                    ("token_id", yes.as_str()),
                    ("interval", "max"),
                    ("bucket_seconds", "86400"),
                    ("cursor", "page-2"),
                ]),
            ],
            vec![
                (json!("2024-11-05T00:00:00Z"), json!(0.5855)),
                (json!("2024-11-06T00:00:00Z"), json!(0.58)),
            ],
            json!({"t": "2024-11-07T00:00:00Z", "price": 1.0}),
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_series_longer_than_five_pages_is_cut_with_a_diagnostic() {
    let gamma = Fixture::start_sequence(vec![body(&market(OPEN, "No cuts"))]);
    let data = Fixture::start_sequence(
        (0..5)
            .map(|page| {
                price_page(
                    &[(1_792_627_200 + page * 300, 0.9695, 300)],
                    Some(&format!("page-{}", page + 2)),
                )
            })
            .collect(),
    );
    let environment = RunEnvironment::new(&history_config(&gamma.url, &data.url));

    let output = run(
        &environment,
        &[
            "history",
            &format!("polymarket:market/{OPEN}"),
            "--interval",
            "1d",
        ],
    );
    gamma.finish();
    let data_requests = data.finish_all();
    let history = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            data_requests.len(),
            history["points"].as_array().map(Vec::len),
            stderr(&output).trim().to_owned(),
        ),
        (
            Some(0),
            5,
            Some(5),
            "Polymarket price history has more than 5 pages; listed the first 5 points; pass a larger --bucket to list the whole window".to_owned(),
        )
    );
}

#[test]
fn a_bucket_past_its_retention_is_an_empty_series_not_a_failure() {
    let gamma = Fixture::start_sequence(vec![body(&resolved(WINNER, "Donald Trump", true))]);
    let data = Fixture::start_sequence(vec![price_page(&[], None)]);
    let environment = RunEnvironment::new(&history_config(&gamma.url, &data.url));

    let output = run(
        &environment,
        &[
            "history",
            &format!("polymarket:market/{WINNER}"),
            "--interval",
            "max",
            "--bucket",
            "1h",
        ],
    );
    gamma.finish();
    let data_request = data.finish();
    let history = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            query(&data_request)["bucket_seconds"].clone(),
            history["points"].clone(),
            history["settlement"].clone(),
        ),
        (Some(0), "3600".to_owned(), json!([]), Value::Null),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_data_api_refusal_fails_the_history_with_the_upstream_reason() {
    let gamma = Fixture::start_sequence(vec![body(&market(OPEN, "No cuts"))]);
    let data = Fixture::start_sequence(vec![Response::json(
        400,
        r#"{"error":"bucket_seconds must be at least 600 for this interval","code":"invalid_request","parameter":"bucket_seconds","retryable":false,"trace_id":"fcb5b19e"}"#,
    )]);
    let environment = RunEnvironment::new(&history_config(&gamma.url, &data.url));

    let output = run(
        &environment,
        &["history", &format!("polymarket:market/{OPEN}")],
    );
    gamma.finish();
    data.finish();
    let failure = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            failure["error_kind"].clone(),
            failure["message"].clone(),
        ),
        (
            Some(4),
            json!("parameter"),
            json!(
                "Polymarket Data API returned HTTP 400: bucket_seconds must be at least 600 for this interval"
            ),
        )
    );
}

#[test]
fn windows_the_data_api_would_refuse_fail_before_any_request() {
    let market = format!("polymarket:market/{OPEN}");
    let refusals = [
        &["--from", "2026-10-08", "--to", "2026-10-23"][..],
        &["--from", "2026-10-23", "--to", "2026-10-22"],
        &["--interval", "1m", "--bucket", "5m"],
        &["--interval", "max", "--bucket", "1m"],
        &["--interval", "1w", "--bucket", "1m"],
        &[
            "--interval",
            "1d",
            "--from",
            "2026-10-22",
            "--to",
            "2026-10-23",
        ],
        &["--from", "2026-10-22"],
    ]
    .map(|options| {
        let mut arguments = vec![market.as_str()];
        arguments.extend_from_slice(options);
        refused(&arguments)
    });

    assert_eq!(
        refusals.each_ref().map(|(code, _)| *code),
        [Some(2); 7],
        "{refusals:#?}"
    );
    assert_eq!(
        refusals[..5]
            .iter()
            .map(|(_, stderr)| stderr.trim().to_owned())
            .collect::<Vec<_>>(),
        [
            "argument_error: --from 2026-10-08 --to 2026-10-23 spans 16 days, and the Polymarket Data API serves at most 15; use --interval for a longer span",
            "argument_error: --from 2026-10-23 is after --to 2026-10-22",
            "argument_error: --bucket 5m is finer than the Polymarket Data API serves over --interval 1m; use 30m or coarser",
            "argument_error: --bucket 1m is finer than the Polymarket Data API serves over --interval max; use 30m or coarser",
            "argument_error: --bucket 1m is finer than the Polymarket Data API serves over --interval 1w; use 5m or coarser",
        ]
    );
}

#[test]
fn a_polymarket_us_link_is_refused_before_any_request() {
    let (code, message) = refused(&["https://polymarket.us/event/fedfnds-ondt-2027-12-31"]);

    assert_eq!(
        (
            code,
            message.contains("Polymarket US (polymarket.us) is a separate product")
        ),
        (Some(2), true),
        "{message}"
    );
}
