use std::cell::RefCell;
use std::fmt::Write as _;
use std::rc::Rc;
use std::time::Duration;

use serde_json::{Value, json};

use super::{fetch, is_thin_content, web_fetch_chain_settings};
use crate::chain::{ChainStep, StepSuccess, StepVerdict, run_chain};
use crate::config::runtime_config_from_toml;
use crate::fixture_server::{Fixture, Response, advance_after_request, block_on};
use crate::net::{CONTENT_TRUNCATED_DIAGNOSTIC, MAX_RESPONSE_BYTES, RetryPolicy, build_client};
use crate::providers::{FetchRequest, ProviderId};
use crate::samples::sample;
use crate::types::{AttemptErrorKind, Deadline, FetchOutcome, ProviderAttempt, ProviderError};

const ARTICLE: &str = "https://example.test/article";

/// The stored body at `name` with the string at `pointer` set to `value`.
fn stored_with(name: &str, pointer: &str, value: &str) -> String {
    let mut body: Value = serde_json::from_str(&sample(name)).expect("stored JSON");
    *body.pointer_mut(pointer).expect("stored field") = json!(value);
    body.to_string()
}

fn firecrawl(markdown: &str) -> Response {
    Response::json(
        200,
        &stored_with("firecrawl/scrape.json", "/data/markdown", markdown),
    )
}

fn jina(content: &str) -> Response {
    Response::json(
        200,
        &stored_with("jina/read.json", "/data/content", content),
    )
}

fn tavily(raw_content: &str) -> Response {
    Response::json(
        200,
        &stored_with("tavily/extract.json", "/results/0/raw_content", raw_content),
    )
}

/// A configuration document that points each named Web Fetch provider at its fixture URL with
/// one key, and sets `order` unless it is `None`.
fn document(providers: &[(&str, &str)], order: Option<&[&str]>) -> String {
    let mut document = String::new();
    for (provider, url) in providers {
        let _ = write!(
            document,
            "[providers.{provider}]\nurl = {url:?}\nkeys = [\"{provider}-key\"]\ntimeout = 30\n\n"
        );
    }
    if let Some(order) = order {
        let _ = write!(document, "[capabilities.web_fetch]\norder = {order:?}\n");
    }
    document
}

/// Fetches `url` through the Web Fetch chain of the configuration `document`, with one attempt
/// per request, inside a command deadline of `deadline`.
async fn fetch_within(
    document: &str,
    url: &str,
    deadline: Duration,
) -> Result<FetchOutcome, ProviderError> {
    let config = runtime_config_from_toml(document).expect("valid configuration");
    fetch(
        FetchRequest {
            url: url.into(),
            verbose: true,
        },
        config.web_fetch,
        build_client(true).expect("build client"),
        RetryPolicy::new(1, 1.0, Duration::ZERO),
        Deadline::new(deadline),
    )
    .await
}

fn fetch_through(document: &str, url: &str) -> Result<FetchOutcome, ProviderError> {
    block_on(fetch_within(document, url, Duration::from_secs(30)))
}

/// Each attempt as its provider and error kind, in chain order.
fn chain(attempts: &[ProviderAttempt]) -> Vec<(&'static str, Option<AttemptErrorKind>)> {
    attempts
        .iter()
        .map(|attempt| (attempt.provider, attempt.error_kind))
        .collect()
}

#[test]
fn single_line_content_is_thin_for_html_but_not_pdf() {
    let content = "x".repeat(250);

    assert_eq!(
        (
            is_thin_content(&content, false),
            is_thin_content(&content, true),
        ),
        (true, false)
    );
}

#[test]
fn web_fetch_preserves_fallback_budget_after_an_attempt_timeout() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .expect("test runtime");

    runtime.block_on(async {
        let budgets = Rc::new(RefCell::new(Vec::new()));
        let outcome = run_chain(
            vec![
                ChainStep {
                    context: (ProviderId::Jina, ()),
                    configured: true,
                },
                ChainStep {
                    context: (ProviderId::Tavily, ()),
                    configured: true,
                },
            ],
            web_fetch_chain_settings(true),
            Deadline::new(Duration::from_secs(12)),
            {
                let budgets = Rc::clone(&budgets);
                move |(provider, ()), deadline| {
                    budgets
                        .borrow_mut()
                        .push(deadline.remaining().unwrap_or_default());
                    async move {
                        if provider == ProviderId::Jina {
                            tokio::time::sleep(Duration::from_secs(6)).await;
                            StepVerdict::Failed(ProviderError {
                                kind: AttemptErrorKind::Timeout,
                                message: "timed out".into(),
                                attempts: Vec::new(),
                                verbose: true,
                                diagnostic: None,
                            })
                        } else {
                            StepVerdict::Accepted(StepSuccess {
                                value: "fallback content",
                                attempts: Vec::new(),
                                diagnostic: None,
                            })
                        }
                    }
                }
            },
        )
        .await
        .expect("fallback succeeds");

        assert_eq!(outcome.value, "fallback content");
        let budgets = budgets.borrow();
        assert_eq!(budgets[0], Duration::from_secs(6));
        assert!(
            budgets[1].abs_diff(Duration::from_secs(6)) < Duration::from_millis(1),
            "fallback budget: {:?}",
            budgets[1]
        );
    });
}

#[test]
fn a_fetch_without_an_order_runs_firecrawl_jina_and_tavily_until_the_content_is_not_thin() {
    let firecrawl = Fixture::start_sequence(vec![firecrawl("thin")]);
    let jina = Fixture::start_sequence(vec![jina("also thin")]);
    let tavily = Fixture::start_sequence(vec![tavily(&"Tavily fallback content. ".repeat(30))]);
    let document = document(
        &[
            ("jina", &jina.url),
            ("tavily", &tavily.url),
            ("firecrawl", &firecrawl.url),
        ],
        None,
    );

    let outcome = fetch_through(&document, ARTICLE).expect("tavily's content is accepted");
    for fixture in [firecrawl, jina, tavily] {
        fixture.finish();
    }

    assert_eq!(
        (
            outcome.provider,
            chain(&outcome.attempts),
            outcome
                .attempts
                .iter()
                .map(|attempt| (attempt.anonymous, attempt.message.as_str()))
                .collect::<Vec<_>>(),
        ),
        (
            "tavily",
            vec![
                ("firecrawl", Some(AttemptErrorKind::Quality)),
                ("jina", Some(AttemptErrorKind::Quality)),
                ("tavily", None),
            ],
            vec![
                (false, "extracted content is too thin (4 characters)"),
                (false, "extracted content is too thin (9 characters)"),
                (false, ""),
            ],
        )
    );
}

#[test]
fn a_site_firecrawl_refuses_is_read_through_jina_before_tavily() {
    let firecrawl = Fixture::start_json(403, &sample("firecrawl/error.json"));
    let jina = Fixture::start_sequence(vec![jina(&"Jina article content. ".repeat(30))]);
    let tavily = Fixture::start_canary();
    let document = document(
        &[
            ("jina", &jina.url),
            ("tavily", &tavily.url),
            ("firecrawl", &firecrawl.url),
        ],
        None,
    );

    let outcome = fetch_through(&document, "https://x.com/i/article/2110126570233094527")
        .expect("jina reads the page");
    firecrawl.finish();
    jina.finish();

    assert_eq!(
        (
            outcome.provider,
            chain(&outcome.attempts),
            tavily.finish_all().len()
        ),
        (
            "jina",
            vec![
                ("firecrawl", Some(AttemptErrorKind::Parameter)),
                ("jina", None),
            ],
            0,
        )
    );
}

#[test]
fn a_pdf_needs_only_enough_characters_while_thinner_pdfs_still_fall_back() {
    let content = "P".repeat(250);
    let tavily = Fixture::start_sequence(vec![tavily("thin")]);
    let firecrawl = Fixture::start_sequence(vec![firecrawl("also thin")]);
    let jina = Fixture::start_sequence(vec![jina(&content)]);
    let document = document(
        &[
            ("jina", &jina.url),
            ("tavily", &tavily.url),
            ("firecrawl", &firecrawl.url),
        ],
        Some(&["tavily", "firecrawl", "jina"]),
    );

    let outcome = fetch_through(&document, "https://example.test/report.PDF?download=1")
        .expect("the PDF content is accepted");
    for fixture in [tavily, firecrawl, jina] {
        fixture.finish();
    }

    assert_eq!((outcome.provider, outcome.content), ("jina", content));
}

#[test]
fn truncated_content_that_is_still_thin_falls_back_and_keeps_the_truncation_diagnostic() {
    let tavily_body = format!(
        r#"{{"results":[{{"raw_content":"thin","padding":"{}"#,
        "x".repeat(MAX_RESPONSE_BYTES)
    );
    let tavily = Fixture::start_json(200, &tavily_body);
    let firecrawl = Fixture::start_sequence(vec![firecrawl(
        &"Firecrawl fallback after truncated thin content. ".repeat(20),
    )]);
    let document = document(
        &[("tavily", &tavily.url), ("firecrawl", &firecrawl.url)],
        Some(&["tavily", "firecrawl"]),
    );

    let outcome = fetch_through(&document, ARTICLE).expect("firecrawl's content is accepted");
    tavily.finish();
    firecrawl.finish();

    assert_eq!(
        (
            outcome.provider,
            chain(&outcome.attempts),
            outcome.diagnostic.as_deref(),
        ),
        (
            "firecrawl",
            vec![
                ("tavily", Some(AttemptErrorKind::Quality)),
                ("firecrawl", None),
            ],
            Some(CONTENT_TRUNCATED_DIAGNOSTIC),
        )
    );
}

#[test]
fn a_fetched_page_names_its_url_with_secrets_redacted() {
    let jina = Fixture::start_sequence(vec![jina(&"A substantial fixture paragraph. ".repeat(20))]);
    let document = document(&[("jina", &jina.url)], Some(&["jina"]));

    let outcome = fetch_through(
        &document,
        "https://example.test/source?token=source-secret#fragment",
    )
    .expect("the page is read");
    jina.finish();

    assert_eq!(
        (outcome.provider, outcome.url.as_str()),
        ("jina", "https://example.test/source?token=********")
    );
}

#[test]
fn a_command_deadline_shorter_than_the_attempt_timeout_ends_the_fetch_as_a_timeout() {
    let jina = Fixture::start_unanswered();
    let document = document(&[("jina", &jina.url)], Some(&["jina"]));

    let error = advance_after_request(
        &jina,
        Duration::from_secs(1),
        fetch_within(&document, ARTICLE, Duration::from_secs(1)),
    )
    .expect_err("the command deadline ends the fetch");
    jina.finish();

    assert_eq!(
        (error.kind, chain(&error.attempts)),
        (
            AttemptErrorKind::Timeout,
            vec![("jina", Some(AttemptErrorKind::Timeout))]
        )
    );
}
