mod support;

use std::process::Output;

use serde_json::{Value, json};

use support::reddit::{
    SearchResult, challenge_page, config, no_results_page, pairs, path, payload, query, run,
    search_page, stderr,
};
use support::{Fixture, Response, RunEnvironment};

fn search(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["search"];
    command.extend_from_slice(arguments);
    run(environment, &command)
}

fn refs(result: &Value) -> Vec<String> {
    result["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| item["ref"].as_str().unwrap_or_default().to_owned())
                .collect()
        })
        .unwrap_or_default()
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
fn results_carry_their_subreddit_score_comments_and_search_excerpt() {
    let reddit = Fixture::start_sequence(vec![search_page(
        &[
            SearchResult {
                snippet: Some(("Start with the Tokio tutorial.", "t1_ovnjfji")),
                ..SearchResult::new("1unx4wf", "Guidance on Async Rust")
            },
            SearchResult {
                snippet: Some(("Async & await, visually", "t3_1wboo7g")),
                votes: 0,
                comments: 6,
                ..SearchResult::new("1wboo7g", "A visual guide to Rust async")
            },
            SearchResult {
                subreddit: "programming",
                author: "gamer_redditor",
                votes: 389,
                comments: 94,
                ..SearchResult::new("1w2pcmd", "Learning async rust")
            },
        ],
        None,
    )]);
    let environment = RunEnvironment::new(&config(&reddit.url));

    let output = search(
        &environment,
        &["rust async", "--sort", "top", "--time", "week"],
    );
    let requests = reddit.finish_all();

    assert_eq!(
        (
            output.status.code(),
            path(&requests[0]),
            query(&requests[0]),
        ),
        (
            Some(0),
            "/svc/shreddit/search/".to_owned(),
            pairs(&[
                ("q", "rust async"),
                ("type", "posts"),
                ("sort", "top"),
                ("t", "week")
            ]),
        ),
        "stderr: {}",
        stderr(&output)
    );
    let item = |id: &str, title: &str, slug: &str, depth: &str, fields: Value| {
        let mut item = json!({
            "ref": format!("reddit:{id}"),
            "url": format!("https://www.reddit.com/comments/{id}/"),
            "depth": depth,
            "title": title,
            "authors": ["ferris"],
            "published": "2026-07-05T08:48:25Z",
            "subreddit": "rust",
            "permalink": format!("https://www.reddit.com/r/rust/comments/{id}/{slug}/"),
            "post_type": null,
            "link_url": null,
            "score": 2,
            "comments": 10,
            "nsfw": false,
            "spoiler": false,
            "snippet": null,
            "snippet_source": null
        });
        for (key, value) in fields.as_object().expect("fields object") {
            item[key] = value.clone();
        }
        item
    };
    assert_eq!(
        payload(&output),
        json!({
            "platform": "reddit",
            "provider": "reddit_web",
            "items": [
                item("1unx4wf", "Guidance on Async Rust", "guidance_on_async_rust", "snippet", json!({
                    "snippet": "Start with the Tokio tutorial.",
                    "snippet_source": "comment"
                })),
                item("1wboo7g", "A visual guide to Rust async", "a_visual_guide_to_rust_async", "snippet", json!({
                    "score": 0,
                    "comments": 6,
                    "snippet": "Async & await, visually",
                    "snippet_source": "post"
                })),
                item("1w2pcmd", "Learning async rust", "learning_async_rust", "metadata", json!({
                    "authors": ["gamer_redditor"],
                    "subreddit": "programming",
                    "permalink": "https://www.reddit.com/r/programming/comments/1w2pcmd/learning_async_rust/",
                    "score": 389,
                    "comments": 94
                })),
            ],
            "next_cursor": null
        })
    );
}

#[test]
fn a_subreddit_search_reads_that_subreddits_search_fragment() {
    let reddit = Fixture::start_sequence(vec![no_results_page()]);
    let environment = RunEnvironment::new(&config(&reddit.url));

    let output = search(
        &environment,
        &["tokio", "--subreddit", "r/rust", "--sort", "new"],
    );
    let requests = reddit.finish_all();

    assert_eq!(
        (
            output.status.code(),
            path(&requests[0]),
            query(&requests[0]),
            payload(&output)["items"].clone(),
        ),
        (
            Some(0),
            "/svc/shreddit/r/rust/search/".to_owned(),
            pairs(&[("q", "tokio"), ("type", "posts"), ("sort", "new")]),
            json!([]),
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_search_reads_on_until_its_limit_and_its_cursor_resumes_inside_the_last_page() {
    let first = ["a1", "a2", "a3"].map(|id| SearchResult::new(id, "First page"));
    let second = ["b1", "b2", "b3"].map(|id| SearchResult::new(id, "Second page"));
    let reddit = Fixture::start_sequence(vec![
        search_page(&first, Some("c2")),
        search_page(&second, None),
        search_page(&second, None),
    ]);
    let environment = RunEnvironment::new(&config(&reddit.url));

    let page = search(&environment, &["rust", "--limit", "4"]);
    let page = payload(&page);
    let cursor = page["next_cursor"].as_str().unwrap_or_default().to_owned();
    let next = search(&environment, &["--cursor", &cursor]);
    let requests = reddit.finish_all();
    let next = payload(&next);

    assert_eq!(
        (
            refs(&page),
            refs(&next),
            next["next_cursor"].clone(),
            requests
                .iter()
                .map(|request| query(request).get("cursor").cloned())
                .collect::<Vec<_>>(),
        ),
        (
            ["a1", "a2", "a3", "b1"]
                .map(|id| format!("reddit:{id}"))
                .to_vec(),
            ["b2", "b3"].map(|id| format!("reddit:{id}")).to_vec(),
            Value::Null,
            vec![None, Some("c2".to_owned()), Some("c2".to_owned())],
        )
    );
    assert!(cursor.starts_with("v1.reddit_web."), "{cursor}");
}

#[test]
fn a_verification_page_is_an_auth_failure_and_is_never_answered() {
    let reddit = Fixture::start_sequence(vec![challenge_page()]);
    let environment = RunEnvironment::new(&config(&reddit.url));

    let output = search(&environment, &["rust"]);
    reddit.finish_all();
    let (code, kind, message) = failure(&output);

    assert_eq!((code, kind), (Some(4), json!("auth")), "{message}");
    assert!(
        message.contains("verification page") && message.contains("egress IP"),
        "{message}"
    );
}

#[test]
fn a_refused_request_is_not_retried_and_names_the_egress_ip() {
    let reddit = Fixture::start_sequence(vec![Response::new(
        403,
        "text/html",
        "<html>Blocked</html>",
    )]);
    let environment = RunEnvironment::new(&config(&reddit.url));

    let output = search(&environment, &["rust"]);
    let requests = reddit.finish_all();
    let (code, _, message) = failure(&output);

    assert_eq!((code, requests.len()), (Some(4), 1), "{message}");
    assert!(
        message.contains("HTTP 403") && message.contains("egress IP"),
        "{message}"
    );
}

#[test]
fn a_page_that_is_not_a_results_page_is_a_runtime_failure() {
    let reddit = Fixture::start_sequence(vec![support::reddit::html(
        "<html><body>Welcome</body></html>",
    )]);
    let environment = RunEnvironment::new(&config(&reddit.url));

    let output = search(&environment, &["rust"]);
    reddit.finish_all();
    let (code, kind, message) = failure(&output);

    assert_eq!((code, kind), (Some(4), json!("runtime")), "{message}");
    assert!(
        message.contains("page format may have changed"),
        "{message}"
    );
}

#[test]
fn the_route_is_off_by_default_and_invalid_options_send_no_request() {
    let reddit = Fixture::start_canary();
    let disabled = RunEnvironment::new(&format!(
        "[providers.reddit_web]\nurl = \"{}\"\n",
        reddit.url
    ));
    let enabled = RunEnvironment::new(&config(&reddit.url));

    let off = search(&disabled, &["rust"]);
    let time_with_new = search(&enabled, &["rust", "--sort", "new", "--time", "week"]);
    let bad_subreddit = search(&enabled, &["rust", "--subreddit", "no spaces"]);
    let empty_query = search(&enabled, &["  "]);

    assert_eq!(
        (
            off.status.code(),
            time_with_new.status.code(),
            bad_subreddit.status.code(),
            empty_query.status.code(),
            reddit.finish_all().len(),
        ),
        (Some(3), Some(2), Some(2), Some(2), 0)
    );
    let message = stderr(&off);
    assert!(
        message.contains("never your account")
            && message.contains(r#"forager config set platforms.reddit.order '["reddit_web"]'"#),
        "{message}"
    );
}
