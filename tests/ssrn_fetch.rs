mod support;

use std::process::Output;

use serde_json::{Value, json};

use support::{Fixture, Response, RunEnvironment};

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

fn request_path(request: &str) -> String {
    request
        .split_whitespace()
        .nth(1)
        .expect("request target")
        .to_owned()
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
                .map(|request| request_path(request))
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
                "snippet": null,
                "doi": "10.2139/ssrn.2042750",
                "crossref_type": "posted-content",
                "crossref_created": "2012-04-25T11:01:44Z",
                "posted": null,
                "last_revised": null,
                "date_written": null
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
                request_path(&requests[0]),
                payload(&output)["ref"].clone()
            ),
            (Some(0), DOI_PATH.to_owned(), json!("ssrn:2042750")),
            "input: {input}"
        );
    }
}

#[test]
fn a_record_without_an_abstract_succeeds_at_metadata_depth() {
    let (output, _) = run(&["ssrn:2042750"], vec![paper(None)]);
    let payload = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &payload["depth"],
            &payload["abstract"]
        ),
        (Some(0), &json!("metadata"), &Value::Null)
    );
}

#[test]
fn an_abstract_request_without_an_abstract_is_a_quality_failure() {
    let (output, _) = run(&["ssrn:2042750", "--depth", "abstract"], vec![paper(None)]);
    let payload = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            &payload["message"]
        ),
        (
            Some(5),
            &json!("quality"),
            &json!("Crossref has no abstract for ssrn:2042750")
        )
    );
}

#[test]
fn an_abstract_request_with_an_abstract_succeeds() {
    let (output, _) = run(
        &["ssrn:2042750", "--depth", "abstract"],
        vec![paper(ABSTRACT)],
    );

    assert_eq!(
        (output.status.code(), payload(&output)["depth"].clone()),
        (Some(0), json!("abstract"))
    );
}

#[test]
fn a_missing_doi_names_crossref_as_the_source() {
    let (output, _) = run(
        &["ssrn:2042750"],
        vec![Response::new(404, "text/plain", "Resource not found.")],
    );
    let payload = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            &payload["message"]
        ),
        (
            Some(4),
            &json!("parameter"),
            &json!("SSRN paper not found in Crossref: ssrn:2042750")
        )
    );
}

#[test]
fn a_rate_limited_request_ends_the_route_without_retrying() {
    let (output, requests) = run(
        &["ssrn:2042750", "--verbose"],
        vec![Response::new(429, "text/plain", "Too many requests")],
    );
    let payload = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            requests.len(),
            payload["provider_attempts"].as_array().map(Vec::len),
        ),
        (Some(4), &json!("rate_limited"), 1, Some(1))
    );
}

#[test]
fn a_record_for_another_doi_is_a_runtime_failure() {
    let (output, _) = run(
        &["ssrn:2042750"],
        vec![work(&record("10.2139/ssrn.1", ABSTRACT))],
    );
    let payload = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            &payload["message"]
        ),
        (
            Some(4),
            &json!("runtime"),
            &json!("Crossref returned DOI 10.2139/ssrn.1 for ssrn:2042750")
        )
    );
}

#[test]
fn a_response_of_another_message_type_is_a_runtime_failure() {
    let body = json!({"status": "ok", "message-type": "work-list", "message": {"total-results": 0, "items": []}});
    let (output, _) = run(
        &["ssrn:2042750"],
        vec![Response::json(200, &body.to_string())],
    );

    assert_eq!(
        (output.status.code(), payload(&output)["error_kind"].clone()),
        (Some(4), json!("runtime"))
    );
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

fn assert_preflight_exit(config: &str, arguments: &[&str], exit_code: i32, message: &str) {
    let fixture = Fixture::start_canary();
    let environment = RunEnvironment::new(&config.replace("{url}", &fixture.url));

    let output = fetch(&environment, arguments);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        (
            output.status.code(),
            stderr.contains(message),
            fixture.finish_all().len()
        ),
        (Some(exit_code), true, 0),
        "arguments: {arguments:?}\nstderr: {stderr}"
    );
}

const ROUTE_CONFIG: &str = "[providers.ssrn_crossref]\nurl = \"{url}\"\n";

#[test]
fn download_links_and_short_links_fail_before_any_request() {
    for input in [
        "https://papers.ssrn.com/sol3/Delivery.cfm/SSRN_ID2881657_code1556771.pdf?abstractid=2042750&mirid=1",
        "https://papers.ssrn.com.evil.example/sol3/papers.cfm?abstract_id=2042750",
        "https://bit.ly/abc",
    ] {
        assert_preflight_exit(ROUTE_CONFIG, &[input], 2, "unrecognized ssrn reference");
    }
}

#[test]
fn a_full_text_request_fails_before_any_request() {
    assert_preflight_exit(
        ROUTE_CONFIG,
        &["ssrn:2042750", "--depth", "full_text"],
        2,
        "ssrn_crossref cannot fetch at depth `full_text`",
    );
}
