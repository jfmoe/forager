mod support;

use std::process::Output;

use serde_json::json;

use support::youtube::{payload, stderr};
use support::youtube_web::{config, player, watch_page};
use support::{Fixture, RunEnvironment};

fn fetch(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "youtube", "fetch", "youtube:jNQXAC9IVRw"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

#[test]
fn abstract_reads_the_page_and_the_player_only_and_inlines_the_description() {
    let youtube = Fixture::start_sequence_for(|origin| vec![watch_page(), player(origin)]);
    let environment = RunEnvironment::new(&config(&youtube.url));

    let output = fetch(&environment, &["--depth", "abstract"]);
    youtube.finish_all();
    let item = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &item["depth"],
            &item["description"],
            &item["chapters"],
            item.get("transcript"),
        ),
        (
            Some(0),
            &json!("abstract"),
            &json!("The first video on YouTube.\n\n00:00 Intro\n00:05 The cool thing\n00:17 End"),
            &json!([
                {"start_seconds": 0, "title": "Intro"},
                {"start_seconds": 5, "title": "The cool thing"},
                {"start_seconds": 17, "title": "End"}
            ]),
            None,
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn full_text_without_youtube_web_fails_before_any_request_and_names_the_order_that_adds_it() {
    let data_api = Fixture::start_canary();
    let environment = RunEnvironment::new(&format!(
        "[providers.youtube_api]\nurl = \"{}\"\nkeys = [\"yt-key\"]\n",
        data_api.url
    ));

    let default_depth = fetch(&environment, &[]);

    assert_eq!(
        (default_depth.status.code(), data_api.finish_all().len()),
        (Some(2), 0)
    );
    let message = stderr(&default_depth);
    assert!(
        message.contains("the Data API does not serve captions")
            && message.contains("never your account")
            && message.contains(
                r#"forager config set platforms.youtube.order '["youtube_api", "youtube_web", "gemini_api"]'"#
            ),
        "{message}"
    );
}
