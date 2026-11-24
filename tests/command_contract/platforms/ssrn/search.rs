use std::process::Output;

use serde_json::json;

use crate::support::ssrn_samples::{stored, stored_json, work_list};
use crate::support::{Fixture, Response, RunEnvironment, payload};

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
fn search_returns_abstract_and_metadata_depth_items_and_skips_non_ssrn_dois() {
    let (output, _) = page(
        &["momentum"],
        Response::json(200, &stored("work-list.json")),
    );

    assert_eq!(
        (
            output.status.code(),
            payload(&output),
            String::from_utf8_lossy(&output.stderr).contains(
                "ssrn_crossref skipped 1 Crossref records without an SSRN DOI: 10.1016/j.jfineco.2020.01.001"
            ),
        ),
        (
            Some(0),
            json!({
                "platform": "ssrn",
                "provider": "ssrn_crossref",
                "items": [
                    {
                        "ref": "ssrn:1",
                        "url": "https://papers.ssrn.com/sol3/papers.cfm?abstract_id=1",
                        "depth": "abstract",
                        "title": "Paper 1",
                        "authors": ["Gary Antonacci"],
                        "published": "2012",
                        "abstract": "Abstract of 1.",
                        "doi": "10.2139/ssrn.1",
                        "crossref_type": "posted-content",
                        "crossref_created": "2012-04-25T11:01:44Z"
                    },
                    {
                        "ref": "ssrn:2",
                        "url": "https://papers.ssrn.com/sol3/papers.cfm?abstract_id=2",
                        "depth": "metadata",
                        "title": "Paper 2",
                        "authors": ["Gary Antonacci"],
                        "published": "2012",
                        "abstract": null,
                        "doi": "10.2139/ssrn.2",
                        "crossref_type": "journal-article",
                        "crossref_created": "2012-04-25T11:01:44Z"
                    }
                ],
                "next_cursor": null
            }),
            true,
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
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
