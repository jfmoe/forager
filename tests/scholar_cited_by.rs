mod support;

use std::process::Output;

use serde_json::json;

use support::scholar::{config, fixture, ok, payload, refs, stderr};
use support::{Fixture, Response, RunEnvironment};

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
fn cited_by_decodes_citing_works_like_search_results() {
    let source = fixture("cited_by.json");
    let (output, _) = run(&[CITED, "--query", "crash"], vec![ok(&source)]);
    let mut page = payload(&output);
    let first = page["items"][0].clone();
    page.as_object_mut().expect("page").remove("items");

    assert_eq!(
        (
            output.status.code(),
            refs(&payload(&output)),
            first,
            page["platform"].clone(),
            page["provider"].clone(),
            page["next_cursor"].is_string(),
        ),
        (
            Some(0),
            vec![
                "scholar:2061579707271568590".to_owned(),
                "scholar:13440399377941409520".to_owned(),
                "scholar:239332318604014067".to_owned(),
            ],
            json!({
                "ref": "scholar:2061579707271568590",
                "url": "https://scholar.google.com/scholar?cluster=2061579707271568590",
                "depth": "snippet",
                "title": "Momentum crashes",
                "authors": ["K Daniel", "TJ Moskowitz"],
                "published": "2016",
                "snippet": source["organic_results"][0]["snippet"],
                "link": "https://www.sciencedirect.com/science/article/pii/S0304405X16301490",
                "source": "K Daniel, TJ Moskowitz - Journal of Financial economics, 2016 - Elsevier",
                "cited_by": 1851,
                "version_count": 28,
                "resources": [{
                    "title": "sciencedirect.com",
                    "file_format": "HTML",
                    "url": "https://www.sciencedirect.com/science/article/pii/S0304405X16301490"
                }],
                "result_type": "Html"
            }),
            json!("scholar"),
            json!("serpapi"),
            true,
        ),
        "stderr: {}",
        stderr(&output)
    );
}
