mod support;

use serde_json::{Value, json};

use support::hn::{path, payload, stderr};
use support::polymarket::{config, json as body, last_trades, prices, run};
use support::polymarket_samples::{event, market, resolved, tokens, with};
use support::{Fixture, RunEnvironment};

const EVENT: &str = "how-many-fed-rate-cuts-in-2026";

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

fn none() -> Value {
    Value::Null
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
