mod gemini_api;
mod health;
mod live;
mod offline;
#[cfg(unix)]
mod one_provider;
mod platform_routes;
mod serpapi;

use serde_json::{Value, json};

use std::fs;

use crate::support::doctor::{SHALLOW_PROVIDERS, reachable_responses, shallow_config};
use crate::support::{Fixture, RunEnvironment, object_keys};

/// The providers of a doctor report that `field` marks true.
fn providers_where<'a>(payload: &'a Value, field: &str) -> Vec<&'a str> {
    payload["providers"]
        .as_array()
        .expect("providers")
        .iter()
        .filter(|provider| provider[field] == Value::Bool(true))
        .map(|provider| provider["provider"].as_str().expect("provider name"))
        .collect()
}

#[test]
fn shallow_doctor_reports_all_registry_providers_and_reuses_the_config_list_view() {
    let fixture = Fixture::start_sequence(reachable_responses(SHALLOW_PROVIDERS.len()));
    let environment = RunEnvironment::new(&shallow_config(&format!(
        "{}?token=url-secret",
        fixture.url
    )));

    let doctor = environment.run(&["doctor"]);
    let config_list = environment.run(&["config", "list"]);
    let payload: Value = serde_json::from_slice(&doctor.stdout).expect("parse doctor JSON");
    let config: Value = serde_json::from_slice(&config_list.stdout).expect("parse config JSON");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&doctor.stdout),
        String::from_utf8_lossy(&doctor.stderr)
    );

    assert_eq!(
        (
            doctor.status.code(),
            &payload["mode"],
            &payload["ok"],
            payload["providers"]
                .as_array()
                .expect("providers")
                .iter()
                .map(|provider| provider["provider"].as_str().expect("provider name"))
                .collect::<std::collections::BTreeSet<_>>(),
            &payload["config"],
            providers_where(&payload, "reachable")
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>(),
            &payload["providers"][0]["provider"],
            &payload["providers"][0]["configured"],
            &payload["providers"][0]["key_count"],
            &payload["providers"][0]["source"],
        ),
        (
            Some(0),
            &Value::String("shallow".into()),
            &Value::Bool(true),
            object_keys(&config["providers"]).into_iter().collect(),
            &config,
            SHALLOW_PROVIDERS
                .iter()
                .map(|(provider, _)| *provider)
                .collect(),
            &Value::String("xai".into()),
            &Value::Bool(true),
            &Value::from(1),
            &Value::String("file".into()),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&doctor.stderr)
    );
    for secret in [
        "xai-secret",
        "exa-secret",
        "tavily-secret",
        "firecrawl-secret",
        "jina-secret",
        "url-secret",
    ] {
        assert!(!combined.contains(secret), "doctor leaked {secret}");
    }
    assert!(
        payload["permission_warnings"]
            .as_array()
            .is_some_and(|warnings| !warnings.is_empty())
    );
    fixture.finish_all();
}

#[test]
fn doctor_provider_rejects_values_outside_the_compiled_registry() {
    let environment = RunEnvironment::new("");

    let output = environment.run(&["doctor", "--provider", "not_a_provider"]);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("invalid provider"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn one_provider_doctor_reports_missing_credentials_as_a_json_config_failure() {
    let environment = RunEnvironment::new("");

    let output = environment.run(&["doctor", "--provider", "exa"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");

    assert_eq!(
        (
            output.status.code(),
            &payload["mode"],
            &payload["ok"],
            &payload["configured"],
            &payload["error_kind"],
            &payload["message"],
        ),
        (
            Some(3),
            &json!("provider"),
            &Value::Bool(false),
            &Value::Bool(false),
            &json!("config"),
            &json!("providers.exa.keys has no configured credentials"),
        )
    );
}

#[test]
fn doctor_markdown_preserves_the_json_status_and_effective_configuration() {
    let environment = RunEnvironment::new(&shallow_config("http://127.0.0.1:9"));

    let json_output = environment.run(&["doctor", "--offline"]);
    let markdown_output = environment.run(&["doctor", "--offline", "--format", "markdown"]);
    let payload: Value = serde_json::from_slice(&json_output.stdout).expect("parse doctor JSON");
    let markdown = String::from_utf8(markdown_output.stdout).expect("UTF-8 markdown");
    let config = serde_json::to_string_pretty(&payload["config"]).expect("render config");

    assert_eq!(
        (json_output.status.code(), markdown_output.status.code()),
        (Some(0), Some(0))
    );
    assert!(markdown.contains("# forager doctor"), "{markdown}");
    assert!(markdown.contains("ok: true"), "{markdown}");
    assert!(
        markdown.contains("exa: configured=true, key_count=1, source=file"),
        "{markdown}"
    );
    assert!(
        markdown.contains(&format!(
            "## Effective configuration\n\n```json\n{config}\n```"
        )),
        "{markdown}"
    );
    assert!(!markdown.contains("exa-secret"));
}

#[test]
fn shallow_doctor_probes_reachability_with_get_requests() {
    let fixture = Fixture::start_sequence(reachable_responses(SHALLOW_PROVIDERS.len()));
    let environment = RunEnvironment::new(&shallow_config(&fixture.url));

    let doctor = environment.run(&["doctor"]);
    let methods = fixture
        .finish_all()
        .iter()
        .map(|request| {
            request
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_owned()
        })
        .collect::<Vec<_>>();

    assert_eq!(
        (doctor.status.code(), methods),
        (Some(0), vec!["GET".to_owned(); SHALLOW_PROVIDERS.len()]),
        "stderr: {}",
        String::from_utf8_lossy(&doctor.stderr)
    );
}

#[test]
fn shallow_doctor_reports_a_well_formed_but_dead_endpoint_as_unreachable() {
    let fixture = Fixture::start_sequence(reachable_responses(2 * (SHALLOW_PROVIDERS.len() - 1)));
    let config = shallow_config(&fixture.url).replace(
        &format!("[providers.xai]\nurl = {:?}", fixture.url),
        "[providers.xai]\nurl = \"http://127.0.0.1:9\"",
    );
    let environment = RunEnvironment::new(&config);
    let markdown_environment = RunEnvironment::new(&config);

    let output = environment.run(&["doctor", "--timeout", "2"]);
    let markdown_output =
        markdown_environment.run(&["doctor", "--timeout", "2", "--format", "markdown"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");
    let markdown = String::from_utf8(markdown_output.stdout).expect("UTF-8 markdown");

    assert_eq!(
        (
            output.status.code(),
            &payload["ok"],
            &payload["providers"][0]["provider"],
            &payload["providers"][0]["reachable"],
            markdown_output.status.code(),
        ),
        (
            Some(4),
            &Value::Bool(false),
            &Value::String("xai".into()),
            &Value::Bool(false),
            Some(4),
        )
    );
    assert!(markdown.contains("ok: false"), "{markdown}");
    fixture.finish_all();
}

#[test]
fn shallow_doctor_ignores_an_unconfigured_unreachable_provider() {
    let fixture = Fixture::start_sequence(reachable_responses(SHALLOW_PROVIDERS.len() - 1));
    let config = shallow_config(&fixture.url).replace(
        &format!(
            "[providers.xai]\nurl = {:?}\nkeys = [\"xai-secret\"]",
            fixture.url
        ),
        "[providers.xai]\nurl = \"http://127.0.0.1:9\"\nkeys = []",
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["doctor", "--timeout", "2"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");

    assert_eq!(
        (
            output.status.code(),
            &payload["ok"],
            &payload["providers"][0]["configured"],
            &payload["providers"][0]["reachable"],
        ),
        (
            Some(0),
            &Value::Bool(true),
            &Value::Bool(false),
            &Value::Bool(false),
        )
    );
    fixture.finish_all();
}

#[test]
fn shallow_doctor_treats_providers_that_need_missing_keys_as_unconfigured_and_healthy() {
    let fixture = Fixture::start_sequence(reachable_responses(SHALLOW_PROVIDERS.len()));
    let config = SHALLOW_PROVIDERS
        .iter()
        .filter_map(|(_, key)| *key)
        .fold(shallow_config(&fixture.url), |config, secret| {
            config.replace(&format!("keys = [\"{secret}\"]"), "keys = []")
        });
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["doctor"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");

    assert_eq!(
        (
            output.status.code(),
            &payload["ok"],
            providers_where(&payload, "configured"),
        ),
        (
            Some(0),
            &Value::Bool(true),
            vec![
                "jina",
                "arxiv_api",
                "ssrn_crossref",
                "hn_api",
                "polymarket_api"
            ]
        )
    );
    fixture.finish_all();
}

#[test]
fn doctor_rejects_bad_configuration_before_any_probe() {
    let environment = RunEnvironment::new("[providers.exa]\nunknown = true\n");

    let output = environment.run(&["doctor", "--provider", "exa"]);

    assert_eq!(output.status.code(), Some(3));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unknown"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn shallow_doctor_reports_jina_without_keys_as_configured_and_anonymous() {
    let fixture = Fixture::start_sequence(reachable_responses(SHALLOW_PROVIDERS.len()));
    let config = shallow_config(&fixture.url).replace("keys = [\"jina-secret\"]", "keys = []");
    let environment = RunEnvironment::new(&config);

    let doctor = environment.run(&["doctor"]);
    let payload: Value = serde_json::from_slice(&doctor.stdout).expect("parse doctor JSON");
    let jina = payload["providers"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|provider| provider["provider"] == "jina")
        .cloned()
        .unwrap_or_default();

    assert_eq!(
        (
            doctor.status.code(),
            &jina["configured"],
            &jina["key_count"],
            jina["message"]
                .as_str()
                .is_some_and(|message| message.contains("anonymously")),
        ),
        (Some(0), &Value::Bool(true), &Value::from(0), true),
        "{jina}\nstderr: {}",
        String::from_utf8_lossy(&doctor.stderr)
    );
    fixture.finish_all();
}

#[test]
fn the_default_doctor_reports_and_requires_the_state_directories_as_offline_doctor_does() {
    let fixture = Fixture::start_sequence(reachable_responses(SHALLOW_PROVIDERS.len()));
    let root = tempfile::tempdir().expect("create journal root");
    let blocked = root.path().join("journal");
    fs::write(&blocked, "not a directory").expect("block the journal directory");
    let environment = RunEnvironment::new(&format!(
        "{}[journal]\ndir = {:?}\n",
        shallow_config(&fixture.url),
        blocked.to_str().expect("UTF-8 path")
    ));

    let outputs = [&["doctor"][..], &["doctor", "--offline"]].map(|arguments| {
        let output = environment.run(arguments);
        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");
        (
            output.status.code(),
            payload["ok"].clone(),
            payload["state_directories"].clone(),
        )
    });

    assert_eq!(outputs[0], outputs[1]);
    assert_eq!(
        (
            &outputs[0].0,
            &outputs[0].1,
            &outputs[0].2["journal"]["writable"]
        ),
        (&Some(4), &json!(false), &json!(false)),
        "{:?}",
        outputs[0]
    );
    fixture.finish_all();
}
