use std::collections::BTreeMap;
use std::process::Output;

use serde_json::{Value, json};

use crate::support::samples::sample;
use crate::support::{Fixture, Response, RunEnvironment, arxiv_feed, payload, query};

/// A page of `total` results holding the stored entry once for each of `ids`.
fn feed(total: u64, ids: &[&str]) -> String {
    arxiv_feed::page(&sample("arxiv_api/feed.xml"), total, ids)
}

fn atom(status: u16, body: &str) -> Response {
    Response::new(status, "application/atom+xml", body)
}

fn config(url: &str) -> String {
    format!("[providers.arxiv_api]\nurl = \"{url}/api/query\"\n")
}

fn search(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "arxiv", "search"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

#[test]
fn search_returns_abstract_depth_items_with_refs_and_metadata() {
    let fixture = Fixture::start_sequence(vec![atom(200, &feed(1, &["2401.01234v2"]))]);
    let environment = RunEnvironment::new(&config(&fixture.url));

    let output = search(&environment, &["electron"]);
    fixture.finish();

    assert_eq!(
        (output.status.code(), payload(&output)),
        (
            Some(0),
            json!({
                "platform": "arxiv",
                "provider": "arxiv_api",
                "items": [{
                    "ref": "arxiv:2401.01234v2",
                    "url": "https://arxiv.org/abs/2401.01234v2",
                    "depth": "abstract",
                    "title": "Dark Matter Halos & Galaxies",
                    "authors": ["Ada Lovelace", "Alan Turing"],
                    "published": "2024-01-02T18:59:59Z",
                    "abstract": "First line\n  second line.",
                    "updated": "2024-02-03T04:05:06Z",
                    "primary_category": "astro-ph.CO",
                    "categories": ["astro-ph.CO", "hep-ph"],
                    "doi": "10.1000/example",
                    "journal_ref": "Phys. Rev. D 1 (2024)",
                    "comment": "12 pages, 3 figures",
                    "pdf_url": "https://arxiv.org/pdf/2401.01234v2"
                }],
                "next_cursor": null
            })
        )
    );
}

/// Runs one page in its own environment and returns the page and the received query.
fn page(arguments: &[&str], response: &str) -> (Value, BTreeMap<String, String>) {
    let fixture = Fixture::start_sequence(vec![atom(200, response)]);
    let environment = RunEnvironment::new(&config(&fixture.url));
    let output = search(&environment, arguments);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    (payload(&output), query(&fixture.finish()))
}

#[test]
fn cursors_page_through_results_and_restore_the_original_request() {
    let arguments = [
        r#""neutrino mass" hierarchy"#,
        "--category",
        "hep-ph",
        "--sort",
        "updated",
        "--limit",
        "2",
    ];
    let (first, first_query) = page(&arguments, &feed(5, &["2401.00001v1", "2401.00002v1"]));
    let first_cursor = first["next_cursor"].as_str().expect("first page cursor");
    let (middle, middle_query) = page(
        &["--cursor", first_cursor],
        &feed(5, &["2401.00003v1", "2401.00004v1"]),
    );
    let middle_cursor = middle["next_cursor"].as_str().expect("middle page cursor");
    let (last, last_query) = page(&["--cursor", middle_cursor], &feed(5, &["2401.00005v1"]));
    let restored = |query: &BTreeMap<String, String>| {
        let mut query = query.clone();
        query.remove("start");
        query
    };

    assert_eq!(
        (
            first_cursor.starts_with("v1.arxiv_api."),
            [&first_query, &middle_query, &last_query].map(|query| query["start"].clone()),
            restored(&middle_query) == restored(&first_query),
            restored(&last_query) == restored(&first_query),
            &last["next_cursor"],
            last["items"].as_array().map(Vec::len),
        ),
        (
            true,
            ["0".to_owned(), "2".to_owned(), "4".to_owned()],
            true,
            true,
            &Value::Null,
            Some(1),
        )
    );
}
