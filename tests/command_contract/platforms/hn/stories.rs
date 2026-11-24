use std::process::Output;

use serde_json::json;

use crate::support::hn::config;
use crate::support::hn_samples::story_list;
use crate::support::{Fixture, RunEnvironment, ok_json, payload, stderr};

fn stories(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "hn", "stories"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

#[test]
fn an_empty_list_is_an_empty_success() {
    let firebase = Fixture::start_sequence(vec![ok_json(&story_list(&[]))]);
    let algolia = Fixture::start_sequence(Vec::new());
    let environment = RunEnvironment::new(&config(&algolia.url, &firebase.url));

    let output = stories(&environment, &["job"]);
    firebase.finish();

    assert_eq!(
        (
            output.status.code(),
            payload(&output),
            algolia.finish_all().len(),
        ),
        (
            Some(0),
            json!({"platform": "hn", "provider": "hn_api", "list": "job", "items": []}),
            0,
        ),
        "stderr: {}",
        stderr(&output)
    );
}
