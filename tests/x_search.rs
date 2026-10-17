//! `forager platform x search` against a local xAI Responses fixture built from the live
//! responses of 2026-10-17.

mod support;

use std::process::Output;

use serde_json::{Value, json};

use support::x::{
    completed, config, fixture, payload, refs, request, stderr, terminal, with_posts,
};
use support::{Fixture, Response, RunEnvironment};

const KEY: &str = "test-xai-key-a";

fn search(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "x", "search"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

/// Runs one search against a fixture that answers in order and returns every request.
fn run(arguments: &[&str], responses: Vec<Response>) -> (Output, Vec<String>) {
    let fixture = Fixture::start_sequence(responses);
    let environment = RunEnvironment::new(&config(&fixture.url, &[KEY]));
    let output = search(&environment, arguments);
    (output, fixture.finish_all())
}

#[test]
fn search_sends_only_the_x_search_tool_with_the_filters_and_structured_output() {
    let (output, requests) = run(
        &[
            "Rust 2027 edition",
            "--handle",
            "@rustlang",
            "--handle",
            "RustLang",
            "--handle",
            "nikomatsakis",
            "--from",
            "2026-09-01",
            "--to",
            "2026-10-15",
            "--limit",
            "5",
        ],
        vec![completed(&fixture("search.json"))],
    );
    let (target, body) = request(&requests[0]);

    assert_eq!(
        (
            output.status.code(),
            target.as_str(),
            &body["model"],
            &body["stream"],
            &body["tools"],
            &body["text"]["format"]["type"],
            &body["text"]["format"]["strict"],
        ),
        (
            Some(0),
            "/responses",
            &json!("grok-test"),
            &json!(true),
            &json!([{
                "type": "x_search",
                "allowed_x_handles": ["rustlang", "nikomatsakis"],
                "from_date": "2026-09-01",
                "to_date": "2026-10-16"
            }]),
            &json!("json_schema"),
            &json!(true),
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn verified_posts_become_snippet_items_dated_by_their_id() {
    let (output, _) = run(
        &["Rust 2027 edition"],
        vec![completed(&fixture("search.json"))],
    );
    let page = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &page["platform"],
            &page["provider"],
            &page["next_cursor"]
        ),
        (Some(0), &json!("x"), &json!("xai"), &Value::Null),
        "stderr: {}",
        stderr(&output)
    );
    assert_eq!(
        page["items"][0],
        json!({
            "ref": "x:2106088717547077910",
            "url": "https://x.com/i/status/2106088717547077910",
            "depth": "snippet",
            "title": null,
            "authors": ["loncothad"],
            "published": "2026-10-02T18:27:34Z",
            "author_handle": "loncothad",
            "text": "Rust developers (of The Rust, not working With) are on the generation run to make the Edition 2027 the dream language. Wow. But I can't see the reason why - AI is banned there and it's not like any new cash grants were given to them... Just a random lock in, it seems..."
        })
    );
    assert_eq!(
        refs(&page),
        ["x:2106088717547077910", "x:2110286468623351937"]
    );
}

#[test]
fn posts_the_search_did_not_read_are_dropped_with_one_diagnostic() {
    let response = with_posts(
        &fixture("search.json"),
        &json!([
            {"url": "https://x.com/loncothad/status/2106088717547077910", "author_handle": "loncothad", "text": "read"},
            {"url": "https://x.com/someone/status/2110000000000000000", "author_handle": "someone", "text": "invented"},
            {"url": "https://example.com/post", "author_handle": "someone", "text": "elsewhere"}
        ]),
        &[
            "https://x.com/i/status/2106088717547077910",
            "https://x.com/i/user/1112661938608078848",
        ],
    );

    let (output, _) = run(&["Rust 2027 edition"], vec![completed(&response)]);

    assert_eq!(
        (output.status.code(), refs(&payload(&output))),
        (Some(0), vec!["x:2106088717547077910".to_owned()])
    );
    assert!(
        stderr(&output).contains(
            "xai dropped 2 of 3 X posts it could not verify: https://x.com/someone/status/2110000000000000000 (not among the posts the search read); https://example.com/post (not an X post URL)"
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn posts_dated_outside_the_requested_days_are_dropped() {
    // 2106088717547077910 dates to 2026-10-02 and 2110286468623351937 to 2026-10-14.
    let (output, _) = run(
        &[
            "Rust 2027 edition",
            "--from",
            "2026-10-03",
            "--to",
            "2026-10-14",
        ],
        vec![completed(&fixture("search.json"))],
    );

    assert_eq!(
        (output.status.code(), refs(&payload(&output))),
        (Some(0), vec!["x:2110286468623351937".to_owned()]),
        "stderr: {}",
        stderr(&output)
    );
    assert!(stderr(&output).contains("(published outside the requested dates)"));
}

#[test]
fn repeated_posts_appear_once_and_the_list_stops_at_the_limit() {
    let post = |id: &str| json!({"url": format!("https://x.com/a/status/{id}"), "author_handle": "a", "text": id});
    let response = with_posts(
        &fixture("search.json"),
        &json!([
            post("2106088717547077910"),
            post("2106088717547077910"),
            post("2110286468623351937")
        ]),
        &[
            "https://x.com/i/status/2106088717547077910",
            "https://x.com/i/status/2110286468623351937",
        ],
    );

    let (output, _) = run(&["rust", "--limit", "1"], vec![completed(&response)]);

    assert_eq!(
        (output.status.code(), refs(&payload(&output))),
        (Some(0), vec!["x:2106088717547077910".to_owned()])
    );
}

#[test]
fn a_list_whose_every_post_is_unverified_is_a_runtime_failure() {
    let response = with_posts(
        &fixture("search.json"),
        &json!([{"url": "https://x.com/someone/status/2110000000000000000", "author_handle": "someone", "text": "invented"}]),
        &["https://x.com/i/status/2106088717547077910"],
    );

    let (output, _) = run(&["rust"], vec![completed(&response)]);

    assert_eq!(
        (output.status.code(), &payload(&output)["error_kind"]),
        (Some(4), &json!("runtime")),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn an_empty_list_is_a_legitimate_empty_result() {
    let response = with_posts(&fixture("search.json"), &json!([]), &[]);

    let (output, _) = run(&["a topic nobody posts about"], vec![completed(&response)]);

    assert_eq!(
        (output.status.code(), &payload(&output)["items"]),
        (Some(0), &json!([]))
    );
}

#[test]
fn output_that_is_not_the_requested_json_is_a_runtime_failure() {
    let mut response = fixture("search.json");
    response["output"][2]["content"][0]["text"] = json!("Here are some posts: none.");

    let (output, _) = run(&["rust"], vec![completed(&response)]);

    assert_eq!(
        (output.status.code(), &payload(&output)["error_kind"]),
        (Some(4), &json!("runtime"))
    );
}

#[test]
fn invalid_filters_are_rejected_before_any_request() {
    for (arguments, message) in [
        (
            &["rust", "--handle", "a", "--exclude-handle", "b"][..],
            "--handle cannot be combined with --exclude-handle",
        ),
        (
            &["rust", "--handle", "space-x"],
            "invalid X handle `space-x`",
        ),
        (
            &["rust", "--from", "2026-10-02", "--to", "2026-10-01"],
            "--from must not be later than --to",
        ),
        (&["rust", "--from", "2026-13-01"], "not a calendar date"),
        (&["rust", "--limit", "21"], "21"),
        (&["  "], "x search needs a query"),
    ] {
        let fixture = Fixture::start_canary();
        let environment = RunEnvironment::new(&config(&fixture.url, &[KEY]));

        let output = search(&environment, arguments);

        assert_eq!(
            (
                output.status.code(),
                stderr(&output).contains(message),
                fixture.finish_all().len()
            ),
            (Some(2), true, 0),
            "arguments: {arguments:?}\nstderr: {}",
            stderr(&output)
        );
    }
}

#[test]
fn without_an_xai_key_the_search_fails_before_any_request_and_names_the_key() {
    let fixture = Fixture::start_canary();
    let environment = RunEnvironment::new(&config(&fixture.url, &[]));

    let output = search(&environment, &["rust"]);

    assert_eq!(
        (
            output.status.code(),
            stderr(&output).contains("providers.xai.keys"),
            fixture.finish_all().len()
        ),
        (Some(3), true, 0),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn an_empty_order_disables_the_platform_before_any_request() {
    let fixture = Fixture::start_canary();
    let environment = RunEnvironment::new(&format!(
        "{}[platforms.x]\norder = []\n",
        config(&fixture.url, &[KEY])
    ));

    let output = search(&environment, &["rust"]);

    assert_eq!(
        (
            output.status.code(),
            stderr(&output).contains("platforms.x.order"),
            fixture.finish_all().len()
        ),
        (Some(3), true, 0),
        "stderr: {}",
        stderr(&output)
    );
}

fn authorization(request: &str) -> Option<String> {
    request
        .lines()
        .find_map(|line| {
            line.strip_prefix("authorization: ")
                .or_else(|| line.strip_prefix("Authorization: "))
        })
        .map(str::to_owned)
}

#[test]
fn a_rate_limited_key_rotates_to_the_next_key() {
    let fixture = Fixture::start_sequence(vec![
        terminal(
            "response.failed",
            &json!({"response": {"error": {"code": "rate_limit_exceeded", "message": "slow down"}}}),
        ),
        completed(&fixture("search.json")),
    ]);
    let environment = RunEnvironment::new(&config(&fixture.url, &[KEY, "test-xai-key-b"]));

    let output = search(&environment, &["rust"]);
    let requests = fixture.finish_all();

    assert_eq!(
        (
            output.status.code(),
            requests
                .iter()
                .map(|request| authorization(request))
                .collect::<Vec<_>>()
        ),
        (
            Some(0),
            vec![
                Some(format!("Bearer {KEY}")),
                Some("Bearer test-xai-key-b".to_owned())
            ]
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_rejected_request_maps_to_the_attempt_parameter_error() {
    let (output, _) = run(
        &["rust"],
        vec![terminal(
            "response.failed",
            &json!({"response": {"error": {"code": "invalid_request_error", "message": "bad tool"}}}),
        )],
    );

    assert_eq!(
        (output.status.code(), &payload(&output)["error_kind"]),
        (Some(4), &json!("parameter")),
        "stderr: {}",
        stderr(&output)
    );
}
