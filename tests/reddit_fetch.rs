mod support;

use std::fs;
use std::process::Output;

use serde_json::{Value, json};

use support::reddit::{
    Comment, EmbedPost, comments_page, config, embed_page, missing_comments_page, pairs, path,
    payload, permalink_path, query, run, stderr, unavailable_embed_page,
};
use support::{Fixture, RunEnvironment};

const POST: &str = "1w2pcmd";

fn fetch(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["fetch"];
    command.extend_from_slice(arguments);
    run(environment, &command)
}

fn failure(output: &Output) -> (Option<i32>, Value, String) {
    let failure = payload(output);
    (
        output.status.code(),
        failure["error_kind"].clone(),
        failure["message"].as_str().unwrap_or_default().to_owned(),
    )
}

/// The responses of a fetch of a post with `comments` of 12 and the embed page of `post`.
fn post_pages(post: &EmbedPost<'_>, comments: &[Comment<'_>]) -> Vec<support::Response> {
    vec![
        comments_page(POST, &permalink_path(POST), 12, comments, true),
        embed_page(post),
    ]
}

/// A top-level comment with a reply, a deleted reply that keeps its own reply, and a second
/// top-level comment.
fn discussion() -> Vec<Comment<'static>> {
    vec![
        Comment::new(
            "c1",
            0,
            "alice",
            "<p>First <strong>bold</strong> point</p><p>Second paragraph</p>",
        )
        .with_replies(vec![
            Comment::new("c2", 1, "bob", "<p>A reply</p>"),
            Comment::deleted("c3", 1).with_replies(vec![Comment::new(
                "c4",
                2,
                "carol",
                "<p>Under a deleted one</p>",
            )]),
        ]),
        Comment {
            score: -3,
            ..Comment::new(
                "c5",
                0,
                "dave",
                "<blockquote><p>quoted</p></blockquote><p>See <a href=\"/r/rust/wiki\">the wiki</a></p>",
            )
        },
    ]
}

/// The heading line of fixture comment `id` at `depth`.
fn heading(depth: usize, author: &str, score: i64, id: &str) -> String {
    format!(
        "\n{}- **{author}** · {score} points · 2026-08-30T19:48:51Z · [t1_{id}](https://www.reddit.com/r/rust/comments/{POST}/comment/{id}/)\n\n",
        "  ".repeat(depth)
    )
}

#[test]
fn thread_writes_the_post_and_its_first_page_of_top_comments() {
    let post = EmbedPost::text(
        POST,
        "<p>I have been a C programmer.</p><ul><li>one</li><li>two</li></ul>",
    );
    let reddit = Fixture::start_sequence(post_pages(&post, &discussion()));
    let environment = RunEnvironment::new(&config(&reddit.url));
    let content_dir = tempfile::tempdir().expect("content directory");

    let output = fetch(
        &environment,
        &[
            "https://old.reddit.com/r/rust/comments/1W2PCMD/learning_async_rust/",
            "--content-dir",
            content_dir.path().to_str().expect("UTF-8 path"),
        ],
    );
    let requests = reddit.finish_all();
    let mut result = payload(&output);
    let content_path = result["content_path"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    result["content_path"] = Value::Null;
    result["content_len"] = Value::Null;
    let written = fs::read_to_string(&content_path).unwrap_or_default();
    let expected = [
        "# Learning async rust\n\n- Ref: `reddit:1w2pcmd` <https://www.reddit.com/comments/1w2pcmd/>\n- Permalink: <https://www.reddit.com/r/rust/comments/1w2pcmd/learning_async_rust/>\n- Subreddit: r/rust\n- Author: u/gamer_redditor\n- Score: 389\n- Comments: 12\n- Published: 2026-08-30T18:31:14Z\n\nI have been a C programmer.\n\n- one\n- two\n\n## Comments\n\nThe first page of comments in Reddit's top order: 5 of 12 comments; Reddit has more that this file does not hold.\n".to_owned(),
        heading(0, "u/alice", 10, "c1"),
        "  First **bold** point\n\n  Second paragraph\n".to_owned(),
        heading(1, "u/bob", 10, "c2"),
        "    A reply\n".to_owned(),
        heading(1, "[deleted]", 1, "c3"),
        "    *[deleted or removed]*\n".to_owned(),
        heading(2, "u/carol", 10, "c4"),
        "      Under a deleted one\n".to_owned(),
        heading(0, "u/dave", -3, "c5"),
        "  > quoted\n\n  See [the wiki](https://www.reddit.com/r/rust/wiki)\n".to_owned(),
    ]
    .concat();

    assert_eq!(
        (
            output.status.code(),
            requests
                .iter()
                .map(|request| path(request))
                .collect::<Vec<_>>(),
            query(&requests[0]),
            content_path,
        ),
        (
            Some(0),
            vec![
                format!("/svc/shreddit/comments/r/all/t3_{POST}"),
                permalink_path(POST),
            ],
            pairs(&[("sort", "top")]),
            content_dir
                .path()
                .join("reddit-1w2pcmd.md")
                .display()
                .to_string(),
        ),
        "stderr: {}",
        stderr(&output)
    );
    assert_eq!(written, expected);
    assert_eq!(
        result,
        json!({
            "platform": "reddit",
            "provider": "reddit_web",
            "ref": "reddit:1w2pcmd",
            "url": "https://www.reddit.com/comments/1w2pcmd/",
            "depth": "thread",
            "title": "Learning async rust",
            "authors": ["gamer_redditor"],
            "published": "2026-08-30T18:31:14Z",
            "subreddit": "rust",
            "permalink": "https://www.reddit.com/r/rust/comments/1w2pcmd/learning_async_rust/",
            "post_type": "text",
            "link_url": null,
            "score": 389,
            "comments": 12,
            "nsfw": false,
            "spoiler": null,
            "snippet": null,
            "snippet_source": null,
            "text": "I have been a C programmer.\n\n- one\n- two",
            "comments_delivered": 5,
            "has_more_comments": true,
            "order": "top",
            "content_url": "https://www.reddit.com/comments/1w2pcmd/",
            "content_provider": "reddit_web",
            "content_path": null,
            "content_len": null
        })
    );
}

#[test]
fn metadata_of_a_link_post_names_its_link_and_writes_no_file() {
    let post = EmbedPost {
        post_type: "link",
        url: Some("https://blog.example/async".to_owned()),
        body: None,
        ..EmbedPost::text(POST, "")
    };
    let reddit = Fixture::start_sequence(post_pages(&post, &[]));
    let environment = RunEnvironment::new(&config(&reddit.url));

    let output = fetch(
        &environment,
        &["https://redd.it/1w2pcmd", "--depth", "metadata"],
    );
    reddit.finish_all();
    let result = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            [
                &result["depth"],
                &result["post_type"],
                &result["link_url"],
                &result["text"],
                &result["comments"],
                &result["content_path"],
            ],
        ),
        (
            Some(0),
            [
                &json!("metadata"),
                &json!("link"),
                &json!("https://blog.example/async"),
                &Value::Null,
                &json!(12),
                &Value::Null,
            ],
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn the_full_text_of_a_text_post_is_its_own_title_and_body() {
    let post = EmbedPost::text(POST, "<p>Body with <em>emphasis</em>.</p>");
    let reddit = Fixture::start_sequence(post_pages(&post, &discussion()));
    let environment = RunEnvironment::new(&config(&reddit.url));

    let output = fetch(
        &environment,
        &[
            "reddit:t3_1w2pcmd",
            "--depth",
            "full_text",
            "--format",
            "content",
        ],
    );
    reddit.finish_all();

    assert_eq!(
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
        ),
        (
            Some(0),
            "# Learning async rust\n\nBody with *emphasis*.\n".to_owned()
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn an_image_post_without_text_has_no_full_text() {
    let post = EmbedPost {
        post_type: "image",
        url: Some("https://i.redd.it/fixture.png".to_owned()),
        body: None,
        ..EmbedPost::text(POST, "")
    };
    let reddit = Fixture::start_sequence(post_pages(&post, &[]));
    let environment = RunEnvironment::new(&config(&reddit.url));

    let output = fetch(&environment, &["reddit:1w2pcmd", "--depth", "full_text"]);
    reddit.finish_all();

    assert_eq!(
        failure(&output),
        (
            Some(5),
            json!("quality"),
            "reddit:1w2pcmd is a post of type `image` without text of its own; its media is at https://i.redd.it/fixture.png; use --depth thread to read its discussion".to_owned()
        )
    );
}

#[test]
fn a_missing_post_is_a_parameter_failure_after_one_request() {
    let reddit = Fixture::start_sequence(vec![missing_comments_page()]);
    let environment = RunEnvironment::new(&config(&reddit.url));

    let output = fetch(&environment, &["reddit:zzzzzz"]);
    reddit.finish_all();

    assert_eq!(
        failure(&output),
        (
            Some(4),
            json!("parameter"),
            "Reddit post not found: reddit:zzzzzz".to_owned()
        )
    );
}

#[test]
fn a_post_its_embed_page_does_not_show_is_a_parameter_failure() {
    let reddit = Fixture::start_sequence(vec![
        comments_page(POST, &permalink_path(POST), 0, &[], false),
        unavailable_embed_page(POST),
    ]);
    let environment = RunEnvironment::new(&config(&reddit.url));

    let output = fetch(&environment, &["reddit:1w2pcmd"]);
    reddit.finish_all();
    let (code, kind, message) = failure(&output);

    assert_eq!((code, kind), (Some(4), json!("parameter")), "{message}");
    assert!(
        message.contains("removed, private, or restricted"),
        "{message}"
    );
}

#[test]
fn a_comment_page_of_another_post_is_a_runtime_failure() {
    let reddit = Fixture::start_sequence(vec![comments_page(
        "other1",
        &permalink_path("other1"),
        0,
        &[],
        false,
    )]);
    let environment = RunEnvironment::new(&config(&reddit.url));

    let output = fetch(&environment, &["reddit:1w2pcmd"]);
    reddit.finish_all();

    assert_eq!(
        failure(&output),
        (
            Some(4),
            json!("runtime"),
            "the Reddit comment page shows t3_other1 instead of t3_1w2pcmd".to_owned()
        )
    );
}

#[test]
fn comment_links_share_links_and_unsupported_depths_fail_before_any_request() {
    let reddit = Fixture::start_canary();
    let environment = RunEnvironment::new(&config(&reddit.url));

    let outputs = [
        fetch(
            &environment,
            &["https://www.reddit.com/r/rust/comments/1w2pcmd/x/p6uppg7/"],
        ),
        fetch(&environment, &["https://www.reddit.com/r/rust/s/AbCdEf"]),
        fetch(&environment, &["reddit:1w2pcmd", "--depth", "abstract"]),
    ];

    assert_eq!(
        (
            outputs.each_ref().map(|output| output.status.code()),
            reddit.finish_all().len(),
        ),
        ([Some(2), Some(2), Some(2)], 0)
    );
    assert!(
        stderr(&outputs[0]).contains("fetch reddit:1w2pcmd to read the post and its discussion"),
        "{}",
        stderr(&outputs[0])
    );
}
