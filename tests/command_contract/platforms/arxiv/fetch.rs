use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::process::Output;

use serde_json::{Value, json};

use crate::support::samples::sample;
use crate::support::{Fixture, Response, RunEnvironment, arxiv_feed, payload};

/// The stored feed with its paper renamed `id`.
fn entry_feed(id: &str) -> String {
    arxiv_feed::feed_of(&sample("arxiv_api/feed.xml"), id)
}

fn atom(status: u16, body: &str) -> Response {
    Response::new(status, "application/atom+xml", body)
}

fn head(status: u16) -> Response {
    Response::new(status, "text/html", "")
}

fn body(label: &str) -> String {
    format!(
        "# {label} — α\n\n## Introduction\n\n{}",
        "Paper body text. ".repeat(40)
    )
}

/// A Firecrawl scrape answer whose Markdown is `content`.
fn firecrawl(content: &str) -> Response {
    let mut answer: Value =
        serde_json::from_str(&sample("firecrawl/scrape.json")).expect("stored scrape answer");
    answer["data"]["markdown"] = json!(content);
    Response::json(200, &answer.to_string())
}

/// Configures the arXiv route and, for each named Web Fetch provider, a fixture URL and a key.
/// Named providers become the Web Fetch order, so Jina never runs anonymously against the real
/// service; a test that names none never reaches Web Fetch.
fn config(arxiv: &str, web_fetch: &[(&str, &str)]) -> String {
    let mut config = format!("[providers.arxiv_api]\nurl = \"{arxiv}/api/query\"\n");
    for (provider, url) in web_fetch {
        let _ = write!(
            config,
            "\n[providers.{provider}]\nurl = \"{url}\"\nkeys = [\"{provider}-key\"]\ntimeout = 5\n"
        );
    }
    let order = web_fetch
        .iter()
        .map(|(provider, _)| *provider)
        .collect::<Vec<_>>();
    if !order.is_empty() {
        let _ = write!(config, "\n[capabilities.web_fetch]\norder = {order:?}\n");
    }
    config
}

fn fetch(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "arxiv", "fetch"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

#[test]
fn full_text_fetch_writes_the_body_to_a_file_and_reports_the_resolved_version() {
    let arxiv = Fixture::start_sequence(vec![atom(200, &entry_feed("2401.01234v2")), head(200)]);
    let provider = Fixture::start_sequence(vec![firecrawl(&body("HTML body"))]);
    let environment = RunEnvironment::new(&config(&arxiv.url, &[("firecrawl", &provider.url)]));
    let content_dir = tempfile::tempdir().expect("content directory");
    let content_dir_arg = content_dir.path().to_str().expect("UTF-8 path");

    let output = fetch(
        &environment,
        &["arxiv:2401.01234", "--content-dir", content_dir_arg],
    );
    arxiv.finish_all();
    provider.finish_all();
    let payload = payload(&output);
    let path = payload["content_path"].as_str().expect("content path");
    let written = fs::read_to_string(path).expect("read content file");

    assert_eq!(
        (
            output.status.code(),
            Path::new(path),
            written.as_str(),
            &payload["content_len"],
            payload.get("content").is_none(),
            String::from_utf8_lossy(&output.stdout).contains("Paper body text"),
        ),
        (
            Some(0),
            content_dir.path().join("arxiv-2401.01234v2.md").as_path(),
            body("HTML body").as_str(),
            &json!(body("HTML body").chars().count()),
            true,
            false,
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
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

#[test]
fn full_text_without_a_configured_web_fetch_provider_is_a_config_error() {
    assert_preflight_exit(
        "[providers.arxiv_api]\nurl = \"{url}/api/query\"\n\n[capabilities.web_fetch]\norder = [\"firecrawl\", \"tavily\"]\n",
        &["arxiv:2401.01234"],
        3,
        "capabilities.web_fetch.order has no configured provider",
    );
}
