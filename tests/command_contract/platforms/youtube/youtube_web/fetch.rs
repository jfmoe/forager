use std::process::Output;

use serde_json::json;

use crate::support::youtube_web::{config, player, watch_page};
use crate::support::{Fixture, RunEnvironment, payload, stderr};

fn fetch(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "youtube", "fetch", "youtube:jNQXAC9IVRw"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

#[test]
fn an_abstract_fetch_prints_the_video_read_from_its_page_and_player() {
    let youtube = Fixture::start_sequence_for(|origin| vec![watch_page(), player(origin)]);
    let environment = RunEnvironment::new(&config(&youtube.url));

    let output = fetch(&environment, &["--depth", "abstract"]);
    let requests = youtube.finish_all();
    let item = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            requests.len(),
            [&item["provider"], &item["ref"], &item["depth"]],
        ),
        (
            Some(0),
            2,
            [
                &json!("youtube_web"),
                &json!("youtube:jNQXAC9IVRw"),
                &json!("abstract"),
            ],
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
