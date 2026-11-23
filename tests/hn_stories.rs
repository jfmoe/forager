mod support;

use std::process::Output;

use serde_json::json;

use support::hn::{config, json as body, payload, stderr};
use support::hn_samples::story_list;
use support::{Fixture, RunEnvironment, path};

fn stories(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "hn", "stories"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

#[test]
fn an_empty_list_is_an_empty_success() {
    let firebase = Fixture::start_sequence(vec![body(&story_list(&[]))]);
    let algolia = Fixture::start_sequence(Vec::new());
    let environment = RunEnvironment::new(&config(&algolia.url, &firebase.url));

    let output = stories(&environment, &["job"]);
    let request = firebase.finish();

    assert_eq!(
        (
            output.status.code(),
            path(&request),
            payload(&output),
            algolia.finish_all().len(),
        ),
        (
            Some(0),
            "/v0/jobstories.json".to_owned(),
            json!({"platform": "hn", "provider": "hn_api", "list": "job", "items": []}),
            0,
        ),
        "stderr: {}",
        stderr(&output)
    );
}
