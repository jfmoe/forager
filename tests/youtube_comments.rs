mod support;

use serde_json::json;

use support::youtube::{
    KEY, api_error, comment, comment_thread, comment_threads_page, config, header, pairs, path,
    payload, query, stderr,
};
use support::{Fixture, RunEnvironment};

fn comments(environment: &RunEnvironment, arguments: &[&str]) -> std::process::Output {
    let mut command = vec!["platform", "youtube", "comments"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

#[test]
fn comments_carry_their_authors_counts_and_replies_and_say_whether_the_replies_are_complete() {
    let api = Fixture::start_sequence(vec![comment_threads_page(
        &[
            comment_thread(
                &comment("UgzTop1", "elephantfan", "The first video ever.", 4_866_586),
                0,
                &[],
            ),
            comment_thread(
                &comment("UgzTop2", "zoofan", "Those trunks are long.", 94),
                1000,
                &[comment("UgzTop2.r1", "replier", "Really long.", 1)],
            ),
        ],
        Some("QURTSl9pMk"),
    )]);
    let environment = RunEnvironment::new(&config(&api.url));

    let output = comments(
        &environment,
        &[
            "https://www.youtube.com/watch?v=jNQXAC9IVRw&t=4",
            "--limit",
            "2",
            "--sort",
            "time",
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
            "/youtube/v3/commentThreads".to_owned(),
            pairs(&[
                ("part", "snippet,replies"),
                ("videoId", "jNQXAC9IVRw"),
                ("textFormat", "plainText"),
                ("maxResults", "2"),
                ("order", "time"),
            ]),
            Some(KEY.to_owned()),
            false,
            json!({
                "platform": "youtube",
                "provider": "youtube_api",
                "video": "youtube:jNQXAC9IVRw",
                "comments": [
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
                ],
                "has_more": true
            }),
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn disabled_comments_are_an_attempt_level_parameter_failure_not_an_empty_list() {
    let api = Fixture::start_sequence(vec![api_error(
        403,
        "commentsDisabled",
        None,
        "The video identified by the <code>videoId</code> parameter has disabled comments.",
    )]);
    let environment = RunEnvironment::new(&config(&api.url));

    let output = comments(&environment, &["youtube:XqZsoesa55w"]);
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
            &json!("comments are disabled for youtube:XqZsoesa55w")
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_video_without_comments_lists_none_and_succeeds() {
    let api = Fixture::start_sequence(vec![comment_threads_page(&[], None)]);
    let environment = RunEnvironment::new(&config(&api.url));

    let output = comments(&environment, &["youtube:jNQXAC9IVRw"]);
    let request = api.finish();

    assert_eq!(
        (
            output.status.code(),
            query(&request)["order"].as_str(),
            payload(&output)
        ),
        (
            Some(0),
            "relevance",
            json!({
                "platform": "youtube",
                "provider": "youtube_api",
                "video": "youtube:jNQXAC9IVRw",
                "comments": [],
                "has_more": false
            })
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_playlist_or_channel_is_refused_before_any_request() {
    let environment = RunEnvironment::new(&config("http://127.0.0.1:9"));

    let codes = [
        "youtube:playlist/PLZHQObOWTQDPD3MizzM2xVFitgF8hE_ab",
        "youtube:channel/UCYO_jab_esuFRV4b17AJtAw",
    ]
    .map(|reference| comments(&environment, &[reference]).status.code());

    assert_eq!(codes, [Some(2), Some(2)]);
}
