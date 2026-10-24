//! Live smoke of the Polymarket cases: the search, the resolved canary, the live-quote fetch of
//! the event the search found, and the history of the resolved canary market.

mod support;

use serde_json::{Value, json};
use support::hn::path;
use support::polymarket::{
    event, json as body, last_trades, market, price_page, prices, resolved, tokens,
};
use support::smoke::{DISABLED_PLATFORMS, SmokeEnvironment, case};
use support::{Fixture, Response};

const WINNER: &str = "will-donald-trump-win-the-2024-us-presidential-election";

// Debug path formatting supplies the quoted and escaped TOML string literal required by fixtures.
#[expect(clippy::unnecessary_debug_formatting)]
fn environment(gamma: &str, clob: &str, data: &str) -> SmokeEnvironment {
    // The shared list disables Polymarket too; the later table re-enables its route.
    let disabled = DISABLED_PLATFORMS.replace("[platforms.polymarket]\norder = []\n", "");
    SmokeEnvironment::new(|journal_dir| {
        format!(
            "[providers.polymarket_api]\nurl = \"{gamma}\"\nclob_url = \"{clob}\"\ndata_url = \"{data}\"\n[journal]\ndir = {journal_dir:?}\n{disabled}"
        )
    })
}

fn statuses(payload: &Value) -> [Value; 4] {
    ["C43", "C44", "C45", "C46"].map(|id| case(payload, id)["status"].clone())
}

#[test]
fn live_smoke_reads_the_canary_without_the_clob_the_found_event_with_it_and_the_canary_history() {
    let found = "how-many-fed-rate-cuts-in-2026";
    let gamma = Fixture::start_sequence(vec![
        body(&json!({
            "events": [event(found, &[market("two-sided", "Two-sided")])],
            "pagination": {"hasMore": false, "totalResults": 1}
        })),
        body(&event(
            "presidential-election-winner-2024",
            &[resolved(WINNER, "Donald Trump", true)],
        )),
        body(&event(found, &[market("two-sided", "Two-sided")])),
        body(&resolved(WINNER, "Donald Trump", true)),
    ]);
    let [yes, no] = tokens("two-sided");
    let clob = Fixture::start_sequence(vec![
        prices(&[(&yes, "0.961", "0.963"), (&no, "0.037", "0.039")]),
        last_trades(&[(&yes, "0.962")]),
    ]);
    let data = Fixture::start_sequence(vec![price_page(
        &[(1_730_851_200, 0.58, 86_400), (1_730_937_600, 1.0, 0)],
        None,
    )]);
    let environment = environment(&gamma.url, &clob.url, &data.url);

    let output = environment.run(&["smoke", "--live", "--timeout", "20"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse live smoke JSON");
    let gamma_requests = gamma.finish_all();
    let clob_requests = clob.finish_all();
    let data_request = data.finish();

    assert_eq!(
        (
            statuses(&payload),
            gamma_requests
                .iter()
                .map(|request| path(request))
                .collect::<Vec<_>>(),
            clob_requests
                .iter()
                .map(|request| path(request))
                .collect::<Vec<_>>(),
            path(&data_request),
        ),
        (
            [(); 4].map(|()| Value::String("passed".into())),
            vec![
                "/public-search".to_owned(),
                "/events/slug/presidential-election-winner-2024".to_owned(),
                format!("/events/slug/{found}"),
                format!("/markets/slug/{WINNER}"),
            ],
            vec!["/prices".to_owned(), "/last-trades-prices".to_owned()],
            "/v2/prices-history".to_owned(),
        ),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn the_live_quote_case_does_not_start_without_an_event_from_the_search_case() {
    let gamma = Fixture::start_repeating(Response::json(500, r#"{"error":"down"}"#));
    let environment = environment(&gamma.url, "http://127.0.0.1:9", "http://127.0.0.1:9");

    let output = environment.run(&["smoke", "--live", "--timeout", "20"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse live smoke JSON");
    gamma.finish_all();

    assert_eq!(
        (statuses(&payload), case(&payload, "C45")["message"].clone()),
        (
            ["failed", "failed", "skipped", "failed"].map(|status| Value::String(status.into())),
            json!("not started: the Polymarket search case returned no event to fetch"),
        ),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}
