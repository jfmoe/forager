use std::process::Output;

use serde_json::json;

use crate::support::samples::sample_json;
use crate::support::scholar::config;
use crate::support::{Fixture, Response, RunEnvironment, ok_json, payload, refs, stderr};

const KEY: &str = "test-serpapi-key-a";

fn search(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "scholar", "search"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

/// Runs one search against a fixture that answers in order and returns every request.
fn run(keys: &[&str], arguments: &[&str], responses: Vec<Response>) -> (Output, Vec<String>) {
    let fixture = Fixture::start_sequence(responses);
    let environment = RunEnvironment::new(&config(&fixture.url, keys));
    let output = search(&environment, arguments);
    (output, fixture.finish_all())
}

#[test]
fn a_search_prints_the_found_papers_by_their_cluster_refs() {
    let (output, _) = run(
        &[KEY],
        &["momentum"],
        vec![ok_json(&sample_json("serpapi/search.json"))],
    );
    let page = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &page["platform"],
            &page["provider"],
            refs(&page),
        ),
        (
            Some(0),
            &json!("scholar"),
            &json!("serpapi"),
            vec![
                "scholar:18208131694456651388".to_owned(),
                "scholar:10743434366743644145".to_owned(),
                "scholar:2175834747627124651".to_owned(),
                "scholar:15167737520660287132".to_owned(),
                "scholar:18111983945490529711".to_owned(),
                "scholar:5669475373525193992".to_owned(),
                "scholar:8468458679591681722".to_owned(),
                "scholar:12819834709253998113".to_owned(),
            ],
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn an_empty_key_pool_fails_before_any_request_and_names_the_keys_setting() {
    let fixture = Fixture::start_canary();
    let environment = RunEnvironment::new(&config(&fixture.url, &[]));

    let output = search(&environment, &["momentum"]);

    assert_eq!(
        (
            output.status.code(),
            stderr(&output).contains("providers.serpapi.keys"),
            fixture.finish_all().len()
        ),
        (Some(3), true, 0),
        "stderr: {}",
        stderr(&output)
    );
}
