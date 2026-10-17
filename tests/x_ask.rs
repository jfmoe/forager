//! `forager platform x ask` against a local xAI Responses fixture built from the live answer of
//! 2026-10-17.

mod support;

use std::process::Output;

use serde_json::json;

use support::x::{completed, config, fixture, payload, request, stderr};
use support::{Fixture, Response, RunEnvironment};

const KEY: &str = "test-xai-key-a";

fn run(arguments: &[&str], responses: Vec<Response>) -> (Output, Vec<String>) {
    let fixture = Fixture::start_sequence(responses);
    let environment = RunEnvironment::new(&config(&fixture.url, &[KEY]));
    let mut command = vec!["platform", "x", "ask"];
    command.extend_from_slice(arguments);
    let output = environment.run(&command);
    (output, fixture.finish_all())
}

#[test]
fn ask_sends_the_filtered_x_search_tool_without_structured_output() {
    let (output, requests) = run(
        &[
            "What did SpaceX say about Starship in early October?",
            "--handle",
            "SpaceX",
            "--from",
            "2026-10-01",
            "--to",
            "2026-10-14",
        ],
        vec![completed(&fixture("ask.json"))],
    );
    let (target, body) = request(&requests[0]);

    assert_eq!(
        (
            output.status.code(),
            target.as_str(),
            &body["tools"],
            body.get("text"),
            body["input"][0]["content"].as_str().is_some_and(
                |input| input.ends_with("What did SpaceX say about Starship in early October?")
            ),
        ),
        (
            Some(0),
            "/responses",
            &json!([{
                "type": "x_search",
                "allowed_x_handles": ["SpaceX"],
                "from_date": "2026-10-01",
                "to_date": "2026-10-15"
            }]),
            None,
            true,
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn the_answer_keeps_its_citations_and_cited_posts_become_refs() {
    let (output, _) = run(&["Starship news"], vec![completed(&fixture("ask.json"))]);
    let answer = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &answer["platform"],
            &answer["provider"],
            &answer["query"],
            answer["answer"].as_str().is_some_and(
                |text| text.contains("[[1]](https://x.com/SpaceX/status/2106089751837278533)")
            ),
        ),
        (
            Some(0),
            &json!("x"),
            &json!("xai"),
            &json!("Starship news"),
            true
        ),
        "stderr: {}",
        stderr(&output)
    );
    assert_eq!(
        answer["sources"],
        json!([
            {"ref": "x:2106089751837278533", "url": "https://x.com/i/status/2106089751837278533", "title": ""},
            {"ref": "x:2106090316747207047", "url": "https://x.com/i/status/2106090316747207047", "title": ""},
            {"ref": "x:2110126570233094527", "url": "https://x.com/i/status/2110126570233094527", "title": ""}
        ])
    );
}

#[test]
fn a_cited_profile_keeps_its_url_without_a_ref() {
    let mut response = fixture("ask.json");
    response["output"][2]["content"][0]["annotations"] = json!([
        {"type": "url_citation", "url": "https://x.com/i/user/34743251", "start_index": 0, "end_index": 10, "title": "SpaceX"}
    ]);

    let (output, _) = run(&["Who runs Starship?"], vec![completed(&response)]);

    assert_eq!(
        (output.status.code(), &payload(&output)["sources"][0]),
        (
            Some(0),
            &json!({"ref": null, "url": "https://x.com/i/user/34743251", "title": "SpaceX"})
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn verbose_output_lists_the_platform_attempt() {
    let (output, _) = run(
        &["Starship news", "--verbose"],
        vec![completed(&fixture("ask.json"))],
    );
    let answer = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &answer["provider_attempts"][0]["provider"],
            &answer["provider_attempts"][0]["operation"],
        ),
        (Some(0), &json!("xai"), &json!("ask")),
        "{answer}"
    );
}

#[test]
fn invalid_filters_or_a_blank_question_are_rejected_before_any_request() {
    for arguments in [
        &["q", "--handle", "a", "--exclude-handle", "b"][..],
        &["q", "--to", "2026-02-30"],
        &[" "],
    ] {
        let fixture = Fixture::start_canary();
        let environment = RunEnvironment::new(&config(&fixture.url, &[KEY]));
        let mut command = vec!["platform", "x", "ask"];
        command.extend_from_slice(arguments);

        let output = environment.run(&command);

        assert_eq!(
            (output.status.code(), fixture.finish_all().len()),
            (Some(2), 0),
            "arguments: {arguments:?}\nstderr: {}",
            stderr(&output)
        );
    }
}

#[test]
fn without_an_xai_key_ask_fails_before_any_request() {
    let fixture = Fixture::start_canary();
    let environment = RunEnvironment::new(&config(&fixture.url, &[]));

    let output = environment.run(&["platform", "x", "ask", "Starship news"]);

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
