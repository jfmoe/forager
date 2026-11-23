use std::fmt::Write as _;
use std::time::Duration;

use serde_json::{Value, json};

use super::{CapabilityBranch, CapabilityResults, CapabilityTargets, execute_capabilities};
use crate::config::runtime_config_from_toml;
use crate::engine::CapabilityExecution;
use crate::fixture_server::{Fixture, Response, Route, block_on};
use crate::net::{RetryPolicy, build_client};
use crate::samples::sample;
use crate::types::{
    AttemptErrorKind, CapabilitySet, Deadline, FallbackPolicy, SearchCandidate, SearchOutcome,
};

/// Runs a declared documentation search over the configuration `document` and returns its
/// branch.
fn docs_branch(document: &str) -> CapabilityBranch {
    declared_branch("docs_search", "Rust ownership", document)
}

/// Runs the one declared `capability` for `query` over the configuration `document` and
/// returns its branch.
fn declared_branch(capability: &str, query: &str, document: &str) -> CapabilityBranch {
    let config = runtime_config_from_toml(document).expect("valid configuration");
    let capabilities: CapabilitySet = capability.parse().expect("capability set");
    let execution = CapabilityExecution::new(
        FallbackPolicy::Auto,
        build_client(true).expect("build client"),
        RetryPolicy::new(1, 1.0, Duration::ZERO),
        Deadline::new(Duration::from_secs(30)),
    );
    let mut results = block_on(execute_capabilities(
        query,
        &capabilities,
        0,
        &config,
        execution,
    ));
    assert_eq!(results.branches.len(), 1, "one declared capability");
    results.branches.remove(0)
}

fn exa_document(url: &str) -> String {
    format!("[providers.exa]\nurl = {url:?}\nkeys = [\"exa-key\"]\n")
}

/// The branch's candidate URLs, its gaps and the providers of its attempts.
fn branch_summary(branch: &CapabilityBranch) -> (Vec<String>, Value, Vec<&'static str>) {
    (
        branch
            .sources
            .iter()
            .map(|candidate| candidate.url().to_owned())
            .collect(),
        serde_json::to_value(&branch.capability_gaps).expect("encode gaps"),
        branch
            .attempts
            .iter()
            .map(|attempt| attempt.provider)
            .collect(),
    )
}

#[test]
fn a_docs_search_runs_exa_by_default_and_by_its_configured_order() {
    let summaries = [
        String::new(),
        "[capabilities.docs_search]\norder = [\"exa\"]\n".to_owned(),
    ]
    .map(|order| {
        let exa = Fixture::start_json(200, &sample("exa/search.json"));
        let branch = docs_branch(&format!("{}{order}", exa_document(&exa.url)));
        exa.finish();
        branch_summary(&branch)
    });

    let expected = (
        vec!["https://example.test/result?api_key=********&safe=1".to_owned()],
        json!([]),
        vec!["exa"],
    );
    assert_eq!(summaries, [expected.clone(), expected]);
}

#[test]
fn a_docs_search_whose_exa_answer_is_refused_becomes_a_capability_gap() {
    let mut body: Value =
        serde_json::from_str(&sample("exa/search.json")).expect("stored Exa JSON");
    body["results"][0]["url"] = json!("exa:result-id");
    let exa = Fixture::start_json(200, &body.to_string());

    let branch = docs_branch(&exa_document(&exa.url));
    exa.finish();

    assert_eq!(
        branch_summary(&branch),
        (
            Vec::new(),
            json!([{
                "capability": "docs_search",
                "reason": "all_attempts_failed",
                "providers_skipped": []
            }]),
            vec!["exa"],
        )
    );
}

#[test]
fn capability_targets_preserve_branch_local_defaults_and_positive_requests() {
    let cases = [(0, (3, 1)), (1, (1, 1)), (20, (20, 20))];

    for (requested, expected) in cases {
        let targets = CapabilityTargets::new(requested);

        assert_eq!(
            (targets.web, targets.documentation),
            expected,
            "requested={requested}"
        );
    }
}

/// A configuration document that points each named Web Fetch provider at its fixture URL
/// with `keys` and orders the Web Fetch chain as named.
fn web_fetch_document(providers: &[(&str, &str)], keys: &[&str]) -> String {
    let order = providers.iter().map(|(name, _)| *name).collect::<Vec<_>>();
    let mut document = String::new();
    for (name, url) in providers {
        let _ = write!(
            document,
            "[providers.{name}]\nurl = {url:?}\nkeys = {keys:?}\ntimeout = 30\n\n"
        );
    }
    let _ = write!(document, "[capabilities.web_fetch]\norder = {order:?}\n");
    document
}

/// The stored body at `name` with the string at `pointer` set to `value`.
fn stored_with(name: &str, pointer: &str, value: &str) -> String {
    let mut body: Value = serde_json::from_str(&sample(name)).expect("stored JSON");
    *body.pointer_mut(pointer).expect("stored field") = json!(value);
    body.to_string()
}

fn jina(content: &str) -> Response {
    Response::json(
        200,
        &stored_with("jina/read.json", "/data/content", content),
    )
}

fn evidence() -> String {
    "Fetched evidence line one.\nFetched evidence line two.\n".repeat(12)
}

/// Each candidate as its URL, provider and summary.
fn previews(candidates: &[SearchCandidate]) -> Vec<(String, &'static str, Option<String>)> {
    candidates
        .iter()
        .map(|candidate| {
            (
                candidate.url().to_owned(),
                candidate.provider(),
                candidate.summary().map(str::to_owned),
            )
        })
        .collect()
}

fn candidate_urls(branch: &CapabilityBranch) -> Vec<&str> {
    branch.sources.iter().map(SearchCandidate::url).collect()
}

#[test]
fn a_declared_web_fetch_adds_a_content_preview_candidate_without_changing_the_answer() {
    let tavily = Fixture::start_json(
        200,
        &stored_with("tavily/extract.json", "/results/0/raw_content", "thin"),
    );
    let firecrawl = Fixture::start_json(
        200,
        &stored_with("firecrawl/scrape.json", "/data/markdown", "also thin"),
    );
    let jina = Fixture::start_sequence(vec![jina(&evidence())]);
    let document = web_fetch_document(
        &[
            ("tavily", &tavily.url),
            ("firecrawl", &firecrawl.url),
            ("jina", &jina.url),
        ],
        &["fetch-key"],
    );

    let branch = declared_branch(
        "web_fetch",
        "Verify https://example.test/article",
        &document,
    );
    tavily.finish();
    firecrawl.finish();
    let request = jina.finish();
    let candidates = previews(&branch.sources);
    let gaps = serde_json::to_value(&branch.capability_gaps).expect("encode gaps");
    let mut outcome = SearchOutcome {
        provider: "xai",
        answer: "answer".into(),
        sources: Vec::new(),
        extra_sources: Vec::new(),
        capabilities: Vec::new(),
        capability_gaps: Vec::new(),
        attempts: Vec::new(),
        diagnostic: None,
    };
    CapabilityResults {
        branches: vec![branch],
    }
    .merge_into(&mut outcome);
    let preview = (
        "https://example.test/article".to_owned(),
        "jina",
        Some(evidence().chars().take(300).collect::<String>()),
    );

    assert_eq!(
        (
            request.lines().next(),
            candidates,
            gaps,
            outcome.answer.as_str(),
            previews(&outcome.extra_sources),
        ),
        (
            Some("GET /https://example.test/article HTTP/1.1"),
            vec![preview.clone()],
            json!([]),
            "answer",
            vec![preview],
        )
    );
}

#[test]
fn a_declared_web_fetch_reports_a_partial_gap_when_one_target_succeeds() {
    let jina = Fixture::start_routed(vec![
        Route::new("GET", "/https://example.test/one", jina(&evidence())),
        Route::new(
            "GET",
            "/https://example.test/two",
            Response::json(
                503,
                &stored_with("jina/error.json", "/message", "unavailable"),
            ),
        ),
    ]);
    let document = web_fetch_document(&[("jina", &jina.url)], &["jina-key"]);

    let branch = declared_branch(
        "web_fetch",
        "Verify https://example.test/one https://example.test/two",
        &document,
    );
    jina.finish_all();

    assert_eq!(
        (
            candidate_urls(&branch),
            branch
                .sources
                .iter()
                .map(SearchCandidate::provider)
                .collect::<Vec<_>>(),
            serde_json::to_value(&branch.capability_gaps).expect("encode gaps"),
        ),
        (
            vec!["https://example.test/one"],
            vec!["jina"],
            json!([{
                "capability": "web_fetch",
                "reason": "partial_failure",
                "providers_skipped": []
            }]),
        )
    );
}

#[test]
fn a_declared_web_fetch_reads_known_urls_concurrently_and_keeps_their_order() {
    // Neither request is answered until both arrive, so a serial fetch would fail.
    let jina = Fixture::start_synchronized_sequence(vec![jina(&evidence()), jina(&evidence())]);
    let document = web_fetch_document(&[("jina", &jina.url)], &["jina-key"]);

    let branch = declared_branch(
        "web_fetch",
        "Verify https://example.test/one https://example.test/two",
        &document,
    );

    assert_eq!(
        (
            candidate_urls(&branch),
            serde_json::to_value(&branch.capability_gaps).expect("encode gaps"),
            jina.finish_all().len(),
        ),
        (
            vec!["https://example.test/one", "https://example.test/two"],
            json!([]),
            2,
        )
    );
}

#[test]
fn concurrent_rate_limited_targets_each_rotate_once() {
    let targets = ["one", "two", "three", "four"];
    let rate_limited = stored_with("jina/error.json", "/message", "rate limited");
    let jina = Fixture::start_routed(
        targets
            .iter()
            .flat_map(|target| {
                let path = format!("/https://example.test/{target}");
                [
                    Route::new("GET", &path, Response::json(429, &rate_limited)),
                    Route::new("GET", &path, jina(&evidence())),
                ]
            })
            .collect(),
    );
    let document = web_fetch_document(&[("jina", &jina.url)], &["jina-key-a", "jina-key-b"]);
    let query = format!(
        "Verify {}",
        targets
            .map(|target| format!("https://example.test/{target}"))
            .join(" ")
    );

    let branch = declared_branch("web_fetch", &query, &document);
    jina.finish_all();
    let attempts = branch
        .attempts
        .iter()
        .filter(|attempt| attempt.target.seam_name() == Some("web_fetch"))
        .collect::<Vec<_>>();

    assert_eq!(
        (
            branch.sources.len(),
            attempts.len(),
            attempts
                .iter()
                .filter(|attempt| attempt.error_kind == Some(AttemptErrorKind::RateLimited))
                .count(),
            attempts.iter().map(|attempt| attempt.rotation_count).max(),
        ),
        (4, 8, 4, Some(1))
    );
}
