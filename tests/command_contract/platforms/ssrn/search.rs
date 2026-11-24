use std::process::Output;

use serde_json::json;

use crate::support::ssrn_samples::{stored, stored_json, work_list};
use crate::support::{Fixture, Response, RunEnvironment, payload, refs, stderr};

fn config(url: &str) -> String {
    format!("[providers.ssrn_crossref]\nurl = \"{url}\"\n")
}

fn search(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "ssrn", "search"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

/// Runs one page in its own environment and returns the output and the received request.
fn page(arguments: &[&str], response: Response) -> (Output, String) {
    let fixture = Fixture::start_sequence(vec![response]);
    let environment = RunEnvironment::new(&config(&fixture.url));
    let output = search(&environment, arguments);
    (output, fixture.finish())
}

#[test]
fn a_search_prints_the_ssrn_papers_and_names_the_skipped_dois_on_stderr() {
    let (output, _) = page(
        &["momentum"],
        Response::json(200, &stored("work-list.json")),
    );

    let page = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &page["platform"],
            &page["provider"],
            refs(&page),
            stderr(&output).contains("10.1016/j.jfineco.2020.01.001"),
        ),
        (
            Some(0),
            &json!("ssrn"),
            &json!("ssrn_crossref"),
            vec!["ssrn:1".to_owned(), "ssrn:2".to_owned()],
            true,
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn markdown_shows_an_abstract_only_for_items_that_have_one() {
    let records = &stored_json("work-list.json")["message"]["items"];
    let without_abstract = records[2].clone();
    let (output, _) = page(
        &["momentum", "--format", "markdown"],
        Response::json(
            200,
            &work_list(2, &[without_abstract, records[0].clone()]).to_string(),
        ),
    );
    let markdown = String::from_utf8_lossy(&output.stdout);

    assert_eq!(
        (
            markdown.contains("  Abstract of 1."),
            markdown.contains("`ssrn:2` — Gary Antonacci — 2012\n- [Paper 1]"),
        ),
        (true, true),
        "{markdown}"
    );
}
