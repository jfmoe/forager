mod support;

use serde_json::json;

use support::youtube::{
    KEY, LINEAR_ALGEBRA, config, header, pairs, payload, playlist_item, playlist_items_page, stderr,
};
use support::{Fixture, RunEnvironment};
use support::{path, query};

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
                ],
                "next_cursor": null
            }),
        ),
        "stderr: {}",
        stderr(&output)
    );
}
