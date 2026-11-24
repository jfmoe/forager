use std::process::Output;

use serde_json::json;

use crate::support::ssrn_samples::{stored, work_with};
use crate::support::{Fixture, Response, RunEnvironment, payload};

fn config(url: &str) -> String {
    format!("[providers.ssrn_crossref]\nurl = \"{url}\"\n")
}

fn fetch(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "ssrn", "fetch"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

/// Runs one fetch against a fixture that answers with `responses`, and returns the output and
/// the received requests.
fn run(arguments: &[&str], responses: Vec<Response>) -> (Output, Vec<String>) {
    let fixture = Fixture::start_sequence(responses);
    let environment = RunEnvironment::new(&config(&fixture.url));
    let output = fetch(&environment, arguments);
    (output, fixture.finish_all())
}

#[test]
fn a_fetch_prints_the_paper_it_names() {
    let (output, _) = run(
        &["ssrn:2042750"],
        vec![Response::json(200, &stored("work.json"))],
    );
    let item = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            [&item["platform"], &item["provider"], &item["ref"]],
        ),
        (
            Some(0),
            [
                &json!("ssrn"),
                &json!("ssrn_crossref"),
                &json!("ssrn:2042750")
            ],
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn markdown_shows_the_abstract_section_only_when_there_is_an_abstract() {
    let without_abstract = work_with(&json!({"abstract": null})).to_string();
    let sections = [stored("work.json"), without_abstract].map(|body| {
        let (output, _) = run(
            &["ssrn:2042750", "--format", "markdown"],
            vec![Response::json(200, &body)],
        );
        String::from_utf8_lossy(&output.stdout).contains("## Abstract")
    });

    assert_eq!(sections, [true, false]);
}
