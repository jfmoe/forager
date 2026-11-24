//! `forager doctor --provider`: one provider's configuration, its prerequisites and, for a
//! provider that declares one, its free account check. It sends no other request and opens no
//! page. The browser routes run against a shell stand-in, so the module is Unix only.

use std::fmt::Write;

use serde_json::{Value, json};

use crate::support::browser::{StandIn, mark_logged_in};
use crate::support::samples::{sample, sample_json};
use crate::support::{Fixture, Response, Route, RunEnvironment, ok_json, path, payload};

/// The providers that declare an account check, with the check's name and the request it sends.
const ACCOUNT_CHECKS: &[(&str, &str, &str)] = &[
    ("serpapi", "account", "/account.json"),
    ("gemini_api", "models", "/models"),
];

fn names(value: &Value) -> impl Iterator<Item = &str> {
    value
        .as_object()
        .expect("JSON object")
        .keys()
        .map(String::as_str)
}

/// Configures every provider that `effective`, the default `config list` view, names: each URL
/// at `url`, a key wherever the provider reads keys, the browser stand-in, and every platform
/// order extended by the routes named after the platform, which enables the opt-in routes.
fn every_provider_config(effective: &Value, url: &str, stand_in: &StandIn) -> String {
    let mut config = stand_in.config();
    for provider in names(&effective["providers"]) {
        let _ = write!(config, "\n[providers.{provider}]\n");
        for field in names(&effective["providers"][provider]) {
            if field == "url" || field.ends_with("_url") {
                let _ = writeln!(config, "{field} = {url:?}");
            } else if field == "keys" {
                let _ = writeln!(config, "keys = [\"{provider}-fixture-key\"]");
            }
        }
    }
    for platform in names(&effective["platforms"]) {
        let mut order = effective["platforms"][platform]["order"]["value"]
            .as_array()
            .expect("platform order")
            .iter()
            .map(|route| route.as_str().expect("route name").to_owned())
            .collect::<Vec<_>>();
        order.extend(
            names(&effective["providers"])
                .filter(|provider| provider.starts_with(&format!("{platform}_")))
                .filter(|provider| !order.iter().any(|route| route == provider))
                .map(str::to_owned)
                .collect::<Vec<_>>(),
        );
        let _ = write!(config, "\n[platforms.{platform}]\norder = {order:?}\n");
    }
    config
}

#[test]
fn one_provider_doctor_sends_only_the_declared_account_requests_and_starts_no_browser() {
    let fixture = Fixture::start_routed(vec![
        Route::new(
            "GET",
            "/account.json",
            ok_json(&sample_json("serpapi/account.json")),
        ),
        Route::new(
            "GET",
            "/models",
            Response::json(200, &sample("gemini_api/models.json")),
        ),
    ]);
    let stand_in = StandIn::failing("must not start");
    let effective = payload(&RunEnvironment::new("").run(&["config", "list"]));
    let environment =
        RunEnvironment::new(&every_provider_config(&effective, &fixture.url, &stand_in));
    for platform in names(&effective["platforms"]) {
        mark_logged_in(&environment.state_dir, platform);
    }
    let offline = payload(&environment.run(&["doctor", "--offline"]));
    let providers = offline["providers"].as_array().expect("providers");
    let not_ready = providers
        .iter()
        .filter(|provider| provider["configured"] != true || provider["preflight"] == false)
        .collect::<Vec<_>>();
    assert!(
        not_ready.is_empty(),
        "the test configuration leaves these providers unable to run: {not_ready:#?}"
    );

    let reports = providers
        .iter()
        .map(|provider| {
            let name = provider["provider"].as_str().expect("provider name");
            let output = environment.run(&["doctor", "--provider", name]);
            let report = payload(&output);
            (
                name,
                output.status.code(),
                report["mode"].clone(),
                report["configured"].clone(),
                report["checks"].clone(),
            )
        })
        .collect::<Vec<_>>();
    let expected = providers
        .iter()
        .map(|provider| {
            let name = provider["provider"].as_str().expect("provider name");
            let checks = ACCOUNT_CHECKS
                .iter()
                .filter(|(declaring, _, _)| *declaring == name)
                .map(|(_, check, _)| json!({"name": check, "transport": "http", "ok": true}))
                .collect::<Vec<_>>();
            (
                name,
                Some(0),
                json!("provider"),
                json!(true),
                Value::Array(checks),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(reports, expected);
    assert_eq!(
        fixture
            .finish_all()
            .iter()
            .map(|request| path(request))
            .collect::<Vec<_>>(),
        ACCOUNT_CHECKS
            .iter()
            .map(|(_, _, request)| (*request).to_owned())
            .collect::<Vec<_>>()
    );
    assert!(stand_in.launches().is_empty(), "{:?}", stand_in.launches());
}
