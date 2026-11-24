use std::fs;
use std::process::Output;

use serde_json::json;

use crate::support::hn::config;
use crate::support::samples::sample;
use crate::support::{Fixture, Response, RunEnvironment, payload, stderr};

fn fetch(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "hn", "fetch"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

#[test]
fn a_thread_is_written_to_the_content_directory_with_a_receipt_on_stdout() {
    let algolia =
        Fixture::start_sequence(vec![Response::json(200, &sample("hn_api/item-tree.json"))]);
    let firebase = Fixture::start_sequence(vec![Response::json(
        200,
        &sample("hn_api/firebase-item.json"),
    )]);
    let environment = RunEnvironment::new(&config(&algolia.url, &firebase.url));
    let content_dir = tempfile::tempdir().expect("content directory");

    let output = fetch(
        &environment,
        &[
            "https://news.ycombinator.com/item?id=8863&p=2",
            "--content-dir",
            content_dir.path().to_str().expect("UTF-8 path"),
        ],
    );
    algolia.finish_all();
    firebase.finish_all();
    let payload = payload(&output);
    let content_path = payload["content_path"].as_str().expect("content path");
    let written = fs::read_to_string(content_path).expect("read discussion file");

    assert_eq!(
        (
            output.status.code(),
            [&payload["platform"], &payload["provider"], &payload["ref"]],
            [&payload["content_url"], &payload["content_provider"]],
            content_path,
            &payload["content_len"],
            payload.get("content").is_none(),
        ),
        (
            Some(0),
            [&json!("hn"), &json!("hn_api"), &json!("hn:8863")],
            [
                &json!("https://news.ycombinator.com/item?id=8863"),
                &json!("hn_api")
            ],
            content_dir
                .path()
                .join("hn-8863.md")
                .display()
                .to_string()
                .as_str(),
            &json!(written.chars().count()),
            true,
        ),
        "stderr: {}",
        stderr(&output)
    );
}
