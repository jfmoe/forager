//! SSRN full-text fetch: a fake `opencli` downloads a PDF, a fixture server converts it
//! through `/v2/parse`, and the CLI delivers the Markdown.

mod support;

use std::fs;
use std::path::Path;
use std::process::Output;

use serde_json::{Value, json};

use support::opencli::FakeOpenCli;
use support::{Fixture, Response, RunEnvironment};

const PDF: &[u8] = b"%PDF-1.5\nfake pdf body\n";

fn paper_page(id: &str) -> Value {
    json!({
        "url": format!("https://papers.ssrn.com/sol3/papers.cfm?abstract_id={id}"),
        "canonical_url": format!("https://papers.ssrn.com/sol3/papers.cfm?abstract_id={id}"),
        "doi": format!("10.2139/ssrn.{id}"),
        "title": "Risk Premia Harvesting Through Dual Momentum",
        "authors": ["Gary Antonacci"],
        "abstract_paragraphs": ["Momentum is the premier market anomaly."],
        "notes": ["37 Pages", "Posted: 19 Apr 2012"],
    })
}

fn markdown() -> String {
    format!(
        "# Risk Premia Harvesting Through Dual Momentum\n\n## Introduction\n\n{}",
        "Converted body text. ".repeat(20)
    )
}

fn parsed(content: &str) -> Response {
    Response::json(
        200,
        &json!({"success": true, "data": {"markdown": content}}).to_string(),
    )
}

fn environment(fake: &FakeOpenCli, provider: &Fixture) -> RunEnvironment {
    RunEnvironment::new(&format!(
        "[providers.firecrawl]\nurl = {:?}\nkeys = [\"firecrawl-key\"]\n\n{}",
        provider.url,
        fake.config("[\"ssrn_browser\"]")
    ))
}

fn fetch(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec![
        "platform",
        "ssrn",
        "fetch",
        "ssrn:2042750",
        "--depth",
        "full_text",
    ];
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

fn request_line(request: &str) -> &str {
    request.lines().next().expect("request line")
}

fn request_body(request: &str) -> &str {
    request.split_once("\r\n\r\n").expect("request body").1
}

fn files_under(directory: &Path) -> Vec<String> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory).expect("read directory") {
        let path = entry.expect("directory entry").path();
        if path.is_file() {
            files.push(
                path.file_name()
                    .expect("file name")
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    files
}

#[test]
fn full_text_delivers_markdown_and_removes_the_pdf_by_default() {
    let fake = FakeOpenCli::downloading(&paper_page("2042750"), "paper.pdf", PDF);
    let provider = Fixture::start_sequence(vec![parsed(&markdown())]);
    let environment = environment(&fake, &provider);
    let content_dir = tempfile::tempdir().expect("content directory");

    let output = fetch(
        &environment,
        &[
            "--content-dir",
            content_dir.path().to_str().expect("UTF-8 path"),
        ],
    );
    let requests = provider.finish_all();
    let payload = payload(&output);
    let markdown_path = content_dir.path().join("ssrn-2042750.md");

    assert_eq!(
        (
            output.status.code(),
            request_line(&requests[0]),
            payload["provider"].clone(),
            payload["depth"].clone(),
            payload["content_url"].clone(),
            payload["content_provider"].clone(),
            payload["content_path"].clone(),
            payload["content_len"].clone(),
            payload.get("pdf_path").is_none(),
            fs::read_to_string(&markdown_path).expect("read Markdown"),
            fake.path("paper.pdf").exists(),
        ),
        (
            Some(0),
            "POST /parse HTTP/1.1",
            json!("ssrn_browser"),
            json!("full_text"),
            json!("https://papers.ssrn.com/sol3/papers.cfm?abstract_id=2042750"),
            json!("firecrawl"),
            json!(markdown_path.display().to_string()),
            json!(markdown().chars().count()),
            true,
            markdown(),
            false,
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // The multipart parse request carries the local file and the PDF parser options.
    let body = request_body(&requests[0]);
    for part in [
        "name=\"file\"",
        "filename=\"paper.pdf\"",
        "%PDF-1.5",
        "name=\"options\"",
        "\"formats\":[\"markdown\"]",
        "\"pageMarkers\":true",
    ] {
        assert!(body.contains(part), "parse request lacks `{part}`:\n{body}");
    }
    // The signed download address never reaches the output.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("Delivery.cfm") && !stdout.contains("download.ssrn.com"));
}

#[test]
fn keep_pdf_moves_the_pdf_next_to_the_markdown() {
    let fake = FakeOpenCli::downloading(&paper_page("2042750"), "paper.pdf", PDF);
    let provider = Fixture::start_sequence(vec![parsed(&markdown())]);
    let environment = environment(&fake, &provider);
    let content_dir = tempfile::tempdir().expect("content directory");

    let output = fetch(
        &environment,
        &[
            "--keep-pdf",
            "--content-dir",
            content_dir.path().to_str().expect("UTF-8 path"),
        ],
    );
    provider.finish_all();
    let payload = payload(&output);
    let pdf_path = content_dir.path().join("ssrn-2042750.pdf");

    assert_eq!(
        (
            output.status.code(),
            payload["pdf_path"].clone(),
            payload["pdf_bytes"].clone(),
            fs::read(&pdf_path).expect("read kept PDF"),
            fake.path("paper.pdf").exists(),
        ),
        (
            Some(0),
            json!(pdf_path.display().to_string()),
            json!(PDF.len()),
            PDF.to_vec(),
            false,
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_download_that_is_not_a_pdf_is_a_quality_failure() {
    let fake = FakeOpenCli::downloading(
        &paper_page("2042750"),
        "paper.pdf",
        b"<html>challenge</html>",
    );
    let provider = Fixture::start_sequence(Vec::new());
    let environment = environment(&fake, &provider);
    let content_dir = tempfile::tempdir().expect("content directory");

    let output = fetch(
        &environment,
        &[
            "--content-dir",
            content_dir.path().to_str().expect("UTF-8 path"),
        ],
    );
    let payload = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            payload["error_kind"].clone(),
            payload["message"]
                .as_str()
                .expect("message")
                .contains("is not a PDF"),
            provider.finish_all().len(),
            fake.path("paper.pdf").exists(),
            files_under(content_dir.path()),
        ),
        (
            Some(5),
            json!("quality"),
            true,
            0,
            true,
            Vec::<String>::new(),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_failed_conversion_keeps_the_pdf_and_names_its_path() {
    let fake = FakeOpenCli::downloading(&paper_page("2042750"), "paper.pdf", PDF);
    let provider = Fixture::start_repeating(Response::new(500, "text/plain", "parse failed"));
    let environment = environment(&fake, &provider);
    let content_dir = tempfile::tempdir().expect("content directory");

    let output = fetch(
        &environment,
        &[
            "--content-dir",
            content_dir.path().to_str().expect("UTF-8 path"),
        ],
    );
    let requests = provider.finish_all();
    let payload = payload(&output);
    let kept = content_dir.path().join("ssrn-2042750.pdf");
    let message = payload["message"].as_str().expect("message").to_owned();

    assert_eq!(
        (
            output.status.code(),
            payload["error_kind"].clone(),
            message.contains(&format!(
                "the downloaded file is kept at {}",
                kept.display()
            )),
            !requests.is_empty(),
            fs::read(&kept).expect("read kept PDF"),
            fake.path("paper.pdf").exists(),
            files_under(content_dir.path()),
        ),
        (
            Some(4),
            json!("network"),
            true,
            true,
            PDF.to_vec(),
            false,
            vec!["ssrn-2042750.pdf".to_owned()],
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_web_fetch_chain_without_local_file_support_fails_and_keeps_the_pdf() {
    let fake = FakeOpenCli::downloading(&paper_page("2042750"), "paper.pdf", PDF);
    let tavily = Fixture::start_canary();
    let environment = RunEnvironment::new(&format!(
        "[providers.tavily]\nurl = {:?}\nkeys = [\"tavily-key\"]\n\n{}",
        tavily.url,
        fake.config("[\"ssrn_browser\"]")
    ));
    let content_dir = tempfile::tempdir().expect("content directory");

    let output = fetch(
        &environment,
        &[
            "--verbose",
            "--content-dir",
            content_dir.path().to_str().expect("UTF-8 path"),
        ],
    );
    let payload = payload(&output);
    let kept = content_dir.path().join("ssrn-2042750.pdf");
    let attempts = payload["provider_attempts"]
        .as_array()
        .expect("attempts")
        .iter()
        .map(|attempt| {
            (
                attempt["provider"].clone(),
                attempt["disposition"].clone(),
                attempt["message"].clone(),
            )
        })
        .collect::<Vec<_>>();

    let message = payload["message"].as_str().expect("message").to_owned();
    let expected_prefix = "no configured web fetch provider can read a local file (application/pdf); the downloaded file is kept at ";

    assert_eq!(
        (
            output.status.code(),
            payload["error_kind"].clone(),
            message.starts_with(expected_prefix),
            attempts,
            kept.exists(),
            tavily.finish_all().len(),
        ),
        (
            Some(4),
            json!("runtime"),
            true,
            vec![
                (json!("ssrn_browser"), json!("succeeded"), json!(""),),
                (
                    json!("tavily"),
                    json!("skipped"),
                    json!("skipped: tavily cannot read a local file (application/pdf)"),
                ),
            ],
            true,
            0,
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_paper_page_without_an_identity_is_a_quality_failure() {
    let mut page = paper_page("2042750");
    page["canonical_url"] = Value::Null;
    page["doi"] = Value::Null;
    let fake = FakeOpenCli::downloading(&page, "paper.pdf", PDF);
    let provider = Fixture::start_sequence(Vec::new());
    let environment = environment(&fake, &provider);
    let content_dir = tempfile::tempdir().expect("content directory");

    let output = fetch(
        &environment,
        &[
            "--content-dir",
            content_dir.path().to_str().expect("UTF-8 path"),
        ],
    );
    let payload = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            payload["error_kind"].clone(),
            payload["message"]
                .as_str()
                .expect("message")
                .contains("shows no SSRN abstract ID"),
            provider.finish_all().len(),
        ),
        (Some(5), json!("quality"), true, 0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn markdown_format_reports_the_kept_pdf() {
    let fake = FakeOpenCli::downloading(&paper_page("2042750"), "paper.pdf", PDF);
    let provider = Fixture::start_sequence(vec![parsed(&markdown())]);
    let environment = environment(&fake, &provider);
    let content_dir = tempfile::tempdir().expect("content directory");

    let output = fetch(
        &environment,
        &[
            "--format",
            "markdown",
            "--keep-pdf",
            "--content-dir",
            content_dir.path().to_str().expect("UTF-8 path"),
        ],
    );
    provider.finish_all();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let pdf_path = content_dir.path().join("ssrn-2042750.pdf");

    assert_eq!(
        (
            output.status.code(),
            stdout.contains(&format!(
                "Original PDF ({} bytes): `{}`",
                PDF.len(),
                pdf_path.display()
            )),
            pdf_path.exists(),
        ),
        (Some(0), true, true),
        "stdout: {stdout}"
    );
}

#[test]
fn format_content_prints_the_markdown_and_writes_no_file() {
    let fake = FakeOpenCli::downloading(&paper_page("2042750"), "paper.pdf", PDF);
    let provider = Fixture::start_sequence(vec![parsed(&markdown())]);
    let environment = environment(&fake, &provider);
    let content_dir = tempfile::tempdir().expect("content directory");

    let output = fetch(
        &environment,
        &[
            "--format",
            "content",
            "--content-dir",
            content_dir.path().to_str().expect("UTF-8 path"),
        ],
    );
    provider.finish_all();

    assert_eq!(
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            fake.path("paper.pdf").exists(),
            files_under(content_dir.path()),
        ),
        (
            Some(0),
            format!("{}\n", markdown()),
            false,
            Vec::<String>::new(),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn format_content_with_keep_pdf_writes_only_the_pdf() {
    let fake = FakeOpenCli::downloading(&paper_page("2042750"), "paper.pdf", PDF);
    let provider = Fixture::start_sequence(vec![parsed(&markdown())]);
    let environment = environment(&fake, &provider);
    let content_dir = tempfile::tempdir().expect("content directory");

    let output = fetch(
        &environment,
        &[
            "--format",
            "content",
            "--keep-pdf",
            "--content-dir",
            content_dir.path().to_str().expect("UTF-8 path"),
        ],
    );
    provider.finish_all();

    assert_eq!(
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            fake.path("paper.pdf").exists(),
            files_under(content_dir.path()),
        ),
        (
            Some(0),
            format!("{}\n", markdown()),
            false,
            vec!["ssrn-2042750.pdf".to_owned()],
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
