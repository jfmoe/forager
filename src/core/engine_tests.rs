use std::cell::RefCell;
use std::fmt::Write as _;
use std::rc::Rc;
use std::time::Duration;

use serde_json::{Value, json};

use super::{
    CapabilityExecution, fetch, is_thin_content, search, supplemental_web_search,
    web_fetch_chain_settings,
};
use crate::chain::{ChainStep, StepSuccess, StepVerdict, run_chain};
use crate::config::runtime_config_from_toml;
use crate::fixture_server::{
    Fixture, Response, advance_after_request, block_on, block_on_paused, request_json,
};
use crate::main_search_samples::{answer_with, completed_events, stored_answer};
use crate::net::{CONTENT_TRUNCATED_DIAGNOSTIC, MAX_RESPONSE_BYTES, RetryPolicy, build_client};
use crate::providers::{FetchRequest, MainSearchRequest, ProviderId};
use crate::samples::sample;
use crate::types::{
    AttemptErrorKind, Deadline, FallbackPolicy, FetchOutcome, ProviderAttempt, ProviderError,
    SearchOutcome, SupplementalSearchOutcome,
};
use crate::web_samples::{jina, stored_with, tavily_results};

const ARTICLE: &str = "https://example.test/article";

fn firecrawl(markdown: &str) -> Response {
    Response::json(
        200,
        &stored_with("firecrawl/scrape.json", &[("/data/markdown", markdown)]),
    )
}

fn tavily(raw_content: &str) -> Response {
    Response::json(
        200,
        &stored_with(
            "tavily/extract.json",
            &[("/results/0/raw_content", raw_content)],
        ),
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
    block_on_paused(async {
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

/// A configuration document that points each named Web Search provider at its fixture URL with
/// one key and orders the Web Search chain as named.
fn web_search_document(providers: &[(&str, &str)]) -> String {
    let order = providers.iter().map(|(name, _)| *name).collect::<Vec<_>>();
    let mut document = String::new();
    for (provider, url) in providers {
        let _ = write!(
            document,
            "[providers.{provider}]\nurl = {url:?}\nkeys = [\"{provider}-key\"]\ntimeout = 30\n\n"
        );
    }
    let _ = write!(document, "[capabilities.web_search]\norder = {order:?}\n");
    document
}

/// Runs the Web Search chain of the configuration `document` for three sources, with one
/// attempt per request.
fn web_search(
    document: &str,
    fallback: FallbackPolicy,
) -> Result<SupplementalSearchOutcome, ProviderError> {
    let config = runtime_config_from_toml(document).expect("valid configuration");
    block_on(supplemental_web_search(
        "Current information",
        3,
        config.web_search,
        CapabilityExecution::new(
            fallback,
            build_client(true).expect("build client"),
            RetryPolicy::new(1, 1.0, Duration::ZERO),
            Deadline::new(Duration::from_secs(30)),
        ),
    ))
}

fn firecrawl_results(results: &Value) -> Response {
    let mut body: Value =
        serde_json::from_str(&sample("firecrawl/search.json")).expect("stored JSON");
    body["data"]["web"] = results.clone();
    Response::json(200, &body.to_string())
}

fn source_urls(outcome: &SupplementalSearchOutcome) -> Vec<&str> {
    outcome
        .sources
        .iter()
        .map(|source| source.url.as_str())
        .collect()
}

#[test]
fn an_empty_web_search_answer_falls_back_to_the_next_providers_results() {
    let tavily = Fixture::start_sequence(vec![tavily_results(&json!([]))]);
    let firecrawl = Fixture::start_json(200, &sample("firecrawl/search.json"));
    let document = web_search_document(&[("tavily", &tavily.url), ("firecrawl", &firecrawl.url)]);

    let outcome = web_search(&document, FallbackPolicy::Auto).expect("firecrawl answers");
    tavily.finish();
    firecrawl.finish();

    assert_eq!(
        (source_urls(&outcome), chain(&outcome.attempts)),
        (
            vec!["https://example.test/fallback"],
            vec![("tavily", None), ("firecrawl", None)],
        )
    );
}

#[test]
fn a_web_search_whose_every_provider_is_empty_succeeds_empty_with_every_attempt() {
    let tavily = Fixture::start_sequence(vec![tavily_results(&json!([]))]);
    let firecrawl = Fixture::start_sequence(vec![firecrawl_results(&json!([]))]);
    let document = web_search_document(&[("tavily", &tavily.url), ("firecrawl", &firecrawl.url)]);

    let outcome = web_search(&document, FallbackPolicy::Auto).expect("an empty search succeeds");
    tavily.finish();
    firecrawl.finish();

    assert_eq!(
        (source_urls(&outcome), chain(&outcome.attempts)),
        (
            Vec::<&str>::new(),
            vec![("tavily", None), ("firecrawl", None)]
        )
    );
}

#[test]
fn a_web_search_with_fallback_off_returns_the_chain_heads_empty_answer() {
    let tavily = Fixture::start_sequence(vec![tavily_results(&json!([]))]);
    let firecrawl = Fixture::start_canary();
    let document = web_search_document(&[("tavily", &tavily.url), ("firecrawl", &firecrawl.url)]);

    let outcome = web_search(&document, FallbackPolicy::Off).expect("the head's empty answer");
    tavily.finish();

    assert_eq!(
        (
            source_urls(&outcome),
            chain(&outcome.attempts),
            firecrawl.finish_all().len(),
        ),
        (Vec::<&str>::new(), vec![("tavily", None)], 0)
    );
}

#[test]
fn a_malformed_web_search_answer_is_a_runtime_failure_that_falls_back() {
    let cases = [
        (
            ("tavily", Response::json(200, r#"{"results":{}}"#)),
            (
                "firecrawl",
                firecrawl_results(&json!([{"url": "https://example.test/fallback"}])),
            ),
        ),
        (
            ("firecrawl", Response::json(200, r#"{"data":[]}"#)),
            (
                "tavily",
                tavily_results(&json!([{"url": "https://example.test/fallback"}])),
            ),
        ),
    ];

    for ((head, malformed), (next, answer)) in cases {
        let head_fixture = Fixture::start_sequence(vec![malformed]);
        let next_fixture = Fixture::start_sequence(vec![answer]);
        let document = web_search_document(&[(head, &head_fixture.url), (next, &next_fixture.url)]);

        let outcome =
            web_search(&document, FallbackPolicy::Auto).expect("the next provider answers");
        head_fixture.finish();
        next_fixture.finish();

        assert_eq!(
            (source_urls(&outcome), chain(&outcome.attempts)),
            (
                vec!["https://example.test/fallback"],
                vec![(head, Some(AttemptErrorKind::Runtime)), (next, None)],
            ),
            "{head} first"
        );
    }
}

/// A configuration document that orders the main-search chain as `backends`, giving each backend
/// with a fixture URL its own model and one key; a backend without a URL has no key.
fn main_search_document(backends: &[(&str, Option<&str>)]) -> String {
    let order = backends.iter().map(|(name, _)| *name).collect::<Vec<_>>();
    let mut document = format!("[search]\nbackends = {order:?}\n\n");
    for (backend, url) in backends {
        let Some(url) = url else { continue };
        let tools = if *backend == "xai" {
            "tools = [\"web_search\"]\n"
        } else {
            ""
        };
        let _ = write!(
            document,
            "[providers.{backend}]\nurl = \"{url}/v1\"\nkeys = [\"{backend}-key\"]\nmodel = \"{backend}-model\"\n{tools}\n"
        );
    }
    document
}

/// Runs the main-search chain of the configuration `document` under `fallback`, with one
/// attempt per request and the request's `model` override.
fn main_search(
    document: &str,
    model: Option<&str>,
    fallback: FallbackPolicy,
) -> Result<SearchOutcome, ProviderError> {
    let config = runtime_config_from_toml(document).expect("valid configuration");
    block_on(search(
        MainSearchRequest {
            query: "What changed?".into(),
            model: model.map(str::to_owned),
            verbose: true,
        },
        config.main_search,
        fallback,
        build_client(true).expect("build client"),
        RetryPolicy::new(1, 1.0, Duration::ZERO),
        Deadline::new(Duration::from_secs(30)),
    ))
}

#[test]
fn a_command_deadline_ends_main_search_as_a_timeout_that_keeps_its_attempt() {
    let xai = Fixture::start_unanswered();
    let config = runtime_config_from_toml(&main_search_document(&[("xai", Some(&xai.url))]))
        .expect("valid configuration");

    let error = advance_after_request(&xai, Duration::from_secs(1), async {
        search(
            MainSearchRequest {
                query: "Budget".into(),
                model: None,
                verbose: true,
            },
            config.main_search,
            FallbackPolicy::Auto,
            build_client(true).expect("build client"),
            RetryPolicy::new(1, 1.0, Duration::ZERO),
            Deadline::new(Duration::from_secs(1)),
        )
        .await
    })
    .expect_err("the command deadline ends the search");
    xai.finish();

    assert_eq!(
        (error.kind, statuses(&error.attempts)),
        (
            AttemptErrorKind::Timeout,
            vec![("xai", Some(AttemptErrorKind::Timeout), None)]
        )
    );
}

fn answer(provider: &str) -> Response {
    Response::sse(200, &completed_events(&stored_answer(provider)))
}

fn overloaded() -> Response {
    Response::json(503, r#"{"error":{"message":"overloaded"}}"#)
}

/// Each attempt as its provider, error kind and HTTP status, in chain order.
fn statuses(
    attempts: &[ProviderAttempt],
) -> Vec<(&'static str, Option<AttemptErrorKind>, Option<u16>)> {
    attempts
        .iter()
        .map(|attempt| (attempt.provider, attempt.error_kind, attempt.http_status))
        .collect()
}

#[test]
fn main_search_falls_back_from_a_failed_xai_to_openai() {
    let xai = Fixture::start_sequence(vec![overloaded()]);
    let openai = Fixture::start_sequence(vec![answer("openai")]);
    let document = main_search_document(&[("xai", Some(&xai.url)), ("openai", Some(&openai.url))]);

    let outcome = main_search(&document, None, FallbackPolicy::Auto).expect("openai answers");
    xai.finish();
    openai.finish();

    assert_eq!(
        (outcome.provider, statuses(&outcome.attempts)),
        (
            "openai",
            vec![
                ("xai", Some(AttemptErrorKind::Network), Some(503)),
                ("openai", None, Some(200)),
            ],
        )
    );
}

#[test]
fn main_search_falls_back_when_xai_normalization_leaves_no_answer() {
    let hidden = answer_with(
        "xai",
        "<think>hidden only</think>",
        &[("https://example.test/hidden", "Source")],
    );
    let xai = Fixture::start_sequence(vec![Response::sse(200, &completed_events(&hidden))]);
    let openai = Fixture::start_sequence(vec![answer("openai")]);
    let document = main_search_document(&[("xai", Some(&xai.url)), ("openai", Some(&openai.url))]);

    let outcome = main_search(&document, None, FallbackPolicy::Auto).expect("openai answers");
    xai.finish();
    openai.finish();

    assert_eq!(
        (outcome.provider, chain(&outcome.attempts)),
        (
            "openai",
            vec![("xai", Some(AttemptErrorKind::Runtime)), ("openai", None)],
        )
    );
}

#[test]
fn main_search_with_fallback_off_ends_at_a_failed_head() {
    let xai = Fixture::start_sequence(vec![overloaded()]);
    let openai = Fixture::start_canary();
    let document = main_search_document(&[("xai", Some(&xai.url)), ("openai", Some(&openai.url))]);

    let error = main_search(&document, None, FallbackPolicy::Off).expect_err("the head fails");
    xai.finish();

    assert_eq!(
        (
            error.kind,
            chain(&error.attempts),
            openai.finish_all().len()
        ),
        (
            AttemptErrorKind::Network,
            vec![("xai", Some(AttemptErrorKind::Network))],
            0,
        )
    );
}

#[test]
fn an_openai_first_order_answers_without_sending_to_xai() {
    let openai = Fixture::start_sequence(vec![answer("openai")]);
    let xai = Fixture::start_canary();
    let document = main_search_document(&[("openai", Some(&openai.url)), ("xai", Some(&xai.url))]);

    let outcome = main_search(&document, None, FallbackPolicy::Auto).expect("openai answers");
    openai.finish();

    assert_eq!(
        (
            outcome.provider,
            chain(&outcome.attempts),
            xai.finish_all().len()
        ),
        ("openai", vec![("openai", None)], 0)
    );
}

#[test]
fn a_model_override_reaches_only_the_backend_that_runs_first() {
    let xai = Fixture::start_sequence(vec![overloaded()]);
    let openai = Fixture::start_sequence(vec![answer("openai")]);
    let document = main_search_document(&[("xai", Some(&xai.url)), ("openai", Some(&openai.url))]);

    main_search(&document, Some("head-model"), FallbackPolicy::Auto).expect("openai answers");

    assert_eq!(
        (
            request_json(&xai.finish())["model"].clone(),
            request_json(&openai.finish())["model"].clone(),
        ),
        (json!("head-model"), json!("openai-model"))
    );
}

#[test]
fn a_model_override_reaches_the_first_configured_backend_past_a_head_without_keys() {
    let openai = Fixture::start_sequence(vec![answer("openai")]);
    let document = main_search_document(&[("xai", None), ("openai", Some(&openai.url))]);

    let outcome =
        main_search(&document, Some("head-model"), FallbackPolicy::Auto).expect("openai answers");

    assert_eq!(
        (
            outcome.provider,
            request_json(&openai.finish())["model"].clone(),
        ),
        ("openai", json!("head-model"))
    );
}
