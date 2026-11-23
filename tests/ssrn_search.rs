mod support;

use std::process::Output;

use serde_json::{Value, json};

use support::{Fixture, Response, RunEnvironment};

fn work(id: &str) -> Value {
    json!({
        "DOI": format!("10.2139/ssrn.{id}"),
        "title": [format!("Paper {id}")],
        "author": [{"given": "Gary", "family": "Antonacci", "sequence": "first"}],
        "abstract": format!("<jats:p>Abstract of {id}.</jats:p>"),
        "published": {"date-parts": [[2012]]},
        "type": "posted-content",
        "created": {"date-parts": [[2012, 4, 25]], "date-time": "2012-04-25T11:01:44Z"},
        "resource": {"primary": {"URL": format!("https://www.ssrn.com/abstract={id}")}}
    })
}

fn work_list(total: u64, items: &[Value]) -> Response {
    Response::json(
        200,
        &json!({
            "status": "ok",
            "message-type": "work-list",
            "message-version": "1.0.0",
            "message": {"total-results": total, "items": items, "items-per-page": items.len()}
        })
        .to_string(),
    )
}

fn config(url: &str) -> String {
    format!("[providers.ssrn_crossref]\nurl = \"{url}\"\n")
}

fn search(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "ssrn", "search"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

fn payload(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "parse JSON stdout: {error}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
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
    let mut without_abstract = work("2");
    without_abstract["abstract"] = Value::Null;
    without_abstract["type"] = json!("journal-article");
    let items = [
        work("1"),
        json!({"DOI": "10.1016/j.jfineco.2020.01.001", "title": ["Elsewhere"]}),
        without_abstract,
    ];
    let (output, _) = page(&["momentum"], work_list(3, &items));

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
    let mut without_abstract = work("2");
    without_abstract["abstract"] = Value::Null;
    let (output, _) = page(
        &["momentum", "--format", "markdown"],
        work_list(2, &[without_abstract, work("1")]),
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
