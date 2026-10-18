//! `forager platform x fetch` against local xAI Responses fixtures built from the live thread
//! reads of 2026-10-17 and 2026-10-18.

mod support;

use std::process::Output;

use serde_json::{Value, json};

use support::x::{completed, config, fixture, payload, request, stderr};
use support::{Fixture, Response, RunEnvironment};

const KEY: &str = "test-xai-key-a";
const ROOT: &str = "x:2110126570233094527";

fn run(arguments: &[&str], responses: Vec<Response>) -> (Output, Vec<String>) {
    let fixture = Fixture::start_sequence(responses);
    let environment = RunEnvironment::new(&config(&fixture.url, &[KEY]));
    let mut command = vec!["platform", "x", "fetch"];
    command.extend_from_slice(arguments);
    let output = environment.run(&command);
    (output, fixture.finish_all())
}

/// The thread fixture with its structured output replaced by `posts`.
fn thread_with(posts: &Value) -> Value {
    let mut response = fixture("thread.json");
    response["output"][1]["content"][0]["text"] = json!(json!({ "posts": posts }).to_string());
    response
}

#[test]
fn a_thread_fetch_returns_the_root_with_every_post_in_order() {
    let (output, requests) = run(
        &["https://twitter.com/SpaceX/status/2110126570233094527/photo/1?s=20"],
        vec![completed(&fixture("thread.json"))],
    );
    let fetched = payload(&output);
    let (_, body) = request(&requests[0]);

    assert_eq!(
        (
            output.status.code(),
            &body["tools"],
            &body["text"]["format"]["schema"]["properties"]["posts"]["items"]["properties"]["role"]
                ["enum"],
        ),
        (
            Some(0),
            &json!([{"type": "x_search"}]),
            &json!(["parent", "root", "reply", "quoted"]),
        ),
        "stderr: {}",
        stderr(&output)
    );
    assert_eq!(
        (
            &fetched["ref"],
            &fetched["url"],
            &fetched["depth"],
            &fetched["authors"],
            &fetched["published"],
            &fetched["provider"],
        ),
        (
            &json!(ROOT),
            &json!("https://x.com/i/status/2110126570233094527"),
            &json!("thread"),
            &json!(["SpaceX"]),
            &json!("2026-10-13T21:52:33Z"),
            &json!("xai"),
        )
    );
    assert_eq!(
        fetched["thread"][1],
        json!({
            "ref": "x:2110127170869665960",
            "url": "https://x.com/i/status/2110127170869665960",
            "author_handle": "olivia_p_walker",
            "published": "2026-10-13T21:54:56Z",
            "text": "Exquisite photography.",
            "role": "reply"
        })
    );
    assert_eq!(
        fetched["thread"]
            .as_array()
            .expect("thread")
            .iter()
            .map(|post| post["role"].as_str().expect("role"))
            .collect::<Vec<_>>(),
        ["root", "reply", "reply", "reply"]
    );
}

#[test]
fn a_fetch_requires_the_model_to_call_its_tool() {
    let (output, requests) = run(&[ROOT], vec![completed(&fixture("thread.json"))]);
    let (_, body) = request(&requests[0]);

    assert_eq!(
        (output.status.code(), &body["tool_choice"]),
        (Some(0), &json!("required")),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn metadata_depth_returns_the_post_without_its_thread() {
    let (output, _) = run(
        &[ROOT, "--depth", "metadata"],
        vec![completed(&fixture("thread.json"))],
    );
    let fetched = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &fetched["depth"],
            fetched.get("thread"),
            fetched["text"]
                .as_str()
                .map(|text| text.starts_with("The first Starship"))
        ),
        (Some(0), &json!("metadata"), None, Some(true)),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn content_format_prints_the_thread_as_markdown() {
    let (output, _) = run(
        &[ROOT, "--format", "content"],
        vec![completed(&fixture("thread.json"))],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(
        (
            output.status.code(),
            stdout
                .starts_with("- **@SpaceX** (root, 2026-10-13T21:52:33Z) `x:2110126570233094527`"),
            stdout.contains(
                "- **@BassonBrain** (reply, 2026-10-13T22:41:44Z) `x:2110138949234487662`\n\n  Wow!"
            ),
        ),
        (Some(0), true, true),
        "stdout: {stdout}"
    );
}

#[test]
fn a_root_other_than_the_requested_post_is_a_runtime_failure() {
    let response = thread_with(&json!([
        {"url": "https://x.com/SpaceX/status/2110126570233094000", "author_handle": "SpaceX", "text": "other", "role": "root"}
    ]));

    let (output, _) = run(&[ROOT], vec![completed(&response)]);

    assert_eq!(
        (output.status.code(), &payload(&output)["error_kind"]),
        (Some(4), &json!("runtime")),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_reply_is_fetched_although_its_thread_fetch_cites_only_the_parent() {
    let (output, _) = run(
        &["x:2098634733248720992"],
        vec![completed(&fixture("reply_thread.json"))],
    );
    let item = payload(&output);
    let roles = item["thread"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|post| post["role"].clone())
        .collect::<Vec<_>>();

    assert_eq!(
        (output.status.code(), &item["ref"], roles),
        (
            Some(0),
            &json!("x:2098634733248720992"),
            vec![json!("root"), json!("parent"), json!("reply")]
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_root_cited_without_a_thread_fetch_is_fetched() {
    let mut response = fixture("thread.json");
    response["output"].as_array_mut().expect("output").remove(0);

    let (output, _) = run(&[ROOT], vec![completed(&response)]);

    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
}

#[test]
fn a_root_neither_thread_fetched_nor_cited_is_a_runtime_failure() {
    let mut response = fixture("thread.json");
    response["output"].as_array_mut().expect("output").remove(0);
    response["output"][0]["content"][0]["annotations"] = json!([]);

    let (output, _) = run(&[ROOT], vec![completed(&response)]);

    assert_eq!(
        (output.status.code(), &payload(&output)["error_kind"]),
        (Some(4), &json!("runtime"))
    );
}

#[test]
fn a_thread_fetch_of_another_post_does_not_verify_the_root() {
    let mut response = fixture("thread.json");
    response["output"][0]["input"] = json!(r#"{"post_id":"2110126570233094000"}"#);
    response["output"][1]["content"][0]["annotations"] = json!([]);

    let (output, _) = run(&[ROOT], vec![completed(&response)]);

    assert_eq!(
        (output.status.code(), &payload(&output)["error_kind"]),
        (Some(4), &json!("runtime"))
    );
}

#[test]
fn an_unfinished_thread_fetch_does_not_verify_the_root() {
    let mut response = fixture("thread.json");
    response["output"][0]["status"] = json!("failed");
    response["output"][1]["content"][0]["annotations"] = json!([]);

    let (output, _) = run(&[ROOT], vec![completed(&response)]);

    assert_eq!(
        (output.status.code(), &payload(&output)["error_kind"]),
        (Some(4), &json!("runtime"))
    );
}

#[test]
fn a_thread_without_exactly_one_root_is_a_runtime_failure() {
    let reply = json!({"url": "https://x.com/a/status/2110127170869665960", "author_handle": "a", "text": "hi", "role": "reply"});

    let (output, _) = run(&[ROOT], vec![completed(&thread_with(&json!([reply])))]);

    assert_eq!(
        (output.status.code(), &payload(&output)["error_kind"]),
        (Some(4), &json!("runtime"))
    );
}

#[test]
fn a_post_the_model_cannot_see_is_an_attempt_parameter_error() {
    let (output, _) = run(&[ROOT], vec![completed(&thread_with(&json!([])))]);
    let failure = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &failure["error_kind"],
            failure["message"]
                .as_str()
                .is_some_and(|message| message
                    .contains("x:2110126570233094527 was not found or is not visible"))
        ),
        (Some(4), &json!("parameter"), true),
        "{failure}"
    );
}

#[test]
fn unsupported_depths_short_links_and_other_urls_are_rejected_before_any_request() {
    for (arguments, message) in [
        (
            &[ROOT, "--depth", "full_text"][..],
            "cannot fetch at depth `full_text`",
        ),
        (
            &[ROOT, "--depth", "snippet"],
            "cannot fetch at depth `snippet`",
        ),
        (&["https://t.co/AbCdEf"], "t.co short links"),
        (&["https://x.com/SpaceX"], "unrecognized x reference"),
        (&["2110126570233094527"], "unrecognized x reference"),
    ] {
        let fixture = Fixture::start_canary();
        let environment = RunEnvironment::new(&config(&fixture.url, &[KEY]));
        let mut command = vec!["platform", "x", "fetch"];
        command.extend_from_slice(arguments);

        let output = environment.run(&command);

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
