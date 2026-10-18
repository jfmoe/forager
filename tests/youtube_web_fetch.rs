mod support;

use std::fs;
use std::path::Path;
use std::process::Output;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use support::youtube::{header, pairs, path, payload, query, stderr};
use support::youtube_web::{
    INNERTUBE_KEY, ZOO, ZOO_DESCRIPTION, ZOO_SRV1_EN, bot_check_page, config,
    json as json_response, refusal, srv1, track, watch_page, zoo_details, zoo_embedded, zoo_player,
};
use support::{Fixture, Response, RunEnvironment, request_json};

/// The zoo transcript as `youtube_web` renders its English publisher track.
const ZOO_TRANSCRIPT: &str = "# Me at the zoo

- Ref: `youtube:jNQXAC9IVRw`
- Channel: jawed
- Published: 2005-04-24T03:31:52Z
- Duration: 00:19
- Transcript: `en`, captions the publisher uploaded

> This document holds only caption text; nothing here describes the video's picture or its non-speech sounds.

## Description

The first video on YouTube.

00:00 Intro
00:05 The cool thing
00:17 End

## Transcript

### Intro

[00:01](https://www.youtube.com/watch?v=jNQXAC9IVRw&t=1s) All right, so here we are, in front of the elephants

### The cool thing

[00:05](https://www.youtube.com/watch?v=jNQXAC9IVRw&t=5s) the cool thing about these guys is that they have really... really really long trunks and that's cool (baaaaaaaaaaahhh!!) and that's pretty much all there is to say
";

fn fetch(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "youtube", "fetch", "youtube:jNQXAC9IVRw"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

/// The three responses of a full-text fetch of the zoo video with English and German
/// publisher tracks.
fn zoo_full_text(origin: &str, xml: &str) -> Vec<Response> {
    vec![
        watch_page(&zoo_embedded(ZOO_DESCRIPTION)),
        json_response(&zoo_player(
            &[track(origin, "en", false), track(origin, "de", false)],
            0,
        )),
        srv1(xml),
    ]
}

/// The stdout of a full-text zoo fetch whose transcript went to `content_dir`, without its
/// provider attempts.
fn zoo_full_text_result(content_dir: &Path) -> Value {
    json!({
        "platform": "youtube",
        "provider": "youtube_web",
        "ref": "youtube:jNQXAC9IVRw",
        "url": "https://www.youtube.com/watch?v=jNQXAC9IVRw",
        "depth": "full_text",
        "title": "Me at the zoo",
        "authors": ["jawed"],
        "published": "2005-04-24T03:31:52Z",
        "channel_ref": "youtube:channel/UC4QobU6STFB0P71PMvOGN5A",
        "snippet": null,
        "duration_seconds": 19,
        "live_status": "none",
        "views": 444_721_399,
        "likes": null,
        "comments": null,
        "language": null,
        "has_captions": true,
        "caption_tracks": [
            {"language": "en", "source": "manual"},
            {"language": "de", "source": "manual"}
        ],
        "transcript": {"language": "en", "source": "manual", "cues": 6, "paragraphs": 2},
        "content_url": "https://www.youtube.com/watch?v=jNQXAC9IVRw",
        "content_provider": "youtube_web",
        "content_path": content_dir.join("youtube-jNQXAC9IVRw.md").to_str(),
        "content_len": ZOO_TRANSCRIPT.chars().count(),
        "provider_attempts": null
    })
}

/// The method and path of a recorded request.
fn route(request: &str) -> String {
    let method = request.split(' ').next().unwrap_or_default();
    format!("{method} {}", path(request))
}

/// Takes the provider attempts out of `result` as (provider, disposition) pairs.
fn take_attempts(result: &mut Value) -> Vec<(Value, Value)> {
    let attempts = result["provider_attempts"]
        .as_array()
        .map(|attempts| {
            attempts
                .iter()
                .map(|attempt| (attempt["provider"].clone(), attempt["disposition"].clone()))
                .collect()
        })
        .unwrap_or_default();
    result["provider_attempts"] = Value::Null;
    attempts
}

fn failure(output: &Output) -> (Option<i32>, Value, String) {
    let failure = payload(output);
    (
        output.status.code(),
        failure["error_kind"].clone(),
        failure["message"].as_str().unwrap_or_default().to_owned(),
    )
}

#[test]
fn full_text_reads_the_page_the_player_and_one_track_and_writes_the_transcript() {
    let youtube = Fixture::start_sequence_for(|origin| zoo_full_text(origin, ZOO_SRV1_EN));
    let data_api = Fixture::start_canary();
    let environment = RunEnvironment::new(&format!(
        "[providers.youtube_api]\nurl = \"{}\"\nkeys = [\"yt-key\"]\n\n[providers.youtube_web]\nurl = \"{}\"\n\n[platforms.youtube]\norder = [\"youtube_api\", \"youtube_web\"]\n",
        data_api.url, youtube.url
    ));
    let content_dir = tempfile::tempdir().expect("content directory");

    let started = Instant::now();
    let output = fetch(
        &environment,
        &[
            "--content-dir",
            content_dir.path().to_str().expect("UTF-8 path"),
            "--verbose",
        ],
    );
    let elapsed = started.elapsed();
    let requests = youtube.finish_all();
    let mut result = payload(&output);
    let written =
        fs::read_to_string(result["content_path"].as_str().unwrap_or_default()).unwrap_or_default();
    let attempts = take_attempts(&mut result);

    assert_eq!(
        (
            output.status.code(),
            requests
                .iter()
                .map(|request| route(request))
                .collect::<Vec<_>>(),
            query(&requests[0]),
            query(&requests[1]),
            request_json(&requests[1]),
            query(&requests[2]),
            requests
                .iter()
                .map(|request| header(request, "accept-language"))
                .collect::<Vec<_>>(),
            data_api.finish_all().len(),
        ),
        (
            Some(0),
            vec![
                "GET /watch".to_owned(),
                "POST /youtubei/v1/player".to_owned(),
                "GET /api/timedtext".to_owned()
            ],
            pairs(&[("v", ZOO)]),
            pairs(&[("key", INNERTUBE_KEY)]),
            json!({
                "context": {"client": {"clientName": "ANDROID", "clientVersion": "20.10.38"}},
                "videoId": ZOO
            }),
            pairs(&[
                ("v", ZOO),
                ("ei", "fixture"),
                ("xoaf", "5"),
                ("hl", "en"),
                ("lang", "en")
            ]),
            vec![Some("en-US".to_owned()); 3],
            0,
        ),
        "stderr: {}",
        stderr(&output)
    );
    assert_eq!(written, ZOO_TRANSCRIPT);
    assert_eq!(result, zoo_full_text_result(content_dir.path()));
    assert_eq!(
        attempts,
        [
            (json!("youtube_api"), json!("skipped")),
            (json!("youtube_web"), json!("succeeded")),
            (json!("youtube_web"), json!("succeeded")),
            (json!("youtube_web"), json!("succeeded")),
        ]
    );
    // Every request waits for the route's four-second window.
    assert!(elapsed >= Duration::from_secs(8), "{elapsed:?}");
}

#[test]
fn lang_picks_a_publisher_track_and_content_format_prints_its_transcript() {
    let youtube = Fixture::start_sequence_for(|origin| {
        let mut responses = zoo_full_text(
            origin,
            r#"<transcript><text start="1.2" dur="2">Also gut, hier sind wir.</text></transcript>"#,
        );
        responses[1] = json_response(&zoo_player(
            &[
                track(origin, "en", true),
                track(origin, "en", false),
                track(origin, "de", false),
            ],
            1,
        ));
        responses
    });
    let environment = RunEnvironment::new(&config(&youtube.url));

    let output = fetch(
        &environment,
        &["--lang", "fr", "--lang", "de", "--format", "content"],
    );
    let requests = youtube.finish_all();
    let printed = String::from_utf8_lossy(&output.stdout);

    assert_eq!(
        (
            output.status.code(),
            query(&requests[2]).get("lang").cloned(),
            printed.contains("- Transcript: `de`, captions the publisher uploaded\n"),
            printed.trim_end().ends_with(
                "[00:01](https://www.youtube.com/watch?v=jNQXAC9IVRw&t=1s) Also gut, hier sind wir."
            ),
        ),
        (Some(0), Some("de".to_owned()), true, true),
        "stdout: {printed}\nstderr: {}",
        stderr(&output)
    );
}

#[test]
fn abstract_reads_the_page_and_the_player_only_and_inlines_the_description() {
    let youtube = Fixture::start_sequence_for(|origin| {
        zoo_full_text(origin, ZOO_SRV1_EN)
            .into_iter()
            .take(2)
            .collect()
    });
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
            &json!(ZOO_DESCRIPTION),
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
fn full_text_without_youtube_web_and_lang_outside_full_text_fail_before_any_request() {
    let data_api = Fixture::start_canary();
    let environment = RunEnvironment::new(&format!(
        "[providers.youtube_api]\nurl = \"{}\"\nkeys = [\"yt-key\"]\n",
        data_api.url
    ));

    let default_depth = fetch(&environment, &[]);
    let lang_with_metadata = fetch(&environment, &["--depth", "metadata", "--lang", "en"]);
    let invalid_lang = fetch(&environment, &["--lang", "english"]);

    assert_eq!(
        (
            default_depth.status.code(),
            lang_with_metadata.status.code(),
            invalid_lang.status.code(),
            data_api.finish_all().len(),
        ),
        (Some(2), Some(2), Some(2), 0)
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

#[test]
fn a_rate_limited_watch_page_is_rate_limited_and_names_the_egress_ip() {
    let youtube = Fixture::start_sequence(vec![Response::new(
        429,
        "text/html",
        "<html>Too many</html>",
    )]);
    let environment = RunEnvironment::new(&config(&youtube.url));

    let output = fetch(&environment, &["--depth", "metadata"]);
    youtube.finish_all();
    let (code, kind, message) = failure(&output);

    assert_eq!((code, kind), (Some(4), json!("rate_limited")), "{message}");
    assert!(message.contains("egress IP"), "{message}");
}

#[test]
fn a_bot_check_page_without_a_key_is_an_auth_failure() {
    let youtube = Fixture::start_sequence(vec![bot_check_page()]);
    let environment = RunEnvironment::new(&config(&youtube.url));

    let output = fetch(&environment, &["--depth", "metadata"]);
    youtube.finish_all();
    let (code, kind, message) = failure(&output);

    assert_eq!((code, kind), (Some(4), json!("auth")), "{message}");
    assert!(message.contains("egress IP"), "{message}");
}

/// Runs a metadata or full-text fetch whose player answers `player`; only an accepted answer
/// at full text leads to a third request.
fn player_failure(player: &Value, depth: &str) -> (Option<i32>, Value, String) {
    let youtube = Fixture::start_sequence(vec![
        watch_page(&zoo_embedded(ZOO_DESCRIPTION)),
        json_response(player),
    ]);
    let environment = RunEnvironment::new(&config(&youtube.url));
    let output = fetch(&environment, &["--depth", depth]);
    youtube.finish_all();
    failure(&output)
}

#[test]
fn a_missing_video_is_an_attempt_level_parameter_failure() {
    let (code, kind, message) =
        player_failure(&refusal("ERROR", "This video is unavailable"), "metadata");

    assert_eq!(
        (code, kind, message),
        (
            Some(4),
            json!("parameter"),
            "YouTube video not found or not playable: youtube:jNQXAC9IVRw (This video is unavailable)".to_owned()
        )
    );
}

#[test]
fn a_bot_check_of_the_player_is_an_auth_failure_about_the_egress_ip() {
    let (code, kind, message) = player_failure(
        &refusal("LOGIN_REQUIRED", "Sign in to confirm you’re not a bot"),
        "metadata",
    );

    assert_eq!((code, kind), (Some(4), json!("auth")), "{message}");
    assert!(message.contains("egress IP"), "{message}");
}

#[test]
fn a_player_answer_for_another_video_is_a_runtime_failure() {
    let mut player = zoo_player(&[], 0);
    player["videoDetails"] = zoo_details(ZOO_DESCRIPTION);
    player["videoDetails"]["videoId"] = json!("dQw4w9WgXcQ");

    let (code, kind, message) = player_failure(&player, "metadata");

    assert_eq!((code, kind), (Some(4), json!("runtime")), "{message}");
    assert!(message.contains("dQw4w9WgXcQ"), "{message}");
}

#[test]
fn a_live_stream_has_no_full_text_yet() {
    let mut embedded = zoo_embedded(ZOO_DESCRIPTION);
    embedded["microformat"]["playerMicroformatRenderer"]["liveBroadcastDetails"] =
        json!({"isLiveNow": true, "startTimestamp": "2026-10-17T10:00:00+00:00"});
    let youtube = Fixture::start_sequence(vec![
        watch_page(&embedded),
        json_response(&zoo_player(&[track("http://127.0.0.1:9", "en", true)], 0)),
    ]);
    let environment = RunEnvironment::new(&config(&youtube.url));

    let output = fetch(&environment, &[]);
    youtube.finish_all();
    let (code, kind, message) = failure(&output);

    assert_eq!((code, kind), (Some(5), json!("quality")), "{message}");
    assert!(message.contains("streaming live"), "{message}");
}

#[test]
fn no_track_in_the_original_language_is_a_quality_failure_naming_the_tracks() {
    let (code, kind, message) = player_failure(
        &zoo_player(&[track("http://127.0.0.1:9", "de", false)], 9),
        "full_text",
    );

    assert_eq!((code, kind), (Some(5), json!("quality")), "{message}");
    assert!(
        message.contains("its captions are in de; pass --lang")
            && message.contains("`forager platform youtube transcribe`"),
        "{message}"
    );
}

#[test]
fn a_track_outside_the_configured_origin_is_never_requested() {
    let (code, kind, message) = player_failure(
        &zoo_player(&[track("https://elsewhere.example", "en", false)], 0),
        "full_text",
    );

    assert_eq!((code, kind), (Some(4), json!("runtime")), "{message}");
    assert!(
        message.contains("outside the configured origin"),
        "{message}"
    );
}

#[test]
fn an_empty_track_body_is_a_runtime_failure_about_a_soft_block() {
    let youtube = Fixture::start_sequence_for(|origin| zoo_full_text(origin, ""));
    let environment = RunEnvironment::new(&config(&youtube.url));

    let output = fetch(&environment, &[]);
    youtube.finish_all();
    let (code, kind, message) = failure(&output);

    assert_eq!((code, kind), (Some(4), json!("runtime")), "{message}");
    assert!(
        message.contains("softly blocking this egress IP"),
        "{message}"
    );
}
