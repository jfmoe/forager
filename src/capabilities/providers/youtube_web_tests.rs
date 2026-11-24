use std::time::Duration;

use serde_json::{Value, json};

use super::{
    LiveBroadcastDetails, MicroformatRenderer, PlayabilityStatus, Video, VideoDetails, YoutubeWeb,
    check_playability, embedded_player_response, innertube_key, live_status, read_player,
    read_watch_page, render_transcript,
};
use crate::config::HttpRouteRuntimeConfig;
use crate::fixture_server::{Fixture, Response, block_on, header, pairs, query, request_json};
use crate::net::{RetryPolicy, build_client};
use crate::providers::RouteContext;
use crate::providers::test_projection::refused;
use crate::samples::sample;
use crate::types::{
    AttemptDisposition, AttemptErrorKind, ContentDepth, Deadline, FullTextSource,
    PlatformFetchOptions, PlatformFetchOutcome, PlatformFetchRequest, PlatformRef, ProviderError,
    YoutubeFetchOptions, YoutubeLiveStatus, YoutubeRef,
};

/// The video every stored sample describes.
const ZOO: &str = "jNQXAC9IVRw";
/// The origin of the stored player's caption tracks, which a decoding test reads under.
const STORED_ORIGIN: &str = "https://www.youtube.com";
/// The fake `InnerTube` key of the stored watch page.
const STORED_KEY: &str = "AIzaSyFixtureInnertubeKey_0123456789";

/// The stored samples' video rendered from its English publisher track.
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

fn watch_page() -> String {
    sample("youtube_web/watch.html")
}

/// The stored watch page with `change` applied to the player response it embeds.
fn watch_page_with(change: impl FnOnce(&mut Value)) -> String {
    let page = watch_page();
    let marker = "ytInitialPlayerResponse = ";
    let start = page.find(marker).expect("the stored page embeds a player") + marker.len();
    let mut values = serde_json::Deserializer::from_str(&page[start..]).into_iter::<Value>();
    let mut embedded = values
        .next()
        .expect("an embedded player")
        .expect("valid embedded JSON");
    let end = start + values.byte_offset();
    change(&mut embedded);
    format!("{}{embedded}{}", &page[..start], &page[end..])
}

/// The stored player response with its caption tracks served from `origin`.
fn player_at(origin: &str) -> Value {
    serde_json::from_str(&sample("youtube_web/player.json").replace(STORED_ORIGIN, origin))
        .expect("stored player response")
}

fn player() -> Value {
    player_at(STORED_ORIGIN)
}

/// A caption track list in the stored player's shape: each of `tracks`, a language and whether
/// speech recognition made it, is the stored English track in that language and from that
/// source, at `origin`; the player shows `default_track` by default.
pub(super) fn caption_tracks(
    origin: &str,
    tracks: &[(&str, bool)],
    default_track: Option<usize>,
) -> Value {
    let mut list = player_at(origin)["captions"]["playerCaptionsTracklistRenderer"].take();
    let stored = list["captionTracks"][0].clone();
    list["captionTracks"] = tracks
        .iter()
        .map(|(language, asr)| {
            let mut track = stored.clone();
            let caps = if *asr { "&caps=asr" } else { "" };
            let url = stored["baseUrl"]
                .as_str()
                .expect("stored track URL")
                .replace("&xoaf=", &format!("{caps}&xoaf="))
                .replace("&lang=en&", &format!("&lang={language}&"));
            track["baseUrl"] = json!(url);
            track["name"]["runs"][0]["text"] = json!(language);
            track["languageCode"] = json!(language);
            if *asr {
                track["kind"] = json!("asr");
                track["vssId"] = json!(format!("a.{language}"));
            } else {
                track["vssId"] = json!(format!(".{language}"));
            }
            track
        })
        .collect();
    list["audioTracks"][0]["captionTrackIndices"] = json!((0..tracks.len()).collect::<Vec<_>>());
    list["audioTracks"][0]["defaultCaptionTrackIndex"] = json!(default_track);
    list
}

/// The stored player with the caption tracks of [`caption_tracks`].
fn player_with_tracks(origin: &str, tracks: &[(&str, bool)], default_track: usize) -> Value {
    let mut player = player_at(origin);
    player["captions"]["playerCaptionsTracklistRenderer"] =
        caption_tracks(origin, tracks, Some(default_track));
    player
}

/// The stored player's refusal: its playability says `status` for `reason`, and it carries
/// nothing else the route reads.
fn refusal(status: &str, reason: &str) -> Value {
    let mut player = player();
    player["playabilityStatus"] = json!({"status": status, "reason": reason});
    let fields = player.as_object_mut().expect("player object");
    fields.remove("videoDetails");
    fields.remove("captions");
    player
}

fn srv1() -> String {
    sample("youtube_web/srv1.xml")
}

fn zoo() -> YoutubeRef {
    YoutubeRef::video(ZOO).expect("video ref")
}

/// What the watch page `page` and the player answer `player` give a fetch of the stored video
/// at `depth`, read under the stored tracks' origin.
fn read(
    page: &str,
    player: &Value,
    depth: ContentDepth,
    languages: &[&str],
) -> Result<Video, (AttemptErrorKind, String)> {
    let page = refused(read_watch_page(page))?;
    let languages = languages
        .iter()
        .map(|tag| (*tag).to_owned())
        .collect::<Vec<_>>();
    refused(read_player(
        &player.to_string(),
        &zoo(),
        &page,
        (depth, true),
        &languages,
        STORED_ORIGIN,
    ))
}

/// The refusal of a read that must fail.
fn refusal_of(result: Result<Video, (AttemptErrorKind, String)>) -> (AttemptErrorKind, String) {
    match result {
        Ok(_) => panic!("the read succeeds"),
        Err(refusal) => refusal,
    }
}

fn html(body: &str) -> Response {
    Response::new(200, "text/html; charset=utf-8", body)
}

fn srv1_response(body: &str) -> Response {
    Response::new(200, "text/xml; charset=UTF-8", body)
}

/// The route against `url`, as the provider factory builds it.
fn route(url: &str) -> YoutubeWeb {
    YoutubeWeb::build(
        HttpRouteRuntimeConfig {
            url: url.to_owned(),
            timeout_seconds: 5,
        },
        RouteContext {
            client: build_client(true).expect("build client"),
            retry_policy: RetryPolicy::new(3, 1.0, Duration::ZERO),
            deadline: Deadline::new(Duration::from_secs(10)),
        },
    )
}

/// Fetches the stored video at `depth` with `languages` from the route at `url`.
fn fetch(
    url: &str,
    depth: ContentDepth,
    languages: &[&str],
) -> Result<PlatformFetchOutcome, ProviderError> {
    let request = PlatformFetchRequest {
        options: PlatformFetchOptions::Youtube(YoutubeFetchOptions {
            languages: languages.iter().map(|tag| (*tag).to_owned()).collect(),
            ..YoutubeFetchOptions::default()
        }),
        ..PlatformFetchRequest::new(PlatformRef::Youtube(zoo()), depth)
    };
    block_on(route(url).fetch(&request))
}

/// The method and path of a recorded request.
fn method_and_path(request: &str) -> String {
    let mut line = request.split_whitespace();
    let method = line.next().expect("request method");
    let target = line.next().expect("request target");
    format!("{method} {}", target.split('?').next().unwrap_or_default())
}

#[test]
fn a_full_text_fetch_requests_the_page_the_player_and_the_chosen_track_in_english() {
    let fixture = Fixture::start_sequence_for(|origin| {
        vec![
            html(&watch_page()),
            Response::json(200, &player_at(origin).to_string()),
            srv1_response(&srv1()),
        ]
    });

    let outcome = fetch(&fixture.url, ContentDepth::FullText, &[]).expect("fetch succeeds");
    let requests = fixture.finish_all();

    assert_eq!(
        (
            requests
                .iter()
                .map(|request| method_and_path(request))
                .collect::<Vec<_>>(),
            query(&requests[0]),
            query(&requests[1]),
            request_json(&requests[1]),
            query(&requests[2]),
            requests
                .iter()
                .map(|request| header(request, "accept-language"))
                .collect::<Vec<_>>(),
        ),
        (
            vec![
                "GET /watch".to_owned(),
                "POST /youtubei/v1/player".to_owned(),
                "GET /api/timedtext".to_owned()
            ],
            pairs(&[("v", ZOO)]),
            pairs(&[("key", STORED_KEY)]),
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
        )
    );
    assert_eq!(
        outcome
            .attempts
            .iter()
            .map(|attempt| (attempt.provider, attempt.disposition))
            .collect::<Vec<_>>(),
        [("youtube_web", AttemptDisposition::Succeeded); 3]
    );
}

#[test]
fn a_full_text_fetch_returns_the_rendered_transcript_and_reports_it_on_the_item() {
    let fixture = Fixture::start_sequence_for(|origin| {
        vec![
            html(&watch_page()),
            Response::json(200, &player_at(origin).to_string()),
            srv1_response(&srv1()),
        ]
    });

    let outcome = fetch(&fixture.url, ContentDepth::FullText, &[]).expect("fetch succeeds");
    fixture.finish_all();
    let item = serde_json::to_value(&outcome.item).expect("serialize item");

    assert_eq!(
        (&item["transcript"], outcome.content_source),
        (
            &json!({"language": "en", "source": "manual", "cues": 6, "paragraphs": 2}),
            FullTextSource::Native(ZOO_TRANSCRIPT.to_owned())
        )
    );
}

#[test]
fn the_full_text_item_carries_the_videos_metadata_and_its_caption_tracks() {
    let video = read(&watch_page(), &player(), ContentDepth::FullText, &[])
        .unwrap_or_else(|refusal| panic!("the read succeeds: {refusal:?}"));
    let mut item = serde_json::to_value(&video.item).expect("serialize item");
    item.as_object_mut()
        .expect("item object")
        .remove("transcript");

    assert_eq!(
        item,
        json!({
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
            ]
        })
    );
}

#[test]
fn requested_languages_read_the_first_publisher_track_in_their_order() {
    let fixture = Fixture::start_sequence_for(|origin| {
        vec![
            html(&watch_page()),
            Response::json(
                200,
                &player_with_tracks(origin, &[("en", true), ("en", false), ("de", false)], 1)
                    .to_string(),
            ),
            srv1_response(&srv1()),
        ]
    });

    let outcome =
        fetch(&fixture.url, ContentDepth::FullText, &["fr", "de"]).expect("fetch succeeds");
    let requests = fixture.finish_all();
    let FullTextSource::Native(markdown) = outcome.content_source else {
        panic!("a full-text fetch returns the transcript")
    };

    assert_eq!(
        (
            query(&requests[2]).get("lang").cloned(),
            markdown.contains("- Transcript: `de`, captions the publisher uploaded\n"),
            markdown
                .trim_end()
                .ends_with("and that's pretty much all there is to say"),
        ),
        (Some("de".to_owned()), true, true),
        "{markdown}"
    );
}

#[test]
fn an_abstract_fetch_inlines_the_description_and_its_chapters() {
    let video = read(&watch_page(), &player(), ContentDepth::Abstract, &[])
        .unwrap_or_else(|refusal| panic!("the read succeeds: {refusal:?}"));
    let item = serde_json::to_value(&video.item).expect("serialize item");

    assert_eq!(
        (
            &item["depth"],
            &item["description"],
            &item["chapters"],
            video.caption.is_none()
        ),
        (
            &json!("abstract"),
            &json!("The first video on YouTube.\n\n00:00 Intro\n00:05 The cool thing\n00:17 End"),
            &json!([
                {"start_seconds": 0, "title": "Intro"},
                {"start_seconds": 5, "title": "The cool thing"},
                {"start_seconds": 17, "title": "End"}
            ]),
            true
        )
    );
}

#[test]
fn a_rate_limited_watch_page_is_rate_limited_and_names_the_egress_ip() {
    let fixture = Fixture::start_sequence(vec![Response::new(429, "text/html", "")]);

    let error = fetch(&fixture.url, ContentDepth::Metadata, &[])
        .map(|_| ())
        .expect_err("a rate-limited fetch fails");

    assert_eq!(
        (error.kind, fixture.finish_all().len()),
        (AttemptErrorKind::RateLimited, 1),
        "{}",
        error.message
    );
    assert!(error.message.contains("egress IP"), "{}", error.message);
}

#[test]
fn a_bot_check_page_without_a_key_is_an_auth_failure() {
    let Err((kind, message)) = refused(read_watch_page(&sample("youtube_web/bot-check.html")))
    else {
        panic!("a bot check gives no key")
    };

    assert_eq!(kind, AttemptErrorKind::Auth, "{message}");
    assert!(message.contains("egress IP"), "{message}");
}

#[test]
fn a_missing_video_is_an_attempt_level_parameter_failure() {
    let refusal = refusal_of(read(
        &watch_page(),
        &refusal("ERROR", "This video is unavailable"),
        ContentDepth::Metadata,
        &[],
    ));

    assert_eq!(
        refusal,
        (
            AttemptErrorKind::Parameter,
            "YouTube video not found or not playable: youtube:jNQXAC9IVRw (This video is unavailable)".to_owned()
        )
    );
}

#[test]
fn a_bot_check_of_the_player_is_an_auth_failure_about_the_egress_ip() {
    let (kind, message) = refusal_of(read(
        &watch_page(),
        &refusal("LOGIN_REQUIRED", "Sign in to confirm you’re not a bot"),
        ContentDepth::Metadata,
        &[],
    ));

    assert_eq!(kind, AttemptErrorKind::Auth, "{message}");
    assert!(message.contains("egress IP"), "{message}");
}

#[test]
fn a_player_answer_for_another_video_is_a_runtime_failure() {
    let mut player = player();
    player["videoDetails"]["videoId"] = json!("dQw4w9WgXcQ");

    let (kind, message) = refusal_of(read(&watch_page(), &player, ContentDepth::Metadata, &[]));

    assert_eq!(kind, AttemptErrorKind::Runtime, "{message}");
    assert!(message.contains("dQw4w9WgXcQ"), "{message}");
}

#[test]
fn a_live_stream_has_no_full_text_yet() {
    let page = watch_page_with(|embedded| {
        embedded["microformat"]["playerMicroformatRenderer"]["liveBroadcastDetails"] =
            json!({"isLiveNow": true, "startTimestamp": "2026-10-17T10:00:00+00:00"});
    });

    let (kind, message) = refusal_of(read(&page, &player(), ContentDepth::FullText, &[]));

    assert_eq!(kind, AttemptErrorKind::Quality, "{message}");
    assert!(message.contains("streaming live"), "{message}");
}

#[test]
fn tracks_without_a_known_original_language_are_a_quality_failure_naming_them() {
    let player = player_with_tracks(STORED_ORIGIN, &[("de", false), ("fr", false)], 9);

    let (kind, message) = refusal_of(read(&watch_page(), &player, ContentDepth::FullText, &[]));

    assert_eq!(kind, AttemptErrorKind::Quality, "{message}");
    assert!(
        message.contains("its captions are in de, fr; pass --lang")
            && message.contains("`forager platform youtube transcribe`"),
        "{message}"
    );
}

#[test]
fn a_track_outside_the_configured_origin_is_never_requested() {
    let fixture = Fixture::start_sequence(vec![
        html(&watch_page()),
        Response::json(200, &player_at("https://elsewhere.example").to_string()),
    ]);

    let error = fetch(&fixture.url, ContentDepth::FullText, &[])
        .map(|_| ())
        .expect_err("the track is not read");

    assert_eq!(
        (error.kind, fixture.finish_all().len()),
        (AttemptErrorKind::Runtime, 2),
        "{}",
        error.message
    );
    assert!(
        error.message.contains("outside the configured origin"),
        "{}",
        error.message
    );
}

#[test]
fn an_empty_track_body_is_a_runtime_failure_about_a_soft_block() {
    let video = read(&watch_page(), &player(), ContentDepth::FullText, &[])
        .unwrap_or_else(|refusal| panic!("the read succeeds: {refusal:?}"));
    let caption = video.caption.expect("a full-text read picks a track");

    let Err((kind, message)) = refused(render_transcript(
        "",
        &caption,
        &video.item,
        &video.description,
    )) else {
        panic!("an empty track has no transcript")
    };

    assert_eq!(kind, AttemptErrorKind::Runtime, "{message}");
    assert!(
        message.contains("softly blocking this egress IP"),
        "{message}"
    );
}

#[test]
fn the_watch_page_gives_its_innertube_key_and_embedded_player_response() {
    // A `;` or a `}` inside a string does not end the embedded player response.
    let page = watch_page_with(|embedded| embedded["videoDetails"]["title"] = json!("a;b}"));
    let without_a_key = [
        watch_page().replace(STORED_KEY, ""),
        watch_page().replace(STORED_KEY, "a b"),
        sample("youtube_web/bot-check.html"),
    ];

    let embedded = embedded_player_response(&page)
        .and_then(|player| player.video_details)
        .and_then(|details| details.title);

    assert_eq!(
        (innertube_key(&page), embedded.as_deref()),
        (Some(STORED_KEY), Some("a;b}"))
    );
    assert_eq!(
        without_a_key.each_ref().map(|page| innertube_key(page)),
        [None, None, None]
    );
}

fn playability(status: Option<&str>, reason: Option<&str>) -> Option<AttemptErrorKind> {
    let reference = YoutubeRef::video("jNQXAC9IVRw").expect("video ref");
    let status = PlayabilityStatus {
        status: status.map(str::to_owned),
        reason: reason.map(str::to_owned),
    };
    check_playability(Some(&status), &reference)
        .err()
        .map(|refusal| refusal.kind)
}

#[test]
fn playability_maps_missing_videos_to_parameter_and_sign_in_demands_to_auth() {
    let kinds = [
        playability(Some("OK"), None),
        playability(None, None),
        playability(Some("ERROR"), Some("This video is unavailable")),
        playability(Some("UNPLAYABLE"), Some("Video unavailable")),
        playability(
            Some("LOGIN_REQUIRED"),
            Some("Sign in to confirm you’re not a bot"),
        ),
        playability(Some("LOGIN_REQUIRED"), Some("Sign in to confirm your age")),
        playability(Some("CONTENT_CHECK_REQUIRED"), None),
    ];

    assert_eq!(
        kinds,
        [
            None,
            None,
            Some(AttemptErrorKind::Parameter),
            Some(AttemptErrorKind::Parameter),
            Some(AttemptErrorKind::Auth),
            Some(AttemptErrorKind::Auth),
            Some(AttemptErrorKind::Parameter),
        ]
    );
}

fn details(live_content: bool, upcoming: Option<bool>) -> VideoDetails {
    VideoDetails {
        video_id: None,
        title: None,
        author: None,
        channel_id: None,
        length_seconds: None,
        view_count: None,
        short_description: None,
        is_live_content: Some(live_content),
        is_upcoming: upcoming,
    }
}

fn broadcast(live_now: bool, ended: bool) -> MicroformatRenderer {
    MicroformatRenderer {
        publish_date: None,
        live_broadcast_details: Some(LiveBroadcastDetails {
            is_live_now: Some(live_now),
            end_timestamp: ended.then(|| "2026-10-17T11:00:00+00:00".to_owned()),
        }),
    }
}

#[test]
fn broadcast_details_decide_the_live_status() {
    let statuses = [
        live_status(&details(false, None), None),
        live_status(&details(true, Some(true)), Some(&broadcast(false, false))),
        live_status(&details(true, None), Some(&broadcast(true, false))),
        live_status(&details(true, None), Some(&broadcast(false, true))),
        live_status(&details(true, None), Some(&broadcast(false, false))),
    ];

    assert_eq!(
        statuses,
        [
            Some(YoutubeLiveStatus::None),
            Some(YoutubeLiveStatus::Upcoming),
            Some(YoutubeLiveStatus::Live),
            Some(YoutubeLiveStatus::Completed),
            Some(YoutubeLiveStatus::Upcoming),
        ]
    );
}

#[test]
fn live_content_without_broadcast_details_has_an_unknown_live_status() {
    assert_eq!(live_status(&details(true, None), None), None);
}
