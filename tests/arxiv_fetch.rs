mod support;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::process::Output;

use serde_json::{Value, json};

use support::{Fixture, Response, RunEnvironment, request_json};

const TITLE: &str = "Dark Matter Halos";

fn entry_feed(id: &str) -> String {
    format!(
        r#"<?xml version='1.0' encoding='UTF-8'?>
<feed xmlns:opensearch="http://a9.com/-/spec/opensearch/1.1/" xmlns:arxiv="http://arxiv.org/schemas/atom" xmlns="http://www.w3.org/2005/Atom">
  <opensearch:totalResults>1</opensearch:totalResults>
  <entry>
    <id>http://arxiv.org/abs/{id}</id>
    <title>{TITLE}</title>
    <summary>Abstract of {id}.</summary>
    <published>2024-01-02T18:59:59Z</published>
    <updated>2024-01-03T00:00:00Z</updated>
    <author><name>Ada Lovelace</name></author>
    <link href="https://arxiv.org/pdf/{id}" rel="related" type="application/pdf" title="pdf"/>
    <arxiv:primary_category term="astro-ph.CO"/>
    <category term="astro-ph.CO" scheme="http://arxiv.org/schemas/atom"/>
  </entry>
</feed>"#
    )
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

fn firecrawl(content: &str) -> Response {
    Response::json(
        200,
        &json!({"success": true, "data": {"markdown": content}}).to_string(),
    )
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

/// Runs a fetch whose system temporary directory is `temp`.
fn fetch_with_temp(environment: &RunEnvironment, arguments: &[&str], temp: &Path) -> Output {
    let mut command = vec!["platform", "arxiv", "fetch"];
    command.extend_from_slice(arguments);
    let temp = temp.to_str().expect("UTF-8 path");
    environment.run_with_env(&command, &[("TMPDIR", temp), ("TMP", temp), ("TEMP", temp)])
}

fn files_under(directory: &Path) -> Vec<String> {
    walk(directory)
        .into_iter()
        .map(|path| {
            path.strip_prefix(directory)
                .expect("nested path")
                .display()
                .to_string()
        })
        .collect()
}

fn walk(directory: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory).expect("read directory") {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            files.extend(walk(&path));
        } else {
            files.push(path);
        }
    }
    files
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

fn query_pairs(request: &str) -> BTreeMap<String, String> {
    let target = request.split_whitespace().nth(1).expect("request target");
    reqwest::Url::parse(&format!("http://fixture.test{target}"))
        .expect("request URL")
        .query_pairs()
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}

fn fetched_urls(requests: &[String]) -> Vec<String> {
    requests
        .iter()
        .map(|request| {
            request_json(request)["url"]
                .as_str()
                .expect("fetched url")
                .to_owned()
        })
        .collect()
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
    let arxiv_requests = arxiv.finish_all();
    let fetched = fetched_urls(&provider.finish_all());
    let payload = payload(&output);
    let path = payload["content_path"].as_str().expect("content path");
    let written = fs::read_to_string(path).expect("read content file");

    assert_eq!(
        (
            output.status.code(),
            query_pairs(&arxiv_requests[0]).get("id_list").cloned(),
            request_line(&arxiv_requests[1]),
            fetched,
            Path::new(path),
            written.as_str(),
            &payload["content_len"],
            payload.get("content").is_none(),
            String::from_utf8_lossy(&output.stdout).contains("Paper body text"),
        ),
        (
            Some(0),
            Some("2401.01234".to_owned()),
            "HEAD /html/2401.01234v2 HTTP/1.1",
            vec!["https://arxiv.org/html/2401.01234v2".to_owned()],
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

#[test]
fn full_text_fetch_returns_metadata_and_a_file_under_the_system_temporary_directory() {
    let arxiv = Fixture::start_sequence(vec![atom(200, &entry_feed("2401.01234v2")), head(200)]);
    let provider = Fixture::start_sequence(vec![firecrawl(&body("HTML body"))]);
    let environment = RunEnvironment::new(&config(&arxiv.url, &[("firecrawl", &provider.url)]));
    let temp = tempfile::tempdir().expect("temporary directory");

    let output = fetch_with_temp(
        &environment,
        &["https://arxiv.org/abs/2401.01234"],
        temp.path(),
    );
    arxiv.finish_all();
    provider.finish_all();
    let mut payload = payload(&output);
    let path = payload["content_path"]
        .as_str()
        .expect("content path")
        .to_owned();
    payload["content_path"] = Value::Null;

    assert_eq!(
        (
            Path::new(&path).starts_with(temp.path().join("forager-platform")),
            Path::new(&path).ends_with("arxiv-2401.01234v2.md"),
        ),
        (true, true),
        "content path: {path}"
    );
    assert_eq!(
        payload,
        json!({
            "platform": "arxiv",
            "provider": "arxiv_api",
            "ref": "arxiv:2401.01234v2",
            "url": "https://arxiv.org/abs/2401.01234v2",
            "depth": "full_text",
            "title": TITLE,
            "authors": ["Ada Lovelace"],
            "published": "2024-01-02T18:59:59Z",
            "abstract": "Abstract of 2401.01234v2.",
            "updated": "2024-01-03T00:00:00Z",
            "primary_category": "astro-ph.CO",
            "categories": ["astro-ph.CO"],
            "doi": null,
            "journal_ref": null,
            "comment": null,
            "pdf_url": "https://arxiv.org/pdf/2401.01234v2",
            "content_url": "https://arxiv.org/html/2401.01234v2",
            "content_provider": "firecrawl",
            "content_path": null,
            "content_len": body("HTML body").chars().count(),
        })
    );
}

#[test]
fn format_content_prints_the_body_and_writes_no_file() {
    let arxiv = Fixture::start_sequence(vec![atom(200, &entry_feed("2401.01234v2")), head(200)]);
    let provider = Fixture::start_sequence(vec![firecrawl(&body("HTML body"))]);
    let environment = RunEnvironment::new(&config(&arxiv.url, &[("firecrawl", &provider.url)]));
    let temp = tempfile::tempdir().expect("temporary directory");

    let output = fetch_with_temp(
        &environment,
        &["arxiv:2401.01234", "--format", "content"],
        temp.path(),
    );
    arxiv.finish_all();
    provider.finish_all();

    assert_eq!(
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            files_under(temp.path()),
        ),
        (
            Some(0),
            format!("{}\n", body("HTML body")),
            Vec::<String>::new()
        )
    );
}

#[test]
fn an_unwritable_content_directory_is_a_runtime_failure() {
    let arxiv = Fixture::start_sequence(vec![atom(200, &entry_feed("2401.01234v2")), head(200)]);
    let provider = Fixture::start_sequence(vec![firecrawl(&body("HTML body"))]);
    let environment = RunEnvironment::new(&config(&arxiv.url, &[("firecrawl", &provider.url)]));
    let blocker = tempfile::NamedTempFile::new().expect("blocking file");

    let output = fetch(
        &environment,
        &[
            "arxiv:2401.01234",
            "--content-dir",
            blocker.path().to_str().expect("UTF-8 path"),
        ],
    );
    arxiv.finish_all();
    provider.finish_all();
    let payload = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            String::from_utf8_lossy(&output.stdout).contains("Paper body text"),
        ),
        (Some(4), &json!("runtime"), false)
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

#[test]
fn an_empty_platform_order_is_a_config_error() {
    assert_preflight_exit(
        "[providers.arxiv_api]\nurl = \"{url}/api/query\"\n\n[platforms.arxiv]\norder = []\n",
        &["arxiv:2401.01234", "--depth", "abstract"],
        3,
        "platforms.arxiv.order has no configured route for arxiv fetch",
    );
}

#[test]
fn unrecognized_references_and_short_links_fail_before_any_request() {
    for input in [
        "https://bit.ly/abc123",
        "2401.01234",
        "https://arxiv.org/list/cs.AI/recent",
        "arxiv:2401.123",
    ] {
        assert_preflight_exit(
            "[providers.arxiv_api]\nurl = \"{url}/api/query\"\n",
            &[input, "--depth", "abstract"],
            2,
            "pass an `arxiv:<id>[v<n>]` ref or an original arxiv.org URL",
        );
    }
}
