use std::fmt::Write as _;
use std::time::Duration;

use serde_json::{Value, json};

use super::{
    CapabilityBranch, CapabilityResults, CapabilityTargets, SearchFailure, execute_capabilities,
    search,
};
use crate::config::runtime_config_from_toml;
use crate::engine::CapabilityExecution;
use crate::fixture_server::{Fixture, Response, Route, block_on, request_json};
use crate::main_search_samples::completed;
use crate::net::{RetryPolicy, build_client};
use crate::providers::MainSearchRequest;
use crate::samples::{sample, sample_json};
use crate::types::{
    AttemptErrorKind, CapabilitySet, Deadline, FallbackPolicy, SearchCandidate, SearchOutcome,
    Source,
};
use crate::web_samples::{jina, stored_with, tavily_results};

/// Runs a declared documentation search over the configuration `document` and returns its
/// branch.
fn docs_branch(document: &str) -> CapabilityBranch {
    declared_branch("docs_search", "Rust ownership", document)
}

/// Runs the one declared `capability` for `query` over the configuration `document` and
/// returns its branch.
fn declared_branch(capability: &str, query: &str, document: &str) -> CapabilityBranch {
    let mut results = declared_results(capability, query, FallbackPolicy::Auto, document);
    assert_eq!(results.branches.len(), 1, "one declared capability");
    results.branches.remove(0)
}

/// One attempt per request within a long deadline, under `fallback`.
fn execution(fallback: FallbackPolicy) -> CapabilityExecution {
    CapabilityExecution::new(
        fallback,
        build_client(true).expect("build client"),
        RetryPolicy::new(1, 1.0, Duration::ZERO),
        Deadline::new(Duration::from_secs(30)),
    )
}

/// Runs the capabilities of `declaration` for `query` over the configuration `document`, with
/// no requested target.
fn declared_results(
    declaration: &str,
    query: &str,
    fallback: FallbackPolicy,
    document: &str,
) -> CapabilityResults {
    let config = runtime_config_from_toml(document).expect("valid configuration");
    let capabilities: CapabilitySet = declaration.parse().expect("capability set");
    block_on(execute_capabilities(
        query,
        &capabilities,
        0,
        &config,
        execution(fallback),
    ))
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
    let mut body = sample_json("exa/search.json");
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
        &stored_with("tavily/extract.json", &[("/results/0/raw_content", "thin")]),
    );
    let firecrawl = Fixture::start_json(
        200,
        &stored_with("firecrawl/scrape.json", &[("/data/markdown", "also thin")]),
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
                &stored_with("jina/error.json", &[("/message", "unavailable")]),
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
    let rate_limited = stored_with("jina/error.json", &[("/message", "rate limited")]);
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

/// A configuration document that gives each named provider its fixture URL and one key.
fn providers_document(providers: &[(&str, &str)]) -> String {
    let mut document = String::new();
    for (provider, url) in providers {
        let _ = write!(
            document,
            "[providers.{provider}]\nurl = {url:?}\nkeys = [\"{provider}-key\"]\ntimeout = 30\n\n"
        );
    }
    document
}

/// xAI as the only main-search backend, at the fixture `url`.
fn xai_document(url: &str) -> String {
    format!(
        "[providers.xai]\nurl = \"{url}/v1\"\nkeys = [\"xai-key\"]\nmodel = \"test-model\"\ntools = [\"web_search\"]\n\n[search]\nbackends = [\"xai\"]\n\n"
    )
}

/// The stored xAI answer as main search receives it.
fn main_search_answer() -> Response {
    completed(&sample_json("xai/x-ask.json"))
}

fn tavily_search_order() -> &'static str {
    "[capabilities.web_search]\norder = [\"tavily\"]\n"
}

/// Runs a Default Search Invocation of `query` with the capabilities of `declaration`, none
/// when it is `None`, over the configuration `document`.
fn search_invocation(
    query: &str,
    declaration: Option<&str>,
    document: &str,
) -> Result<SearchOutcome, Box<SearchFailure>> {
    let config = runtime_config_from_toml(document).expect("valid configuration");
    let capabilities = declaration.map_or_else(CapabilitySet::default, |declaration| {
        declaration.parse().expect("capability set")
    });
    block_on(search(
        MainSearchRequest {
            query: query.into(),
            model: None,
        },
        &capabilities,
        0,
        &config,
        execution(FallbackPolicy::Auto),
    ))
    .map_err(Box::new)
}

fn attempt_seams(attempts: &[crate::types::ProviderAttempt]) -> Vec<Option<&'static str>> {
    attempts
        .iter()
        .map(|attempt| attempt.target.seam_name())
        .collect()
}

fn gaps(branch: &CapabilityBranch) -> Value {
    serde_json::to_value(&branch.capability_gaps).expect("encode gaps")
}

#[test]
fn a_search_without_declared_capabilities_runs_main_search_alone() {
    let main = Fixture::start_sequence(vec![main_search_answer()]);
    let supplemental = Fixture::start_canary();
    let document = format!(
        "{}{}{}[capabilities.docs_search]\norder = [\"exa\"]\n",
        xai_document(&main.url),
        providers_document(&[("tavily", &supplemental.url), ("exa", &supplemental.url)]),
        tavily_search_order(),
    );

    let outcome =
        search_invocation("Ordinary bare search", None, &document).expect("main search answers");
    main.finish();

    assert_eq!(
        (
            attempt_seams(&outcome.attempts),
            outcome.extra_sources.len(),
            outcome.capabilities.len(),
            supplemental.finish_all().len(),
        ),
        (vec![Some("main_search")], 0, 0, 0)
    );
}

#[test]
fn declared_capabilities_run_while_main_search_is_in_flight() {
    // Neither request is answered until both arrive, so a search that waited for main search
    // before its capabilities would fail.
    let mut fixtures = Fixture::start_synchronized_sequences(vec![
        vec![main_search_answer()],
        vec![Response::json(200, &sample("tavily/search.json"))],
    ]);
    let tavily = fixtures.pop().expect("tavily fixture");
    let main = fixtures.pop().expect("main fixture");
    let document = format!(
        "{}{}{}",
        xai_document(&main.url),
        providers_document(&[("tavily", &tavily.url)]),
        tavily_search_order(),
    );

    let outcome = search_invocation("Current information", Some("web_search"), &document)
        .expect("main search answers");
    main.finish();
    tavily.finish();

    assert_eq!(
        (
            attempt_seams(&outcome.attempts),
            candidate_urls_of(&outcome.extra_sources),
        ),
        (
            vec![Some("main_search"), Some("web_search")],
            vec!["https://example.test/extra"],
        )
    );
}

fn candidate_urls_of(candidates: &[SearchCandidate]) -> Vec<&str> {
    candidates.iter().map(SearchCandidate::url).collect()
}

#[test]
fn a_failed_main_search_keeps_the_capability_candidates_and_gaps() {
    // No main-search backend has a key, so main search fails without a request.
    let tavily = Fixture::start_json(200, &sample("tavily/search.json"));
    let document = format!(
        "{}{}",
        providers_document(&[("tavily", &tavily.url)]),
        tavily_search_order(),
    );

    let failure = search_invocation(
        "Current information",
        Some("docs_search,web_search"),
        &document,
    )
    .expect_err("main search fails");
    tavily.finish();

    assert_eq!(
        (
            serde_json::to_value(&failure.extra_sources).expect("encode candidates"),
            serde_json::to_value(&failure.capability_gaps).expect("encode gaps"),
            attempt_seams(&failure.error.attempts).contains(&Some("web_search")),
        ),
        (
            json!([{
                "provider": "tavily",
                "capability": "web_search",
                "title": "Supplemental",
                "url": "https://example.test/extra",
                "summary": "Provider-native summary of the page.",
                "provider_data": {}
            }]),
            json!([{
                "capability": "docs_search",
                "reason": "no_configured_provider",
                "providers_skipped": ["exa"]
            }]),
            true,
        )
    );
}

#[test]
fn a_declared_web_search_without_a_target_asks_for_three_sources() {
    let tavily = Fixture::start_json(200, &sample("tavily/search.json"));
    let document = format!(
        "{}{}",
        providers_document(&[("tavily", &tavily.url)]),
        tavily_search_order(),
    );

    let branch = declared_branch("web_search", "Ordinary search", &document);
    let request = tavily.finish();

    assert_eq!(
        (
            candidate_urls(&branch),
            request_json(&request)["max_results"].clone()
        ),
        (vec!["https://example.test/extra"], json!(3))
    );
}

#[test]
fn web_search_candidates_join_the_extra_sources_unless_main_search_cited_them() {
    let supplemental = sample_result();
    let mut duplicate = supplemental.clone();
    duplicate["url"] = json!("https://example.test/source?token=canary-secret");
    let tavily = Fixture::start_sequence(vec![tavily_results(&json!([duplicate, supplemental]))]);
    let document = format!(
        "{}{}",
        providers_document(&[("tavily", &tavily.url)]),
        tavily_search_order(),
    );

    let branch = declared_branch("web_search", "Current information", &document);
    tavily.finish();
    let mut outcome = SearchOutcome {
        provider: "xai",
        answer: "answer".into(),
        sources: vec![Source {
            title: "Primary".into(),
            url: "https://example.test/source?token=********".into(),
            published_date: None,
            author: None,
            text: None,
            highlights: Vec::new(),
            id: None,
            image: None,
            favicon: None,
        }],
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

    assert_eq!(
        (
            serde_json::to_value(&outcome.extra_sources).expect("encode candidates"),
            serde_json::to_value(&outcome.capability_gaps).expect("encode gaps"),
        ),
        (
            json!([{
                "provider": "tavily",
                "capability": "web_search",
                "title": "Supplemental",
                "url": "https://example.test/extra",
                "summary": "Provider-native summary of the page.",
                "provider_data": {}
            }]),
            json!([]),
        )
    );
}

#[test]
fn an_untitled_web_search_result_becomes_a_candidate_with_a_null_title() {
    let mut body = sample_json("firecrawl/search.json");
    body["data"]["web"][0]
        .as_object_mut()
        .expect("stored result")
        .remove("title");
    body["data"]["web"][0]["url"] = json!("https://example.test/untitled");
    let firecrawl = Fixture::start_sequence(vec![Response::json(200, &body.to_string())]);
    let document = format!(
        "{}[capabilities.web_search]\norder = [\"firecrawl\"]\n",
        providers_document(&[("firecrawl", &firecrawl.url)]),
    );

    let branch = declared_branch("web_search", "Current information", &document);
    firecrawl.finish();

    assert_eq!(
        serde_json::to_value(&branch.sources).expect("encode candidates"),
        json!([{
            "provider": "firecrawl",
            "capability": "web_search",
            "title": null,
            "url": "https://example.test/untitled",
            "summary": "Firecrawl summary",
            "provider_data": {}
        }])
    );
}

fn sample_result() -> Value {
    let body = sample_json("tavily/search.json");
    body["results"][0].clone()
}

#[test]
fn a_declared_web_search_without_credentials_is_a_gap_naming_every_provider() {
    let branch = declared_branch("web_search", "Current information", "");

    assert_eq!(
        (gaps(&branch), branch.attempts.len(), branch.diagnostics),
        (
            json!([{
                "capability": "web_search",
                "reason": "no_configured_provider",
                "providers_skipped": ["tavily", "firecrawl"]
            }]),
            0,
            vec!["capability gap: web_search has no configured provider".to_owned()],
        )
    );
}

#[test]
fn a_combined_declaration_runs_only_its_seams_in_vocabulary_order_and_concurrently() {
    // Neither request is answered until both arrive, so branches run one after the other would
    // fail; the Web Fetch provider is never asked, although the query names a known URL.
    let mut docs = sample_json("exa/search.json");
    docs["results"][0]["url"] = json!("https://example.test/docs");
    let mut fixtures = Fixture::start_synchronized_sequences(vec![
        vec![Response::json(200, &docs.to_string())],
        vec![Response::json(200, &sample("tavily/search.json"))],
    ]);
    let tavily = fixtures.pop().expect("tavily fixture");
    let exa = fixtures.pop().expect("exa fixture");
    let jina = Fixture::start_canary();
    let document = format!(
        "{}[capabilities.docs_search]\norder = [\"exa\"]\n{}[capabilities.web_fetch]\norder = [\"jina\"]\n",
        providers_document(&[
            ("exa", &exa.url),
            ("tavily", &tavily.url),
            ("jina", &jina.url)
        ]),
        tavily_search_order(),
    );

    let results = declared_results(
        "web_search,docs_search",
        "Read https://example.test/known",
        FallbackPolicy::Auto,
        &document,
    );
    exa.finish();
    tavily.finish();
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
    results.merge_into(&mut outcome);

    assert_eq!(
        (
            attempt_seams(&outcome.attempts),
            candidate_urls_of(&outcome.extra_sources),
            jina.finish_all().len(),
        ),
        (
            vec![Some("docs_search"), Some("web_search")],
            vec!["https://example.test/docs", "https://example.test/extra"],
            0,
        )
    );
}

#[test]
fn a_failed_declared_web_search_is_a_gap_that_keeps_its_attempts() {
    let mut unavailable = sample_json("tavily/error.json");
    unavailable["message"] = json!("unavailable");
    let tavily = Fixture::start_json(503, &unavailable.to_string());
    let document = format!(
        "{}{}",
        providers_document(&[("tavily", &tavily.url)]),
        tavily_search_order(),
    );

    let branch = declared_branch("web_search", "Current information", &document);
    tavily.finish();
    let attempts = serde_json::to_value(&branch.attempts).expect("encode attempts");

    assert_eq!(
        (
            gaps(&branch),
            branch_summary(&branch).2,
            attempts[0].get("model"),
            attempts[0].get("endpoint_host"),
        ),
        (
            json!([{
                "capability": "web_search",
                "reason": "all_attempts_failed",
                "providers_skipped": []
            }]),
            vec!["tavily"],
            None,
            None,
        )
    );
}

#[test]
fn answers_left_without_a_valid_result_fail_the_web_search_unlike_empty_ones() {
    let tavily = Fixture::start_sequence(vec![tavily_results(&json!([
        {"url": null},
        {"title": "missing"},
        {"url": "  "}
    ]))]);
    let firecrawl = Fixture::start_json(200, r#"{"data":{}}"#);
    let document = format!(
        "{}[capabilities.web_search]\norder = [\"tavily\", \"firecrawl\"]\n",
        providers_document(&[("tavily", &tavily.url), ("firecrawl", &firecrawl.url)]),
    );

    let branch = declared_branch("web_search", "Current information", &document);
    tavily.finish();
    firecrawl.finish();

    assert_eq!(
        (
            candidate_urls(&branch),
            gaps(&branch),
            branch
                .attempts
                .iter()
                .map(|attempt| attempt.error_kind)
                .collect::<Vec<_>>(),
        ),
        (
            Vec::<&str>::new(),
            json!([{
                "capability": "web_search",
                "reason": "all_attempts_failed",
                "providers_skipped": []
            }]),
            vec![
                Some(AttemptErrorKind::Evidence),
                Some(AttemptErrorKind::Runtime)
            ],
        )
    );
}

#[test]
fn a_declared_web_search_with_fallback_off_stops_at_the_chain_head() {
    // The head has no key; the configured provider after it is never asked.
    let tavily = Fixture::start_canary();
    let document = format!(
        "{}[capabilities.web_search]\norder = [\"firecrawl\", \"tavily\"]\n",
        providers_document(&[("tavily", &tavily.url)]),
    );

    let mut results = declared_results(
        "web_search",
        "Current information",
        FallbackPolicy::Off,
        &document,
    );
    let branch = results.branches.remove(0);

    assert_eq!(
        (
            branch_summary(&branch).2,
            gaps(&branch),
            tavily.finish_all().len(),
        ),
        (
            vec!["firecrawl"],
            json!([{
                "capability": "web_search",
                "reason": "all_attempts_failed",
                "providers_skipped": ["tavily"]
            }]),
            0,
        )
    );
}
