//! The full-text stage, proven once through the arXiv route, whose metadata answer declares two
//! full-text URLs.

use std::fmt::Write as _;
use std::time::Duration;

use serde_json::{Value, json};

use super::{fetch, plan_fetch};
use crate::config::runtime_config_from_toml;
use crate::fixture_server::{Fixture, Response};
use crate::net::{RetryPolicy, build_client};
use crate::samples::sample;
use crate::types::{
    AttemptErrorKind, ContentDepth, Deadline, Platform, PlatformFetchRequest, PlatformFetchResult,
    PlatformRef, ProviderError,
};

const HTML: &str = "https://arxiv.org/html/2401.01234v2";
const PDF: &str = "https://arxiv.org/pdf/2401.01234v2";
/// A body Web Fetch refuses as too thin to be the paper.
const THIN: &str = "No HTML.";

/// The arXiv route's metadata answer and HTML probe for the stored paper.
fn arxiv_answers() -> Vec<Response> {
    vec![
        Response::new(200, "application/atom+xml", &sample("arxiv_api/feed.xml")),
        Response::new(200, "text/html", ""),
    ]
}

/// A Firecrawl scrape answer whose Markdown is `markdown`.
fn firecrawl(markdown: &str) -> Response {
    let mut body: Value =
        serde_json::from_str(&sample("firecrawl/scrape.json")).expect("stored scrape answer");
    body["data"]["markdown"] = json!(markdown);
    Response::json(200, &body.to_string())
}

/// A Jina Reader answer whose Markdown is `content`.
fn jina(content: &str) -> Response {
    let mut body: Value =
        serde_json::from_str(&sample("jina/read.json")).expect("stored Reader answer");
    body["data"]["content"] = json!(content);
    Response::json(200, &body.to_string())
}

fn paper(label: &str) -> String {
    format!(
        "# {label}\n\n## Introduction\n\n{}",
        "Paper body text. ".repeat(40)
    )
}

/// Fetches `reference` at `depth` with the arXiv route at `arxiv` and the named Web Fetch
/// providers, in that order, at their fixture URLs.
fn run(
    arxiv: &str,
    web_fetch: &[(&str, &str)],
    reference: &str,
    depth: ContentDepth,
    verbose: bool,
) -> Result<PlatformFetchResult, ProviderError> {
    let mut document = format!("[providers.arxiv_api]\nurl = \"{arxiv}/api/query\"\n");
    for (provider, url) in web_fetch {
        let _ = write!(
            document,
            "\n[providers.{provider}]\nurl = \"{url}\"\nkeys = [\"{provider}-key\"]\ntimeout = 5\n"
        );
    }
    let order = web_fetch
        .iter()
        .map(|(provider, _)| *provider)
        .collect::<Vec<_>>();
    if !order.is_empty() {
        let _ = write!(document, "\n[capabilities.web_fetch]\norder = {order:?}\n");
    }
    let runtime = runtime_config_from_toml(&document).expect("valid configuration");
    let request = PlatformFetchRequest::new(
        PlatformRef::parse(Platform::Arxiv, reference).expect("valid ref"),
        depth,
    );
    let plan = plan_fetch(runtime.platforms.get(Platform::Arxiv), request).expect("plan");
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime")
        .block_on(fetch(
            plan,
            runtime.web_fetch,
            build_client(true).expect("build client"),
            RetryPolicy::new(1, 1.0, Duration::ZERO),
            Deadline::new(Duration::from_mins(1)),
            verbose,
        ))
}

fn fetched_url(request: &str) -> String {
    let (_, body) = request.split_once("\r\n\r\n").expect("request body");
    serde_json::from_str::<Value>(body).expect("request JSON")["url"]
        .as_str()
        .expect("fetched URL")
        .to_owned()
}

#[test]
fn a_full_text_url_whose_body_is_too_thin_gives_way_to_the_next() {
    let arxiv = Fixture::start_sequence(arxiv_answers());
    let firecrawl_fixture =
        Fixture::start_sequence(vec![firecrawl(THIN), firecrawl(&paper("PDF"))]);

    let result = run(
        &arxiv.url,
        &[("firecrawl", &firecrawl_fixture.url)],
        "arxiv:2401.01234",
        ContentDepth::FullText,
        false,
    )
    .expect("the PDF is read");
    arxiv.finish_all();
    let content = result.content.expect("full text");

    assert_eq!(
        (
            firecrawl_fixture
                .finish_all()
                .iter()
                .map(|request| fetched_url(request))
                .collect::<Vec<_>>(),
            content.url,
            content.text,
        ),
        (
            vec![HTML.to_owned(), PDF.to_owned()],
            PDF.to_owned(),
            paper("PDF")
        )
    );
}

#[test]
fn when_every_full_text_url_fails_the_fetch_ends_with_the_web_fetch_terminal_state() {
    let arxiv = Fixture::start_sequence(arxiv_answers());
    let firecrawl_fixture = Fixture::start_sequence(vec![firecrawl(THIN), firecrawl(THIN)]);

    let error = run(
        &arxiv.url,
        &[("firecrawl", &firecrawl_fixture.url)],
        "arxiv:2401.01234",
        ContentDepth::FullText,
        false,
    )
    .expect_err("no URL gives a full text");
    arxiv.finish_all();
    firecrawl_fixture.finish_all();

    assert_eq!(error.kind, AttemptErrorKind::Quality);
}

#[test]
fn each_full_text_url_runs_the_global_web_fetch_order() {
    let arxiv = Fixture::start_sequence(arxiv_answers());
    let jina_fixture = Fixture::start_sequence(vec![jina(&paper("Jina"))]);
    let firecrawl_fixture = Fixture::start_sequence(Vec::new());

    let result = run(
        &arxiv.url,
        &[
            ("jina", &jina_fixture.url),
            ("firecrawl", &firecrawl_fixture.url),
        ],
        "arxiv:2401.01234",
        ContentDepth::FullText,
        false,
    )
    .expect("Jina reads the HTML");
    arxiv.finish_all();
    let jina_request = jina_fixture.finish();
    firecrawl_fixture.finish_all();

    assert_eq!(
        (
            jina_request.lines().next(),
            result
                .content
                .map(|content| (content.provider, content.text)),
        ),
        (
            Some("GET /https://arxiv.org/html/2401.01234v2 HTTP/1.1"),
            Some(("jina", paper("Jina"))),
        )
    );
}

#[test]
fn a_verbose_fetch_lists_the_route_attempts_before_the_web_fetch_attempts() {
    let arxiv = Fixture::start_sequence(vec![
        Response::new(200, "application/atom+xml", &sample("arxiv_api/feed.xml")),
        Response::new(404, "text/html", ""),
    ]);
    let firecrawl_fixture = Fixture::start_sequence(vec![firecrawl(&paper("PDF"))]);

    let result = run(
        &arxiv.url,
        &[("firecrawl", &firecrawl_fixture.url)],
        "arxiv:2401.01234",
        ContentDepth::FullText,
        true,
    )
    .expect("the PDF is read");
    arxiv.finish_all();
    firecrawl_fixture.finish_all();
    let attempts = result
        .attempts
        .iter()
        .map(|attempt| {
            (
                attempt.provider,
                attempt.http_status,
                serde_json::to_value(attempt.disposition).expect("disposition"),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        attempts,
        [
            ("arxiv_api", Some(200), json!("succeeded")),
            ("arxiv_api", Some(404), json!("succeeded")),
            ("firecrawl", Some(200), json!("succeeded")),
        ]
    );
}

#[test]
fn an_abstract_fetch_needs_no_web_fetch_provider_and_carries_no_content() {
    let arxiv = Fixture::start_sequence(arxiv_answers().into_iter().take(1).collect());

    let result = run(
        &arxiv.url,
        &[],
        "arxiv:2401.01234v2",
        ContentDepth::Abstract,
        false,
    )
    .expect("the abstract is read");

    assert_eq!(
        (
            arxiv.finish_all().len(),
            result.item.depth,
            result.content.is_none()
        ),
        (1, ContentDepth::Abstract, true)
    );
}

#[test]
fn only_a_full_text_fetch_through_a_route_without_a_native_body_needs_web_fetch() {
    let runtime = runtime_config_from_toml("").expect("default configuration");
    let needs = [ContentDepth::Abstract, ContentDepth::FullText].map(|depth| {
        let request = PlatformFetchRequest::new(
            PlatformRef::parse(Platform::Arxiv, "arxiv:2401.01234").expect("valid ref"),
            depth,
        );
        plan_fetch(runtime.platforms.get(Platform::Arxiv), request)
            .expect("plan")
            .needs_web_fetch()
    });

    assert_eq!(needs, [false, true]);
}
