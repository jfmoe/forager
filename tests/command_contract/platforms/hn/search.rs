use serde_json::json;

use crate::support::hn::config;
use crate::support::hn_samples::stored;
use crate::support::{Fixture, Response, RunEnvironment, payload, refs};

fn search(environment: &RunEnvironment, arguments: &[&str]) -> std::process::Output {
    let mut command = vec!["platform", "hn", "search"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

#[test]
fn a_search_prints_the_found_stories_and_comments_as_a_platform_page() {
    let algolia = Fixture::start_sequence(vec![Response::json(200, &stored("search.json"))]);
    let environment = RunEnvironment::new(&config(&algolia.url, "http://127.0.0.1:9"));

    let output = search(&environment, &["dropbox"]);
    algolia.finish();
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
            &json!("hn"),
            &json!("hn_api"),
            vec!["hn:8863".to_owned(), "hn:8952".to_owned()],
        )
    );
}
