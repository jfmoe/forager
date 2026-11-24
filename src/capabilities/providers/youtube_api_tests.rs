use std::collections::BTreeMap;
use std::time::Duration;

use chrono::NaiveDate;
use serde_json::{Value, json};

use super::comments::decode_threads;
use super::search::{decode_details, decode_results, enrich};
use super::videos::listed_page;
use super::{
    LiveStreamingDetails, YoutubeApi, decode_video, duration_seconds, failure_message,
    fetch_support, live_status, reason_kind,
};
use crate::config::{KeyedHttpRouteRuntimeConfig, runtime_config_from_toml};
use crate::fixture_server::{Fixture, Response, block_on, header, ok_json, pairs, path, query};
use crate::net::{RetryPolicy, build_client};
use crate::providers::RouteContext;
use crate::providers::test_projection::{failure, to_json};
use crate::samples::sample_json;
use crate::types::{
    AttemptDisposition, AttemptErrorKind, AttemptTarget, ContentDepth, Deadline,
    PlatformFetchOutcome, PlatformFetchRequest, PlatformRef, PlatformSearchOptions,
    PlatformSearchOutcome, PlatformSearchRequest, ProviderError, YoutubeCollection,
    YoutubeCommentSort, YoutubeCommentsOutcome, YoutubeCommentsRequest, YoutubeDuration,
    YoutubeLiveStatus, YoutubeRef, YoutubeSearchOptions, YoutubeSort, YoutubeVideosOutcome,
    YoutubeVideosRequest,
};
use crate::youtube_api_samples::{
    comment_threads_page, playlist_item, playlist_items_page, search_page, videos_page, zoo_video,
};

/// The stored error body as a `status` answer with one `errors[]` reason, an `ErrorInfo`
/// detail only when `info_reason` is given, and `message`.
fn api_error(status: u16, reason: &str, info_reason: Option<&str>, message: &str) -> Value {
    let mut body = sample_json("youtube_api/error.json");
    let error = &mut body["error"];
    error["code"] = json!(status);
    error["message"] = json!(message);
    error["errors"][0]["message"] = json!(message);
    error["errors"][0]["reason"] = json!(reason);
    error["status"] = json!(if status == 400 {
        "INVALID_ARGUMENT"
    } else {
        "PERMISSION_DENIED"
    });
    match info_reason {
        Some(info_reason) => error["details"][0]["reason"] = json!(info_reason),
        None => error["details"] = json!([]),
    }
    body
}

/// The stored first playlist entry as one YouTube no longer lets anyone read, titled
/// `Private video` or `Deleted video`: no owner and no publication time.
fn unreadable_playlist_item(playlist: &str, position: u64, video_id: &str, title: &str) -> Value {
    let mut item = playlist_item(
        playlist,
        position,
        video_id,
        title,
        "This video is private.",
    );
    item["snippet"]["publishedAt"] = json!("2016-08-07T00:00:00Z");
    let snippet = item["snippet"].as_object_mut().expect("snippet");
    snippet.remove("videoOwnerChannelTitle");
    snippet.remove("videoOwnerChannelId");
    item["contentDetails"] = json!({"videoId": video_id});
    item
}

/// The stored `channels.list` response with the uploads playlist of each channel ID given.
fn channels_page(channels: &[(&str, &str)]) -> Value {
    let mut page = sample_json("youtube_api/channels.json");
    let stored_channel = page["items"][0].clone();
    let items = channels
        .iter()
        .map(|(id, uploads)| {
            let mut channel = stored_channel.clone();
            channel["etag"] = json!(format!("etag-{id}"));
            channel["id"] = json!(id);
            channel["contentDetails"]["relatedPlaylists"]["uploads"] = json!(uploads);
            channel
        })
        .collect::<Vec<_>>();
    page["pageInfo"]["totalResults"] = json!(items.len());
    let fields = page.as_object_mut().expect("channel list");
    // The API leaves `items` out when nothing matches.
    if items.is_empty() {
        fields.remove("items");
    } else {
        fields.insert("items".to_owned(), json!(items));
    }
    page
}

const KEY: &str = "yt-fixture-key-0123456789";
/// The video of the stored `videos.list` and comment samples.
const ZOO: &str = "jNQXAC9IVRw";
/// The playlist of the stored playlist entries.
const PLAYLIST: &str = "PLZHQObOWTQDPD3MizzM2xVFitgF8hE_ab";
/// The channel of the stored search results, playlist entries and channel list.
const CHANNEL: &str = "UCExampleMathsChannel001";
const UPLOADS: &str = "UUExampleMathsChannel001";
/// The stored description of the zoo video.
const CHAPTERED: &str =
    "The first video on YouTube.\n\n0:00 Elephants\n0:07 Trunks\n0:14 That's it";

/// The route against the fixture at `url`, under the path the real API uses, built as its
/// factory does with one key and one attempt per request.
fn route(url: &str) -> YoutubeApi {
    YoutubeApi::build(
        KeyedHttpRouteRuntimeConfig {
            url: format!("{url}/youtube/v3"),
            keys: vec![KEY.into()],
            timeout_seconds: 5,
        },
        RouteContext {
            client: build_client(true).expect("build client"),
            retry_policy: RetryPolicy::new(1, 1.0, Duration::ZERO),
            deadline: Deadline::new(Duration::from_secs(10)),
        },
    )
}

fn error(status: u16, body: &Value) -> Response {
    Response::json(status, &body.to_string())
}

/// The header `x-goog-api-key` of a request, and whether the key appears anywhere else in it.
fn key_placement(request: &str) -> (Option<String>, bool) {
    let request_line = request.lines().next().unwrap_or_default();
    (
        header(request, "x-goog-api-key"),
        request_line.contains(KEY),
    )
}

fn video(id: &str) -> YoutubeRef {
    YoutubeRef::video(id).expect("a video ID")
}

fn channel() -> YoutubeRef {
    YoutubeRef::channel(CHANNEL).expect("a channel ID")
}

fn playlist() -> YoutubeRef {
    YoutubeRef::playlist(PLAYLIST).expect("a playlist ID")
}

fn search_request(
    query: &str,
    limit: u16,
    options: YoutubeSearchOptions,
    page: Option<&str>,
) -> PlatformSearchRequest {
    PlatformSearchRequest {
        query: query.to_owned(),
        limit,
        options: PlatformSearchOptions::Youtube(options),
        page: page.map(str::to_owned),
    }
}

fn search(
    request: &PlatformSearchRequest,
    answers: Vec<Response>,
) -> (Result<PlatformSearchOutcome, ProviderError>, Vec<String>) {
    let fixture = Fixture::start_sequence(answers);
    let outcome = block_on(route(&fixture.url).search(request));
    (outcome, fixture.finish_all())
}

fn fetch(
    id: &str,
    depth: ContentDepth,
    answers: Vec<Response>,
) -> (Result<PlatformFetchOutcome, ProviderError>, Vec<String>) {
    let fixture = Fixture::start_sequence(answers);
    let request = PlatformFetchRequest::new(PlatformRef::Youtube(video(id)), depth);
    let outcome = block_on(route(&fixture.url).fetch(&request));
    (outcome, fixture.finish_all())
}

fn comments(
    request: &YoutubeCommentsRequest,
    answers: Vec<Response>,
) -> (Result<YoutubeCommentsOutcome, ProviderError>, Vec<String>) {
    let fixture = Fixture::start_sequence(answers);
    let outcome = block_on(route(&fixture.url).comments(request));
    (outcome, fixture.finish_all())
}

fn videos(
    request: &YoutubeVideosRequest,
    answers: Vec<Response>,
) -> (Result<YoutubeVideosOutcome, ProviderError>, Vec<String>) {
    let fixture = Fixture::start_sequence(answers);
    let outcome = block_on(route(&fixture.url).videos(request));
    (outcome, fixture.finish_all())
}

fn comments_of(id: &str) -> YoutubeCommentsRequest {
    YoutubeCommentsRequest {
        video: video(id),
        limit: 20,
        sort: YoutubeCommentSort::Relevance,
    }
}

fn sent(requests: &[String]) -> Vec<(String, BTreeMap<String, String>)> {
    requests
        .iter()
        .map(|request| (path(request), query(request)))
        .collect()
}

// Decoding helpers and the error vocabulary.

#[test]
fn iso_durations_read_as_seconds_and_zero_is_unknown() {
    let read = [
        "PT19S", "PT21M3S", "PT1H2M3S", "P1DT2H", "PT0S", "P0D", "PT", "PT3S4M", "21M",
    ]
    .map(duration_seconds);

    assert_eq!(
        read,
        [
            Some(19),
            Some(1263),
            Some(3723),
            Some(93_600),
            None,
            None,
            None,
            None,
            None
        ]
    );
}

#[test]
fn stream_details_decide_the_live_status_before_the_broadcast_flag() {
    let details = |start: Option<&str>, end: Option<&str>| LiveStreamingDetails {
        actual_start_time: start.map(str::to_owned),
        actual_end_time: end.map(str::to_owned),
    };
    let statuses = [
        live_status(Some("none"), None),
        live_status(Some("upcoming"), None),
        live_status(Some("live"), None),
        live_status(Some("none"), Some(&details(Some("t"), Some("u")))),
        live_status(Some("live"), Some(&details(Some("t"), None))),
        live_status(Some("upcoming"), Some(&details(None, None))),
    ];

    assert_eq!(
        statuses,
        [
            YoutubeLiveStatus::None,
            YoutubeLiveStatus::Upcoming,
            YoutubeLiveStatus::Live,
            YoutubeLiveStatus::Completed,
            YoutubeLiveStatus::Live,
            YoutubeLiveStatus::Upcoming,
        ]
    );
}

#[test]
fn error_reasons_override_the_status_only_where_the_status_misleads() {
    let body = |reason: &str, info: Option<&str>| api_error(400, reason, info, "m").to_string();
    let kinds = [
        reason_kind(&body("badRequest", Some("API_KEY_INVALID"))),
        reason_kind(&body("quotaExceeded", None)),
        reason_kind(&body("dailyLimitExceeded", None)),
        reason_kind(&body("commentsDisabled", None)),
        reason_kind(&body("invalid", None)),
        reason_kind("not json"),
    ];

    assert_eq!(
        kinds,
        [
            Some(AttemptErrorKind::Auth),
            Some(AttemptErrorKind::QuotaExhausted),
            Some(AttemptErrorKind::QuotaExhausted),
            Some(AttemptErrorKind::Parameter),
            None,
            None,
        ]
    );
}

#[test]
fn failure_messages_carry_the_api_message_as_plain_text() {
    let body = api_error(
        400,
        "invalidSearchFilter",
        None,
        r#"The <code><a href="/youtube/v3/docs/search/list#q">q</a></code> parameter &amp; <b>order</b> conflict."#,
    );

    assert_eq!(
        failure_message(&body.to_string(), 400),
        "YouTube Data API returned HTTP 400 (invalidSearchFilter): The q parameter & order conflict."
    );
}

// Configuration.

#[test]
fn a_second_key_is_a_configuration_error_naming_the_keys_setting() {
    let result = runtime_config_from_toml(
        "[providers.youtube_api]\nkeys = [\"first-key\", \"second-key\"]\n",
    );

    let Err(error) = result else {
        panic!("two keys should not configure the route");
    };
    assert!(
        error.to_string().contains("providers.youtube_api.keys"),
        "{error}"
    );
}

// Search.

#[test]
fn search_options_map_to_data_api_parameters_and_the_key_travels_only_in_a_header() {
    let request = search_request(
        "attention -music",
        10,
        YoutubeSearchOptions {
            sort: YoutubeSort::Views,
            from: NaiveDate::from_ymd_opt(2024, 1, 1),
            to: NaiveDate::from_ymd_opt(2024, 1, 31),
            duration: Some(YoutubeDuration::Long),
            channel: Some(CHANNEL.to_owned()),
            has_captions: true,
        },
        None,
    );

    let (outcome, requests) = search(&request, vec![ok_json(&search_page(&[], None))]);
    let outcome = outcome.expect("an empty page is a success");

    assert_eq!(
        (
            sent(&requests),
            key_placement(&requests[0]),
            outcome.items.len(),
            outcome.next_page,
        ),
        (
            vec![(
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
                    ("channelId", CHANNEL),
                    ("videoCaption", "closedCaption"),
                ])
            )],
            (Some(KEY.to_owned()), false),
            0,
            None,
        )
    );
}

#[test]
fn a_page_position_is_sent_as_the_page_token() {
    let request = search_request(
        "attention",
        5,
        YoutubeSearchOptions {
            sort: YoutubeSort::Date,
            has_captions: true,
            ..YoutubeSearchOptions::default()
        },
        Some("CAUQAA"),
    );

    let (outcome, requests) = search(&request, vec![ok_json(&search_page(&[], None))]);

    assert_eq!(
        (
            outcome.map(|outcome| outcome.items.len()).ok(),
            query(&requests[0])
        ),
        (
            Some(0),
            pairs(&[
                ("part", "snippet"),
                ("type", "video"),
                ("q", "attention"),
                ("maxResults", "5"),
                ("order", "date"),
                ("videoCaption", "closedCaption"),
                ("pageToken", "CAUQAA"),
            ])
        )
    );
}

#[test]
fn search_results_decode_their_entities_and_an_excerpt_makes_a_snippet() {
    let (items, next_page) = decode_results(&sample_json("youtube_api/search.json").to_string())
        .unwrap_or_else(|refusal| panic!("{}", refusal.message));

    assert_eq!(
        (
            items
                .iter()
                .map(|item| (
                    to_json(&item.reference),
                    to_json(&item.depth),
                    item.title.clone(),
                    to_json(&item.data)["snippet"].clone(),
                ))
                .collect::<Vec<_>>(),
            next_page,
        ),
        (
            vec![
                (
                    json!("youtube:eMlx5fFNoYc"),
                    json!("snippet"),
                    Some(
                        "Attention in transformers, step-by-step | Deep Learning Chapter 6"
                            .to_owned()
                    ),
                    json!("Demystifying attention, the key mechanism inside transformers & LLMs."),
                ),
                (
                    json!("youtube:Ks-_Mh1QhMc"),
                    json!("metadata"),
                    Some("Your Body Language May Shape Who You Are 'TED'".to_owned()),
                    Value::Null,
                ),
            ],
            Some("CAUQAA".to_owned()),
        )
    );
}

#[test]
fn search_results_carry_the_details_of_one_batched_video_request() {
    let request = search_request("attention", 20, YoutubeSearchOptions::default(), None);

    let (outcome, requests) = search(
        &request,
        vec![
            ok_json(&sample_json("youtube_api/search.json")),
            ok_json(&sample_json("youtube_api/video-details.json")),
        ],
    );
    let outcome = outcome.expect("the search succeeds");

    assert_eq!(
        (
            query(&requests[0]).get("maxResults").cloned(),
            sent(&requests[1..]),
            key_placement(&requests[1]),
            to_json(&outcome.items),
            outcome.next_page,
            outcome.diagnostic,
        ),
        (
            Some("20".to_owned()),
            vec![(
                "/youtube/v3/videos".to_owned(),
                pairs(&[
                    ("part", "contentDetails,statistics"),
                    ("id", "eMlx5fFNoYc,Ks-_Mh1QhMc")
                ])
            )],
            (Some(KEY.to_owned()), false),
            json!([
                {
                    "ref": "youtube:eMlx5fFNoYc",
                    "url": "https://www.youtube.com/watch?v=eMlx5fFNoYc",
                    "depth": "snippet",
                    "title": "Attention in transformers, step-by-step | Deep Learning Chapter 6",
                    "authors": ["Example Maths"],
                    "published": "2024-04-07T12:53:54Z",
                    "channel_ref": "youtube:channel/UCExampleMathsChannel001",
                    "snippet": "Demystifying attention, the key mechanism inside transformers & LLMs.",
                    "duration_seconds": 1570,
                    "live_status": "none",
                    "views": 4_773_774,
                    "likes": 91777,
                    "comments": 2478,
                    "language": null,
                    "has_captions": true,
                    "caption_tracks": null
                },
                {
                    "ref": "youtube:Ks-_Mh1QhMc",
                    "url": "https://www.youtube.com/watch?v=Ks-_Mh1QhMc",
                    "depth": "metadata",
                    "title": "Your Body Language May Shape Who You Are 'TED'",
                    "authors": ["Example Maths"],
                    "published": "2024-04-07T12:53:54Z",
                    "channel_ref": "youtube:channel/UCExampleMathsChannel001",
                    "snippet": null,
                    "duration_seconds": 1263,
                    "live_status": "none",
                    "views": 29_311_923,
                    "likes": 91777,
                    "comments": null,
                    "language": null,
                    "has_captions": true,
                    "caption_tracks": null
                }
            ]),
            Some("CAUQAA".to_owned()),
            None,
        )
    );
}

#[test]
fn a_listed_video_the_detail_batch_leaves_out_keeps_no_details() {
    let (mut items, _) = decode_results(&sample_json("youtube_api/search.json").to_string())
        .unwrap_or_else(|refusal| panic!("{}", refusal.message));
    let mut details = sample_json("youtube_api/video-details.json");
    details["items"] = json!([details["items"][1]]);
    let videos = decode_details(&details.to_string())
        .unwrap_or_else(|refusal| panic!("{}", refusal.message));

    enrich(&mut items, videos);

    assert_eq!(
        items
            .iter()
            .map(|item| to_json(&item.data)["views"].clone())
            .collect::<Vec<_>>(),
        [json!(4_773_774), Value::Null]
    );
}

#[test]
fn a_failed_detail_batch_keeps_the_results_and_says_what_is_missing() {
    let mut page = sample_json("youtube_api/search.json");
    page["items"] = json!([page["items"][0]]);
    let request = search_request("attention", 20, YoutubeSearchOptions::default(), None);

    let (outcome, requests) = search(
        &request,
        vec![
            ok_json(&page),
            error(503, &api_error(503, "backendError", None, "Backend Error")),
        ],
    );
    let outcome = outcome.expect("the results survive a failed batch");
    let item = to_json(&outcome.items[0]);

    assert_eq!(
        (
            requests.len(),
            &item["ref"],
            &item["duration_seconds"],
            &item["views"],
            outcome.diagnostic.as_deref(),
        ),
        (
            2,
            &json!("youtube:eMlx5fFNoYc"),
            &Value::Null,
            &Value::Null,
            Some(
                "YouTube video details unavailable (YouTube Data API returned HTTP 503 (backendError): Backend Error); duration, counts, and captions are missing"
            ),
        )
    );
}

#[test]
fn error_reasons_decide_the_attempt_error_kind_and_the_key_never_appears() {
    let cases = [
        (
            api_error(
                400,
                "badRequest",
                Some("API_KEY_INVALID"),
                "API key not valid. Please pass a valid API key.",
            ),
            400,
            AttemptErrorKind::Auth,
        ),
        (
            api_error(
                403,
                "quotaExceeded",
                None,
                "The request cannot be completed because you have exceeded your quota.",
            ),
            403,
            AttemptErrorKind::QuotaExhausted,
        ),
        (
            api_error(400, "invalid", None, "Invalid value at 'published_after'"),
            400,
            AttemptErrorKind::Parameter,
        ),
    ];
    let request = search_request("attention", 20, YoutubeSearchOptions::default(), None);

    for (body, status, kind) in cases {
        let (outcome, _) = search(&request, vec![error(status, &body)]);

        let Err(failure) = outcome else {
            panic!("a {status} {kind:?} answer should fail");
        };
        let attempts = failure
            .attempts
            .iter()
            .map(|attempt| {
                (
                    attempt.provider,
                    attempt.target,
                    attempt.disposition,
                    attempt.error_kind,
                    attempt.http_status,
                    attempt.transport,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            (failure.kind, attempts),
            (
                kind,
                vec![(
                    "youtube_api",
                    AttemptTarget::platform("youtube", "search"),
                    AttemptDisposition::Failed,
                    Some(kind),
                    Some(status),
                    Some("http"),
                )]
            ),
            "{failure:?}"
        );
        assert!(
            !format!("{failure:?}").contains(KEY),
            "the {kind:?} failure leaked the key: {failure:?}"
        );
    }
}

// Fetch.

#[test]
fn a_fetch_reads_one_video_with_every_part_and_the_key_only_in_a_header() {
    let (outcome, requests) = fetch(
        ZOO,
        ContentDepth::Metadata,
        vec![ok_json(&sample_json("youtube_api/video.json"))],
    );

    assert_eq!(
        (
            outcome.map(|outcome| outcome.item.title).ok().flatten(),
            sent(&requests),
            key_placement(&requests[0]),
        ),
        (
            Some("Me at the zoo".to_owned()),
            vec![(
                "/youtube/v3/videos".to_owned(),
                pairs(&[
                    (
                        "part",
                        "snippet,contentDetails,statistics,status,liveStreamingDetails"
                    ),
                    ("id", ZOO),
                ])
            )],
            (Some(KEY.to_owned()), false),
        )
    );
}

#[test]
fn metadata_reads_the_videos_fields_and_leaves_out_the_description() {
    let item = decode_video(
        &video(ZOO),
        ContentDepth::Metadata,
        true,
        &sample_json("youtube_api/video.json").to_string(),
    )
    .unwrap_or_else(|refusal| panic!("{}", refusal.message));

    assert_eq!(
        to_json(&item),
        json!({
            "ref": "youtube:jNQXAC9IVRw",
            "url": "https://www.youtube.com/watch?v=jNQXAC9IVRw",
            "depth": "metadata",
            "title": "Me at the zoo",
            "authors": ["Example Zoo Visitor"],
            "published": "2005-04-24T03:31:52Z",
            "channel_ref": "youtube:channel/UCExampleZooVisitor00001",
            "snippet": null,
            "duration_seconds": 19,
            "live_status": "none",
            "views": 444_694_156,
            "likes": 20_083_234,
            "comments": 10_641_589,
            "language": "en",
            "has_captions": true,
            "caption_tracks": null
        })
    );
}

#[test]
fn abstract_adds_the_full_description_and_its_chapters() {
    let item = to_json(
        &decode_video(
            &video(ZOO),
            ContentDepth::Abstract,
            true,
            &sample_json("youtube_api/video.json").to_string(),
        )
        .unwrap_or_else(|refusal| panic!("{}", refusal.message)),
    );

    assert_eq!(
        (&item["depth"], &item["description"], &item["chapters"]),
        (
            &json!("abstract"),
            &json!(CHAPTERED),
            &json!([
                {"start_seconds": 0, "title": "Elephants"},
                {"start_seconds": 7, "title": "Trunks"},
                {"start_seconds": 14, "title": "That's it"}
            ]),
        )
    );
}

#[test]
fn a_video_the_api_does_not_return_is_an_attempt_level_parameter_failure() {
    let (outcome, _) = fetch(
        "zzQq9ZxZ9Zq",
        ContentDepth::Metadata,
        vec![ok_json(&videos_page(&[]))],
    );

    assert_eq!(
        failure(outcome),
        (
            AttemptErrorKind::Parameter,
            "YouTube video not found or not public: youtube:zzQq9ZxZ9Zq".to_owned()
        )
    );
}

#[test]
fn abstract_of_a_video_without_a_description_is_a_quality_failure() {
    let (outcome, _) = fetch(
        ZOO,
        ContentDepth::Abstract,
        vec![ok_json(&videos_page(&[zoo_video("  ")]))],
    );

    assert_eq!(
        failure(outcome),
        (
            AttemptErrorKind::Quality,
            "youtube:jNQXAC9IVRw has no description".to_owned()
        )
    );
}

#[test]
fn full_text_and_collections_are_refused_before_any_request() {
    let full_text =
        PlatformFetchRequest::new(PlatformRef::Youtube(video(ZOO)), ContentDepth::FullText);
    let collection =
        PlatformFetchRequest::new(PlatformRef::Youtube(playlist()), ContentDepth::Metadata);

    let (outcome, requests) = fetch(ZOO, ContentDepth::FullText, Vec::new());

    assert_eq!(
        (
            fetch_support(&full_text),
            fetch_support(&collection),
            failure(outcome),
            requests.len(),
        ),
        (
            Err(
                "youtube_api cannot fetch at depth `full_text`: the Data API does not serve captions"
                    .to_owned()
            ),
            Err("youtube_api fetches videos, not a playlist".to_owned()),
            (
                AttemptErrorKind::Parameter,
                "youtube_api cannot fetch at depth `full_text`: the Data API does not serve captions"
                    .to_owned()
            ),
            0,
        )
    );
}

// Comments.

#[test]
fn comments_read_one_page_of_threads_as_plain_text_with_the_key_only_in_a_header() {
    let request = YoutubeCommentsRequest {
        video: video(ZOO),
        limit: 2,
        sort: YoutubeCommentSort::Time,
    };

    let (outcome, requests) = comments(
        &request,
        vec![ok_json(&sample_json("youtube_api/comment-threads.json"))],
    );

    assert_eq!(
        (
            outcome.map(|outcome| outcome.comments.len()).ok(),
            sent(&requests),
            key_placement(&requests[0]),
        ),
        (
            Some(2),
            vec![(
                "/youtube/v3/commentThreads".to_owned(),
                pairs(&[
                    ("part", "snippet,replies"),
                    ("videoId", ZOO),
                    ("textFormat", "plainText"),
                    ("maxResults", "2"),
                    ("order", "time"),
                ])
            )],
            (Some(KEY.to_owned()), false),
        )
    );
}

#[test]
fn comments_carry_their_authors_counts_and_replies_and_say_whether_the_replies_are_complete() {
    let (comments, has_more) = decode_threads(
        &video(ZOO),
        &sample_json("youtube_api/comment-threads.json").to_string(),
    )
    .unwrap_or_else(|refusal| panic!("{}", refusal.message));

    assert_eq!(
        (to_json(&comments), has_more),
        (
            json!([
                {
                    "id": "UgzTop1",
                    "author": "@elephantfan",
                    "author_channel_ref": "youtube:channel/UCelephantfan___________",
                    "text": "The first video ever.",
                    "likes": 4_866_586,
                    "published": "2020-02-17T18:58:15Z",
                    "reply_count": 0,
                    "replies": [],
                    "replies_complete": true
                },
                {
                    "id": "UgzTop2",
                    "author": "@zoofan",
                    "author_channel_ref": "youtube:channel/UCzoofan________________",
                    "text": "Those trunks are long.",
                    "likes": 94,
                    "published": "2020-02-17T18:58:15Z",
                    "reply_count": 1000,
                    "replies": [
                        {
                            "id": "UgzTop2.r1",
                            "author": "@replier",
                            "author_channel_ref": "youtube:channel/UCreplier_______________",
                            "text": "Really long.",
                            "likes": 1,
                            "published": "2020-02-17T18:58:15Z"
                        }
                    ],
                    "replies_complete": false
                }
            ]),
            true
        )
    );
}

#[test]
fn a_video_without_comments_lists_none_and_has_no_more() {
    let (comments, has_more) =
        decode_threads(&video(ZOO), &comment_threads_page(&[], None).to_string())
            .unwrap_or_else(|refusal| panic!("{}", refusal.message));

    assert_eq!((comments.len(), has_more), (0, false));
}

#[test]
fn disabled_comments_are_an_attempt_level_parameter_failure_not_an_empty_list() {
    let (outcome, _) = comments(
        &comments_of("XqZsoesa55w"),
        vec![error(
            403,
            &api_error(
                403,
                "commentsDisabled",
                None,
                "The video identified by the <code>videoId</code> parameter has disabled comments.",
            ),
        )],
    );

    assert_eq!(
        failure(outcome),
        (
            AttemptErrorKind::Parameter,
            "comments are disabled for youtube:XqZsoesa55w".to_owned()
        )
    );
}

#[test]
fn a_missing_video_is_an_attempt_level_parameter_failure_naming_the_video() {
    let (outcome, _) = comments(
        &comments_of("zzQq9ZxZ9Zq"),
        vec![error(
            404,
            &api_error(
                404,
                "videoNotFound",
                None,
                r#"The video identified by the <code><a href="/youtube/v3/docs/commentThreads/list#videoId">videoId</a></code> parameter could not be found."#,
            ),
        )],
    );

    assert_eq!(
        failure(outcome),
        (
            AttemptErrorKind::Parameter,
            "YouTube video not found or not public: youtube:zzQq9ZxZ9Zq".to_owned()
        )
    );
}

// Videos.

#[test]
fn a_playlist_page_reads_its_entries_and_a_later_page_sends_its_token() {
    let first = YoutubeVideosRequest {
        collection: YoutubeCollection::Ref(playlist()),
        limit: 2,
        page: None,
    };
    let later = YoutubeVideosRequest {
        page: Some("EAAaBlBUOkNBRQ".to_owned()),
        ..first.clone()
    };

    let (first_page, first_requests) = videos(
        &first,
        vec![ok_json(&playlist_items_page(
            &sample_json("youtube_api/playlist-items.json")["items"]
                .as_array()
                .expect("entries")
                .clone(),
            Some("EAAaBlBUOkNBRQ"),
        ))],
    );
    let (later_page, later_requests) = videos(
        &later,
        vec![ok_json(&sample_json("youtube_api/playlist-items.json"))],
    );
    let first_page = first_page.expect("the first page succeeds");

    assert_eq!(
        (
            sent(&first_requests),
            key_placement(&first_requests[0]),
            first_page.collection,
            first_page.next_page,
            sent(&later_requests),
            later_page.expect("the later page succeeds").next_page,
        ),
        (
            vec![(
                "/youtube/v3/playlistItems".to_owned(),
                pairs(&[
                    ("part", "snippet,contentDetails"),
                    ("playlistId", PLAYLIST),
                    ("maxResults", "2"),
                ])
            )],
            (Some(KEY.to_owned()), false),
            playlist(),
            Some("EAAaBlBUOkNBRQ".to_owned()),
            vec![(
                "/youtube/v3/playlistItems".to_owned(),
                pairs(&[
                    ("part", "snippet,contentDetails"),
                    ("playlistId", PLAYLIST),
                    ("maxResults", "2"),
                    ("pageToken", "EAAaBlBUOkNBRQ"),
                ])
            )],
            None,
        )
    );
}

#[test]
fn playlist_entries_carry_their_position_owner_and_video_publication_time() {
    let page = listed_page(&sample_json("youtube_api/playlist-items.json").to_string())
        .unwrap_or_else(|refusal| panic!("{}", refusal.message));

    assert_eq!(
        (to_json(&page.items), page.unreadable, page.next_page),
        (
            json!([
                {
                    "ref": "youtube:fNk_zzaMoSs",
                    "url": "https://www.youtube.com/watch?v=fNk_zzaMoSs",
                    "depth": "snippet",
                    "title": "Vectors | Chapter 1, Essence of linear algebra",
                    "authors": ["Example Maths"],
                    "published": "2016-08-06T00:05:49Z",
                    "channel_ref": "youtube:channel/UCExampleMathsChannel001",
                    "snippet": "Beginning the linear algebra series with the basics.",
                    "duration_seconds": null,
                    "live_status": null,
                    "views": null,
                    "likes": null,
                    "comments": null,
                    "language": null,
                    "has_captions": null,
                    "caption_tracks": null,
                    "position": 0
                },
                {
                    "ref": "youtube:k7RM-ot2NWY",
                    "url": "https://www.youtube.com/watch?v=k7RM-ot2NWY",
                    "depth": "metadata",
                    "title": "Linear combinations, span, and basis vectors | Chapter 2",
                    "authors": ["Example Maths"],
                    "published": "2016-08-06T00:05:49Z",
                    "channel_ref": "youtube:channel/UCExampleMathsChannel001",
                    "snippet": null,
                    "duration_seconds": null,
                    "live_status": null,
                    "views": null,
                    "likes": null,
                    "comments": null,
                    "language": null,
                    "has_captions": null,
                    "caption_tracks": null,
                    "position": 1
                }
            ]),
            Vec::<u64>::new(),
            None,
        )
    );
}

#[test]
fn private_and_deleted_entries_are_skipped_with_one_diagnostic() {
    let request = YoutubeVideosRequest {
        collection: YoutubeCollection::Ref(playlist()),
        limit: 20,
        page: None,
    };

    let (outcome, _) = videos(
        &request,
        vec![ok_json(&playlist_items_page(
            &[
                unreadable_playlist_item(PLAYLIST, 0, "aaaaaaaaaaa", "Private video"),
                playlist_item(PLAYLIST, 1, "k7RM-ot2NWY", "Chapter 2", ""),
                unreadable_playlist_item(PLAYLIST, 2, "bbbbbbbbbbb", "Deleted video"),
            ],
            None,
        ))],
    );
    let outcome = outcome.expect("the readable entries are listed");

    assert_eq!(
        (
            outcome
                .items
                .iter()
                .map(|item| to_json(&item.data)["position"].clone())
                .collect::<Vec<_>>(),
            outcome.diagnostic.as_deref(),
        ),
        (
            vec![json!(1)],
            Some("skipped private or deleted videos at playlist positions 0, 2"),
        )
    );
}

#[test]
fn a_handle_resolves_to_its_channel_and_the_next_page_names_its_uploads_playlist() {
    let request = YoutubeVideosRequest {
        collection: YoutubeCollection::Handle("@examplemaths".to_owned()),
        limit: 1,
        page: None,
    };

    let (outcome, requests) = videos(
        &request,
        vec![
            ok_json(&sample_json("youtube_api/channels.json")),
            ok_json(&playlist_items_page(
                &[playlist_item(UPLOADS, 0, "LKrwhjOI8Nc", "Newest", "")],
                Some("EAAaBlBUOkNBRQ"),
            )),
        ],
    );
    let outcome = outcome.expect("the handle's uploads are listed");

    assert_eq!(
        (
            sent(&requests),
            outcome.collection,
            to_json(&outcome.items[0].reference),
            outcome.next_page,
        ),
        (
            vec![
                (
                    "/youtube/v3/channels".to_owned(),
                    pairs(&[("part", "contentDetails"), ("forHandle", "@examplemaths")]),
                ),
                (
                    "/youtube/v3/playlistItems".to_owned(),
                    pairs(&[
                        ("part", "snippet,contentDetails"),
                        ("playlistId", UPLOADS),
                        ("maxResults", "1"),
                    ]),
                ),
            ],
            channel(),
            json!("youtube:LKrwhjOI8Nc"),
            Some(format!("{UPLOADS}:EAAaBlBUOkNBRQ")),
        )
    );
}

#[test]
fn a_later_channel_page_reads_its_uploads_playlist_without_another_lookup() {
    let request = YoutubeVideosRequest {
        collection: YoutubeCollection::Ref(channel()),
        limit: 1,
        page: Some(format!("{UPLOADS}:EAAaBlBUOkNBRQ")),
    };

    let (outcome, requests) = videos(
        &request,
        vec![ok_json(&playlist_items_page(
            &[playlist_item(UPLOADS, 1, "ausLKMojXaY", "Older", "")],
            None,
        ))],
    );
    let outcome = outcome.expect("the later page is listed");

    assert_eq!(
        (
            sent(&requests),
            outcome.collection,
            to_json(&outcome.items[0].reference),
            outcome.next_page,
        ),
        (
            vec![(
                "/youtube/v3/playlistItems".to_owned(),
                pairs(&[
                    ("part", "snippet,contentDetails"),
                    ("playlistId", UPLOADS),
                    ("maxResults", "1"),
                    ("pageToken", "EAAaBlBUOkNBRQ"),
                ]),
            )],
            channel(),
            json!("youtube:ausLKMojXaY"),
            None,
        )
    );
}

#[test]
fn a_handle_nobody_owns_is_an_attempt_level_parameter_failure() {
    let request = YoutubeVideosRequest {
        collection: YoutubeCollection::Handle("@nobody-owns-this".to_owned()),
        limit: 20,
        page: None,
    };

    let (outcome, _) = videos(&request, vec![ok_json(&channels_page(&[]))]);

    assert_eq!(
        failure(outcome),
        (
            AttemptErrorKind::Parameter,
            "YouTube channel not found: handle @nobody-owns-this".to_owned()
        )
    );
}
