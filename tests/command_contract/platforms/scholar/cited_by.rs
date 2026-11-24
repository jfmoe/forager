use std::process::Output;

use serde_json::json;

use crate::support::scholar::{config, fixture};
use crate::support::{Fixture, Response, RunEnvironment, ok_json, payload, refs, stderr};

const KEY: &str = "test-serpapi-key-a";
const CITED: &str = "scholar:18208131694456651388";

fn cited_by(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "scholar", "cited-by"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

/// Runs one cited-by command against a fixture that answers in order and returns every request.
fn run(arguments: &[&str], responses: Vec<Response>) -> (Output, Vec<String>) {
    let fixture = Fixture::start_sequence(responses);
    let environment = RunEnvironment::new(&config(&fixture.url, &[KEY]));
    let output = cited_by(&environment, arguments);
    (output, fixture.finish_all())
}

#[test]
fn a_cited_by_page_prints_the_citing_papers_by_their_cluster_refs() {
    let (output, _) = run(
        &[CITED, "--query", "crash"],
        vec![ok_json(&fixture("cited_by.json"))],
    );
    let page = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &page["platform"],
            &page["provider"],
            refs(&page),
        ),
        (
            Some(0),
            &json!("scholar"),
            &json!("serpapi"),
            vec![
                "scholar:2061579707271568590".to_owned(),
                "scholar:13440399377941409520".to_owned(),
                "scholar:239332318604014067".to_owned(),
            ],
        ),
        "stderr: {}",
        stderr(&output)
    );
}
