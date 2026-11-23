mod support;

use std::process::Output;

use serde_json::json;

use support::hn::{config, payload};
use support::{Fixture, Response, RunEnvironment};

fn stories(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "hn", "stories"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

#[test]
fn an_empty_list_is_an_empty_success_and_a_failed_list_fails() {
    let firebase = Fixture::start_sequence(vec![
        Response::json(200, "[]"),
        Response::json(503, "{}"),
        Response::json(200, "null"),
    ]);
    let algolia = Fixture::start_sequence(Vec::new());
    let environment = RunEnvironment::new(&config(&algolia.url, &firebase.url));

    let empty = stories(&environment, &["job"]);
    let failed = stories(&environment, &["new"]);
    let missing = stories(&environment, &["best"]);
    firebase.finish_all();

    assert_eq!(
        (
            empty.status.code(),
            payload(&empty)["items"].clone(),
            failed.status.code(),
            payload(&failed)["error_kind"].clone(),
            missing.status.code(),
            payload(&missing)["message"].clone(),
            algolia.finish_all().len(),
        ),
        (
            Some(0),
            json!([]),
            Some(4),
            json!("network"),
            Some(4),
            json!("HN Firebase API returned no best story list"),
            0,
        )
    );
}
