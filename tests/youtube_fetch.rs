mod support;

use serde_json::json;

use support::youtube::{
    KEY, config, header, pairs, path, payload, query, stderr, videos_page, zoo_video,
};
use support::{Fixture, RunEnvironment};

const CHAPTERED: &str =
    "The first video on YouTube.\n\n0:00 Elephants\n0:07 Trunks\n0:14 That's it";

fn fetch(environment: &RunEnvironment, arguments: &[&str]) -> std::process::Output {
    let mut command = vec!["platform", "youtube", "fetch"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

#[test]
fn metadata_reads_one_video_with_every_part_and_leaves_out_the_description() {
    let api = Fixture::start_sequence(vec![videos_page(&[zoo_video(CHAPTERED)])]);
    let environment = RunEnvironment::new(&config(&api.url));

    let output = fetch(
        &environment,
        &["https://youtu.be/jNQXAC9IVRw?t=3", "--depth", "metadata"],
    );
    let request = api.finish();

    assert_eq!(
        (
            output.status.code(),
            path(&request),
            query(&request),
            header(&request, "x-goog-api-key"),
            payload(&output),
        ),
        (
            Some(0),
            "/youtube/v3/videos".to_owned(),
            pairs(&[
                (
                    "part",
                    "snippet,contentDetails,statistics,status,liveStreamingDetails"
                ),
                ("id", "jNQXAC9IVRw"),
            ]),
            Some(KEY.to_owned()),
            json!({
                "platform": "youtube",
                "provider": "youtube_api",
                "ref": "youtube:jNQXAC9IVRw",
                "url": "https://www.youtube.com/watch?v=jNQXAC9IVRw",
                "depth": "metadata",
                "title": "Me at the zoo",
                "authors": ["jawed"],
                "published": "2005-04-24T03:31:52Z",
                "channel_ref": "youtube:channel/UC4QobU6STFB0P71PMvOGN5A",
                "snippet": null,
                "duration_seconds": 19,
                "live_status": "none",
                "views": 444_694_156,
                "likes": 20_083_234,
                "comments": 10_641_589,
                "language": "en",
                "has_captions": true,
                "caption_tracks": null
            }),
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn abstract_adds_the_full_description_and_its_chapters() {
    let api = Fixture::start_sequence(vec![videos_page(&[zoo_video(CHAPTERED)])]);
    let environment = RunEnvironment::new(&config(&api.url));

    let output = fetch(
        &environment,
        &["youtube:jNQXAC9IVRw", "--depth", "abstract"],
    );
    api.finish();
    let item = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &item["depth"],
            &item["description"],
            &item["chapters"],
        ),
        (
            Some(0),
            &json!("abstract"),
            &json!(CHAPTERED),
            &json!([
                {"start_seconds": 0, "title": "Elephants"},
                {"start_seconds": 7, "title": "Trunks"},
                {"start_seconds": 14, "title": "That's it"}
            ]),
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn content_format_prints_the_description() {
    let api = Fixture::start_sequence(vec![videos_page(&[zoo_video(CHAPTERED)])]);
    let environment = RunEnvironment::new(&config(&api.url));

    let output = fetch(
        &environment,
        &[
            "youtube:jNQXAC9IVRw",
            "--depth",
            "abstract",
            "--format",
            "content",
        ],
    );
    api.finish();

    assert_eq!(
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).trim_end()
        ),
        (Some(0), CHAPTERED),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_video_the_api_does_not_return_is_an_attempt_level_parameter_failure() {
    let api = Fixture::start_sequence(vec![videos_page(&[])]);
    let environment = RunEnvironment::new(&config(&api.url));

    let output = fetch(
        &environment,
        &["youtube:zzQq9ZxZ9Zq", "--depth", "metadata"],
    );
    api.finish();
    let failure = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &failure["error_kind"],
            &failure["message"]
        ),
        (
            Some(4),
            &json!("parameter"),
            &json!("YouTube video not found or not public: youtube:zzQq9ZxZ9Zq")
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn abstract_of_a_video_without_a_description_is_a_quality_failure() {
    let api = Fixture::start_sequence(vec![videos_page(&[zoo_video("  ")])]);
    let environment = RunEnvironment::new(&config(&api.url));

    let output = fetch(
        &environment,
        &["youtube:jNQXAC9IVRw", "--depth", "abstract"],
    );
    api.finish();

    assert_eq!(
        (output.status.code(), &payload(&output)["error_kind"]),
        (Some(5), &json!("quality")),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn full_text_collections_and_handles_fail_before_any_request() {
    let environment = RunEnvironment::new(&config("http://127.0.0.1:9"));

    let outputs = [
        fetch(&environment, &["youtube:jNQXAC9IVRw"]),
        fetch(
            &environment,
            &["https://www.youtube.com/playlist?list=PLZHQObOWTQDPD3MizzM2xVFitgF8hE_ab"],
        ),
        fetch(&environment, &["youtube:channel/UCYO_jab_esuFRV4b17AJtAw"]),
        fetch(&environment, &["https://www.youtube.com/@3blue1brown"]),
    ];

    assert_eq!(outputs.map(|output| output.status.code()), [Some(2); 4]);
}
