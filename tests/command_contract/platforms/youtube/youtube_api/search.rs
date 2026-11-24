use serde_json::json;

use crate::support::header;
use crate::support::youtube::{KEY, config};
use crate::support::youtube_api_samples::search_page;
use crate::support::{Fixture, RunEnvironment, ok_json, payload, stderr};

fn search(environment: &RunEnvironment, arguments: &[&str]) -> std::process::Output {
    let mut command = vec!["platform", "youtube", "search"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

#[test]
fn a_search_sends_the_configured_key_only_in_a_header() {
    let api = Fixture::start_sequence(vec![ok_json(&search_page(&[], None))]);
    let environment = RunEnvironment::new(&config(&api.url));

    let output = search(
        &environment,
        &[
            "attention -music",
            "--sort",
            "views",
            "--from",
            "2024-01-01",
            "--to",
            "2024-01-31",
            "--duration",
            "long",
            "--channel",
            "https://www.youtube.com/channel/UCExampleMathsChannel001",
            "--has-captions",
            "--limit",
            "10",
        ],
    );
    let request = api.finish();
    let page = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            header(&request, "x-goog-api-key"),
            request.contains(&format!("key={KEY}")),
            &page["platform"],
            &page["provider"],
        ),
        (
            Some(0),
            Some(KEY.to_owned()),
            false,
            &json!("youtube"),
            &json!("youtube_api"),
        ),
        "stderr: {}",
        stderr(&output)
    );
}
