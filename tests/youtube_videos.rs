mod support;

use serde_json::json;

use support::youtube::{
    KEY, LINEAR_ALGEBRA, THREE_BLUE_ONE_BROWN, THREE_BLUE_ONE_BROWN_UPLOADS, channels_page, config,
    header, pairs, path, payload, playlist_item, playlist_items_page, query, stderr,
    unreadable_playlist_item,
};
use support::{Fixture, RunEnvironment};

fn videos(environment: &RunEnvironment, arguments: &[&str]) -> std::process::Output {
    let mut command = vec!["platform", "youtube", "videos"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

#[test]
fn a_playlist_lists_its_videos_with_their_position_and_video_publication_time() {
    let api = Fixture::start_sequence(vec![playlist_items_page(
        &[
            playlist_item(
                LINEAR_ALGEBRA,
                0,
                "fNk_zzaMoSs",
                "Vectors | Chapter 1, Essence of linear algebra",
                "Beginning the linear algebra series with the basics.",
            ),
            playlist_item(
                LINEAR_ALGEBRA,
                1,
                "k7RM-ot2NWY",
                "Linear combinations, span, and basis vectors | Chapter 2",
                "",
            ),
        ],
        None,
    )]);
    let environment = RunEnvironment::new(&config(&api.url));

    let output = videos(
        &environment,
        &[
            &format!("https://www.youtube.com/playlist?list={LINEAR_ALGEBRA}"),
            "--limit",
            "2",
        ],
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
            "/youtube/v3/playlistItems".to_owned(),
            pairs(&[
                ("part", "snippet,contentDetails"),
                ("playlistId", LINEAR_ALGEBRA),
                ("maxResults", "2"),
            ]),
            Some(KEY.to_owned()),
            json!({
                "platform": "youtube",
                "provider": "youtube_api",
                "collection": {
                    "ref": format!("youtube:playlist/{LINEAR_ALGEBRA}"),
                    "url": format!("https://www.youtube.com/playlist?list={LINEAR_ALGEBRA}")
                },
                "items": [
                    {
                        "ref": "youtube:fNk_zzaMoSs",
                        "url": "https://www.youtube.com/watch?v=fNk_zzaMoSs",
                        "depth": "snippet",
                        "title": "Vectors | Chapter 1, Essence of linear algebra",
                        "authors": ["3Blue1Brown"],
                        "published": "2016-08-06T00:05:49Z",
                        "channel_ref": "youtube:channel/UCYO_jab_esuFRV4b17AJtAw",
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
                        "authors": ["3Blue1Brown"],
                        "published": "2016-08-06T00:05:49Z",
                        "channel_ref": "youtube:channel/UCYO_jab_esuFRV4b17AJtAw",
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
                ],
                "next_cursor": null
            }),
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_handle_resolves_to_its_channel_whose_uploads_the_cursor_continues_without_another_lookup() {
    let uploads = THREE_BLUE_ONE_BROWN_UPLOADS;
    let api = Fixture::start_sequence(vec![
        channels_page(&[(THREE_BLUE_ONE_BROWN, uploads)]),
        playlist_items_page(
            &[playlist_item(uploads, 0, "LKrwhjOI8Nc", "Newest", "")],
            Some("EAAaBlBUOkNBRQ"),
        ),
        playlist_items_page(
            &[playlist_item(uploads, 1, "ausLKMojXaY", "Older", "")],
            None,
        ),
    ]);
    let environment = RunEnvironment::new(&config(&api.url));

    let first = videos(
        &environment,
        &[
            "https://www.youtube.com/@3blue1brown/videos",
            "--limit",
            "1",
        ],
    );
    let first_page = payload(&first);
    let cursor = first_page["next_cursor"].as_str().unwrap_or_default();
    let second = videos(&environment, &["--cursor", cursor]);
    let second_page = payload(&second);
    let requests = api.finish_all();
    let sent = requests
        .iter()
        .map(|request| (path(request), query(request)))
        .collect::<Vec<_>>();
    let channel = json!({
        "ref": format!("youtube:channel/{THREE_BLUE_ONE_BROWN}"),
        "url": format!("https://www.youtube.com/channel/{THREE_BLUE_ONE_BROWN}")
    });

    assert_eq!(
        (
            first.status.code(),
            second.status.code(),
            cursor.starts_with("v1.youtube_api."),
            sent,
            [&first_page["collection"], &second_page["collection"]],
            [
                &first_page["items"][0]["ref"],
                &second_page["items"][0]["ref"]
            ],
            &second_page["next_cursor"],
        ),
        (
            Some(0),
            Some(0),
            true,
            vec![
                (
                    "/youtube/v3/channels".to_owned(),
                    pairs(&[("part", "contentDetails"), ("forHandle", "@3blue1brown")]),
                ),
                (
                    "/youtube/v3/playlistItems".to_owned(),
                    pairs(&[
                        ("part", "snippet,contentDetails"),
                        ("playlistId", uploads),
                        ("maxResults", "1"),
                    ]),
                ),
                (
                    "/youtube/v3/playlistItems".to_owned(),
                    pairs(&[
                        ("part", "snippet,contentDetails"),
                        ("playlistId", uploads),
                        ("maxResults", "1"),
                        ("pageToken", "EAAaBlBUOkNBRQ"),
                    ]),
                ),
            ],
            [&channel, &channel],
            [&json!("youtube:LKrwhjOI8Nc"), &json!("youtube:ausLKMojXaY")],
            &json!(null),
        ),
        "stderr: {}{}",
        stderr(&first),
        stderr(&second)
    );
}

#[test]
fn private_and_deleted_entries_are_skipped_with_one_diagnostic() {
    let api = Fixture::start_sequence(vec![playlist_items_page(
        &[
            unreadable_playlist_item(LINEAR_ALGEBRA, 0, "aaaaaaaaaaa", "Private video"),
            playlist_item(LINEAR_ALGEBRA, 1, "k7RM-ot2NWY", "Chapter 2", ""),
            unreadable_playlist_item(LINEAR_ALGEBRA, 2, "bbbbbbbbbbb", "Deleted video"),
        ],
        None,
    )]);
    let environment = RunEnvironment::new(&config(&api.url));

    let output = videos(
        &environment,
        &[&format!("youtube:playlist/{LINEAR_ALGEBRA}")],
    );
    api.finish();
    let page = payload(&output);
    let positions = page["items"].as_array().map(|items| {
        items
            .iter()
            .map(|item| item["position"].clone())
            .collect::<Vec<_>>()
    });

    assert_eq!(
        (
            output.status.code(),
            positions,
            stderr(&output)
                .contains("skipped private or deleted videos at playlist positions 0, 2"),
        ),
        (Some(0), Some(vec![json!(1)]), true),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_handle_nobody_owns_is_an_attempt_level_parameter_failure() {
    let api = Fixture::start_sequence(vec![channels_page(&[])]);
    let environment = RunEnvironment::new(&config(&api.url));

    let output = videos(&environment, &["https://www.youtube.com/@nobody-owns-this"]);
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
            &json!("YouTube channel not found: handle @nobody-owns-this")
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_video_or_an_unknown_url_is_refused_before_any_request() {
    let environment = RunEnvironment::new(&config("http://127.0.0.1:9"));

    let codes = [
        "youtube:jNQXAC9IVRw",
        "https://www.youtube.com/c/3blue1brown",
    ]
    .map(|input| videos(&environment, &[input]).status.code());

    assert_eq!(codes, [Some(2), Some(2)]);
}
