mod support;

use serde_json::json;

use support::youtube::{KEY, config, pairs, payload, search_page, stderr};
use support::{Fixture, RunEnvironment};
use support::{header, path, query};

fn search(environment: &RunEnvironment, arguments: &[&str]) -> std::process::Output {
    let mut command = vec!["platform", "youtube", "search"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

#[test]
fn options_map_to_data_api_parameters_and_the_key_travels_only_in_a_header() {
    let api = Fixture::start_sequence(vec![search_page(&[], None)]);
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

    assert_eq!(
        (
            output.status.code(),
            path(&request),
            query(&request),
            header(&request, "x-goog-api-key"),
            request.contains(&format!("key={KEY}")),
            payload(&output),
        ),
        (
            Some(0),
            "/youtube/v3/search".to_owned(),
            pairs(&[
                ("part", "snippet"),
                ("type", "video"),
                ("q", "attention -music"),
                ("maxResults", "10"),
                ("order", "viewCount"),
                ("publishedAfter", "2024-01-01T00:00:00Z"),
                ("publishedBefore", "2024-01-31T23:59:59Z"),
                ("videoDuration", "long"),
                ("channelId", "UCExampleMathsChannel001"),
                ("videoCaption", "closedCaption"),
            ]),
            Some(KEY.to_owned()),
            false,
            json!({"platform": "youtube", "provider": "youtube_api", "items": [], "next_cursor": null}),
        ),
        "stderr: {}",
        stderr(&output)
    );
}
