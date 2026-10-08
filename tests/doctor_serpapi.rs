//! `forager doctor --provider serpapi`: the deep probe asks SerpApi's free Account API about
//! every key in the pool instead of running a billed search.

mod support;

use serde_json::{Value, json};

use support::scholar::{config, fixture, ok, payload, query_pairs, request_target};
use support::{Fixture, Response, RunEnvironment};

#[test]
fn deep_doctor_reports_the_quota_of_every_key_from_the_account_api() {
    let fixture = Fixture::start_sequence(vec![
        ok(&fixture("account.json")),
        ok(&fixture("account.json")),
    ]);
    let environment = RunEnvironment::new(&config(&fixture.url, &["first-key", "second-key"]));

    let output = environment.run(&["doctor", "--provider", "serpapi"]);
    let requests = fixture.finish_all();

    let quota = |key_index| {
        json!({
            "key_index": key_index,
            "ok": true,
            "searches_left": 219,
            "plan_searches_left": 219,
            "this_month_usage": 31,
            "this_hour_searches": 2,
            "hourly_limit": 250
        })
    };
    let payload = payload(&output);
    assert_eq!(
        (output.status.code(), &payload["ok"], &payload["keys"]),
        (Some(0), &Value::Bool(true), &json!([quota(0), quota(1)])),
    );
    assert_eq!(
        requests
            .iter()
            .map(|request| (
                request_target(request).path().to_owned(),
                query_pairs(request).into_iter().collect::<Vec<_>>()
            ))
            .collect::<Vec<_>>(),
        ["first-key", "second-key"].map(|key| (
            "/account.json".to_owned(),
            vec![("api_key".to_owned(), key.to_owned())]
        )),
    );
}

#[test]
fn an_invalid_key_fails_the_probe_while_the_other_keys_are_still_checked() {
    let fixture = Fixture::start_sequence(vec![
        Response::json(401, r#"{"error":"Invalid API key."}"#),
        ok(&fixture("account.json")),
    ]);
    let environment = RunEnvironment::new(&config(&fixture.url, &["revoked-key", "good-key"]));

    let output = environment.run(&["doctor", "--provider", "serpapi"]);
    let requests = fixture.finish_all();

    let payload = payload(&output);
    let keys = payload["keys"].as_array().expect("keys");
    assert_eq!(
        (
            output.status.code(),
            &payload["ok"],
            &payload["error_kind"],
            &payload["message"],
            &keys[0],
            &keys[1]["ok"],
            requests.len(),
        ),
        (
            Some(4),
            &Value::Bool(false),
            &json!("auth"),
            &json!("providers.serpapi.keys[0]: SerpApi returned HTTP 401: Invalid API key."),
            &json!({
                "key_index": 0,
                "ok": false,
                "searches_left": null,
                "plan_searches_left": null,
                "this_month_usage": null,
                "this_hour_searches": null,
                "hourly_limit": null,
                "error_kind": "auth"
            }),
            &Value::Bool(true),
            2,
        ),
    );
}

#[test]
fn a_key_without_searches_left_reports_its_quota_and_fails_as_exhausted() {
    let mut exhausted = fixture("account.json");
    exhausted["total_searches_left"] = json!(0);
    exhausted["plan_searches_left"] = json!(0);
    let fixture = Fixture::start(200, "application/json", &exhausted.to_string());
    let environment = RunEnvironment::new(&config(&fixture.url, &["spent-key"]));

    let output = environment.run(&["doctor", "--provider", "serpapi"]);
    fixture.finish();

    let payload = payload(&output);
    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            &payload["keys"]
        ),
        (
            Some(4),
            &json!("quota_exhausted"),
            &json!([{
                "key_index": 0,
                "ok": false,
                "searches_left": 0,
                "plan_searches_left": 0,
                "this_month_usage": 31,
                "this_hour_searches": 2,
                "hourly_limit": 250,
                "error_kind": "quota_exhausted"
            }]),
        ),
    );
}

#[test]
fn neither_the_keys_nor_the_account_identity_reach_output_or_trace_logs() {
    const KEYS: [&str; 3] = [
        "serpapi-canary-1a2b",
        "serpapi-canary-3c4d",
        "serpapi-canary-5e6f",
    ];
    let account = fixture("account.json");
    let mut busy = account.clone();
    busy["this_hour_searches"] = json!(250);
    let echo = format!(
        "{{\"error\":\"Invalid API key {key}, see ?api_key={key}\"}}",
        key = KEYS[0]
    );
    let partial = json!({"api_key": KEYS[2], "account_email": "fixture-owner@example.test"});
    let fixture =
        Fixture::start_sequence(vec![Response::json(401, &echo), ok(&busy), ok(&partial)]);
    let unreachable = "http://127.0.0.1:9";
    let mut secrets = KEYS.map(str::to_owned).to_vec();
    secrets.extend(
        ["api_key", "account_email", "account_id"]
            .map(|field| account[field].as_str().expect("identity field").to_owned()),
    );

    for (case, url, keys) in [
        ("account responses", fixture.url.as_str(), &KEYS[..]),
        ("network", unreachable, &KEYS[..1]),
    ] {
        let environment = RunEnvironment::new(&config(url, keys));
        let output = environment.run_with_env(
            &["doctor", "--provider", "serpapi"],
            &[("FORAGER_LOG__LEVEL", "trace")],
        );

        assert_eq!(output.status.code(), Some(4), "{case}");
        for (sink, contents) in [("stdout", &output.stdout), ("stderr", &output.stderr)] {
            let contents = String::from_utf8_lossy(contents);
            for secret in &secrets {
                assert!(!contents.contains(secret), "{case}: {sink} leaked {secret}");
            }
        }
    }
    assert_eq!(fixture.finish_all().len(), 3);
}

#[test]
fn deep_doctor_without_keys_fails_before_any_request() {
    let fixture = Fixture::start_canary();
    let environment = RunEnvironment::new(&config(&fixture.url, &[]));

    let output = environment.run(&["doctor", "--provider", "serpapi"]);

    let payload = payload(&output);
    assert_eq!(
        (
            output.status.code(),
            &payload["configured"],
            &payload["error_kind"],
            &payload["message"],
            payload.get("keys"),
            fixture.finish_all().len(),
        ),
        (
            Some(3),
            &Value::Bool(false),
            &json!("config"),
            &json!("providers.serpapi.keys has no configured credentials"),
            None,
            0,
        )
    );
}

#[test]
fn markdown_lists_one_line_per_key() {
    let fixture = Fixture::start_sequence(vec![
        ok(&fixture("account.json")),
        Response::json(401, r#"{"error":"Invalid API key."}"#),
    ]);
    let environment = RunEnvironment::new(&config(&fixture.url, &["good-key", "revoked-key"]));

    let output = environment.run(&["doctor", "--provider", "serpapi", "--format", "markdown"]);
    fixture.finish_all();

    let markdown = String::from_utf8(output.stdout).expect("UTF-8 markdown");
    let key_lines = markdown
        .lines()
        .filter(|line| line.starts_with("- key "))
        .collect::<Vec<_>>();
    assert_eq!(
        key_lines,
        [
            "- key 0: ok, searches_left=219, plan_searches_left=219, this_month_usage=31, \
             this_hour_searches=2, hourly_limit=250",
            "- key 1: auth",
        ],
        "{markdown}"
    );
}
