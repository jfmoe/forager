mod support;

use std::process::Output;

use serde_json::{Value, json};

use support::{Fixture, Response, RunEnvironment, path};

const DOI_PATH: &str = "/works/10.2139/ssrn.2042750";

fn record(doi: &str, abstract_markup: Option<&str>) -> Value {
    json!({
        "DOI": doi,
        "title": ["Risk Premia Harvesting Through Dual Momentum"],
        "author": [{"given": "Gary", "family": "Antonacci", "sequence": "first"}],
        "abstract": abstract_markup,
        "published": {"date-parts": [[2012, 4]]},
        "type": "posted-content",
        "created": {"date-parts": [[2012, 4, 25]], "date-time": "2012-04-25T11:01:44Z"},
        "resource": {"primary": {"URL": "https://www.ssrn.com/abstract=2042750"}}
    })
}

fn work(message: &Value) -> Response {
    Response::json(
        200,
        &json!({"status": "ok", "message-type": "work", "message-version": "1.0.0", "message": message})
            .to_string(),
    )
}

fn paper(abstract_markup: Option<&str>) -> Response {
    work(&record("10.2139/ssrn.2042750", abstract_markup))
}

const ABSTRACT: Option<&str> = Some("<jats:p>Momentum is the premier market anomaly.</jats:p>");

fn config(url: &str) -> String {
    format!("[providers.ssrn_crossref]\nurl = \"{url}\"\n")
}

fn fetch(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "ssrn", "fetch"];
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

/// Runs one fetch against a fixture that answers with `responses`, and returns the output and
/// the received requests.
fn run(arguments: &[&str], responses: Vec<Response>) -> (Output, Vec<String>) {
    let fixture = Fixture::start_sequence(responses);
    let environment = RunEnvironment::new(&config(&fixture.url));
    let output = fetch(&environment, arguments);
    (output, fixture.finish_all())
}

#[test]
fn fetch_returns_metadata_and_the_abstract_at_the_default_depth() {
    let (output, requests) = run(&["ssrn:2042750"], vec![paper(ABSTRACT)]);

    assert_eq!(
        (
            output.status.code(),
            requests
                .iter()
                .map(|request| path(request))
                .collect::<Vec<_>>(),
            payload(&output)
        ),
        (
            Some(0),
            vec![DOI_PATH.to_owned()],
            json!({
                "platform": "ssrn",
                "provider": "ssrn_crossref",
                "ref": "ssrn:2042750",
                "url": "https://papers.ssrn.com/sol3/papers.cfm?abstract_id=2042750",
                "depth": "abstract",
                "title": "Risk Premia Harvesting Through Dual Momentum",
                "authors": ["Gary Antonacci"],
                "published": "2012-04",
                "abstract": "Momentum is the premier market anomaly.",
                "doi": "10.2139/ssrn.2042750",
                "crossref_type": "posted-content",
                "crossref_created": "2012-04-25T11:01:44Z"
            })
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn every_accepted_input_form_reads_the_same_doi() {
    for input in [
        "https://papers.ssrn.com/sol3/papers.cfm?abstract_id=2042750",
        "https://www.ssrn.com/abstract=2042750",
        "10.2139/ssrn.2042750",
        "https://doi.org/10.2139/ssrn.2042750",
    ] {
        let (output, requests) = run(&[input], vec![paper(ABSTRACT)]);

        assert_eq!(
            (
                output.status.code(),
                path(&requests[0]),
                payload(&output)["ref"].clone()
            ),
            (Some(0), DOI_PATH.to_owned(), json!("ssrn:2042750")),
            "input: {input}"
        );
    }
}

#[test]
fn markdown_shows_the_abstract_section_only_when_there_is_an_abstract() {
    let sections = [ABSTRACT, None].map(|abstract_markup| {
        let (output, _) = run(
            &["ssrn:2042750", "--format", "markdown"],
            vec![paper(abstract_markup)],
        );
        String::from_utf8_lossy(&output.stdout).contains("## Abstract")
    });

    assert_eq!(sections, [true, false]);
}
