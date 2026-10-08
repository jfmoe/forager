mod support;

use std::collections::BTreeMap;
use std::process::Output;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Value, json};

use support::scholar::{config, fixture, ok, payload, query_pairs, refs, stderr};
use support::{Fixture, Response, RunEnvironment};

const KEY: &str = "test-serpapi-key-a";
const CITED: &str = "scholar:18208131694456651388";

fn cited_by(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "scholar", "cited-by"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

/// Runs one cited-by command against a fixture that answers in order and returns every request.
fn run(arguments: &[&str], responses: Vec<Response>) -> (Output, Vec<String>) {
    let fixture = Fixture::start_sequence(responses);
    let environment = RunEnvironment::new(&config(&fixture.url, &[KEY]));
    let output = cited_by(&environment, arguments);
    (output, fixture.finish_all())
}

#[test]
fn cited_by_sends_the_cited_cluster_and_page_size_with_the_key() {
    let (output, requests) = run(&[CITED], vec![ok(&fixture("cited_by.json"))]);

    assert_eq!(
        (output.status.code(), query_pairs(&requests[0])),
        (
            Some(0),
            BTreeMap::from([
                ("engine".to_owned(), "google_scholar".to_owned()),
                ("hl".to_owned(), "en".to_owned()),
                ("cites".to_owned(), "18208131694456651388".to_owned()),
                ("num".to_owned(), "20".to_owned()),
                ("api_key".to_owned(), KEY.to_owned()),
            ])
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn the_query_years_and_date_order_join_the_query_parameters() {
    let cases = [
        (
            &[
                CITED,
                "--query",
                "crash",
                "--year-from",
                "2020",
                "--year-to",
                "2024",
            ][..],
            [Some("crash"), Some("2020"), Some("2024"), None],
        ),
        (&[CITED, "--sort", "date"], [None, None, None, Some("2")]),
    ];
    for (arguments, expected) in cases {
        let (output, requests) = run(arguments, vec![ok(&fixture("cited_by.json"))]);
        let query = query_pairs(&requests[0]);

        assert_eq!(
            (
                output.status.code(),
                ["q", "as_ylo", "as_yhi", "scisbd"].map(|name| query.get(name).cloned())
            ),
            (Some(0), expected.map(|value| value.map(str::to_owned))),
            "arguments: {arguments:?}\nstderr: {}",
            stderr(&output)
        );
    }
}

#[test]
fn cited_by_decodes_citing_works_like_search_results() {
    let source = fixture("cited_by.json");
    let (output, _) = run(&[CITED, "--query", "crash"], vec![ok(&source)]);
    let mut page = payload(&output);
    let first = page["items"][0].clone();
    page.as_object_mut().expect("page").remove("items");

    assert_eq!(
        (
            output.status.code(),
            refs(&payload(&output)),
            first,
            page["platform"].clone(),
            page["provider"].clone(),
            page["next_cursor"].is_string(),
        ),
        (
            Some(0),
            vec![
                "scholar:2061579707271568590".to_owned(),
                "scholar:13440399377941409520".to_owned(),
                "scholar:239332318604014067".to_owned(),
            ],
            json!({
                "ref": "scholar:2061579707271568590",
                "url": "https://scholar.google.com/scholar?cluster=2061579707271568590",
                "depth": "snippet",
                "title": "Momentum crashes",
                "authors": ["K Daniel", "TJ Moskowitz"],
                "published": "2016",
                "snippet": source["organic_results"][0]["snippet"],
                "link": "https://www.sciencedirect.com/science/article/pii/S0304405X16301490",
                "source": "K Daniel, TJ Moskowitz - Journal of Financial economics, 2016 - Elsevier",
                "cited_by": 1851,
                "version_count": 28,
                "resources": [{
                    "title": "sciencedirect.com",
                    "file_format": "HTML",
                    "url": "https://www.sciencedirect.com/science/article/pii/S0304405X16301490"
                }],
                "result_type": "Html"
            }),
            json!("scholar"),
            json!("serpapi"),
            true,
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_paper_without_citing_works_lists_nothing_and_succeeds() {
    let (output, requests) = run(&["scholar:1"], vec![ok(&fixture("cited_by_empty.json"))]);
    let page = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &page["items"],
            &page["next_cursor"],
            requests.len()
        ),
        (Some(0), &json!([]), &Value::Null, 1),
        "stderr: {}",
        stderr(&output)
    );
}

/// Runs a cited-by command that must fail before any request and returns its exit code, stderr,
/// and request count.
fn preflight(configuration: &str, arguments: &[&str]) -> (Option<i32>, String, usize) {
    let fixture = Fixture::start_canary();
    let environment = RunEnvironment::new(&configuration.replace("{url}", &fixture.url));
    let output = cited_by(&environment, arguments);
    (
        output.status.code(),
        stderr(&output),
        fixture.finish_all().len(),
    )
}

#[test]
fn invalid_targets_and_options_are_rejected_before_any_request() {
    for (arguments, message) in [
        (
            &["https://scholar.google.com/scholar?cites=18208131694456651388"][..],
            "unrecognized scholar reference",
        ),
        (&[], "required"),
        (&[CITED, "--limit", "0"], "--limit"),
        (&[CITED, "--limit", "21"], "--limit"),
        (
            &[CITED, "--year-from", "2025", "--year-to", "2024"],
            "--year-from must not be later than --year-to",
        ),
        (
            &[CITED, "--sort", "date", "--year-from", "2020"],
            "--sort date cannot be combined with --year-from or --year-to",
        ),
        (
            &[CITED, "--sort", "date", "--year-to", "2024"],
            "--sort date cannot be combined with --year-from or --year-to",
        ),
        (&[CITED, "--query", " "], "--query needs a word"),
    ] {
        let (code, stderr, requests) = preflight(&config("{url}", &[KEY]), arguments);

        assert_eq!(
            (code, stderr.contains(message), requests),
            (Some(2), true, 0),
            "arguments: {arguments:?}\nstderr: {stderr}"
        );
    }
}

/// A cursor as the route issues it: `fields` joined to the cited paper and page size.
fn cursor(limit: u64, page: &str, fields: Value) -> String {
    let Value::Object(fields) = fields else {
        panic!("cursor fields must be an object")
    };
    let mut request =
        json!({"cited": 18_208_131_694_456_651_388_u64, "limit": limit, "page": page});
    request.as_object_mut().expect("request").extend(fields);
    encoded(&request)
}

fn encoded(request: &Value) -> String {
    format!("v1.serpapi.{}", URL_SAFE_NO_PAD.encode(request.to_string()))
}

#[test]
fn a_cursor_resumes_the_next_page_with_every_original_condition() {
    let fixture = Fixture::start_sequence(vec![
        ok(&fixture("cited_by.json")),
        ok(&fixture("cited_by.json")),
    ]);
    let environment = RunEnvironment::new(&config(&fixture.url, &[KEY]));
    let first = cited_by(
        &environment,
        &[
            CITED,
            "--query",
            "crash",
            "--limit",
            "10",
            "--year-from",
            "2020",
            "--year-to",
            "2024",
        ],
    );
    let cursor = payload(&first)["next_cursor"]
        .as_str()
        .expect("first page cursor")
        .to_owned();
    let second = cited_by(&environment, &["--cursor", &cursor]);
    let requests = fixture.finish_all();
    let mut resumed = query_pairs(&requests[1]);
    let start = resumed.remove("start");

    assert_eq!(
        (
            second.status.code(),
            cursor.starts_with("v1.serpapi."),
            start.as_deref(),
            resumed,
        ),
        (Some(0), true, Some("10"), query_pairs(&requests[0])),
        "stderr: {}",
        stderr(&second)
    );
}

#[test]
fn a_date_ordered_cursor_keeps_the_order() {
    let (output, requests) = run(
        &["--cursor", &cursor(20, "20", json!({"sort": "date"}))],
        vec![ok(&fixture("cited_by.json"))],
    );
    let query = query_pairs(&requests[0]);

    assert_eq!(
        (
            output.status.code(),
            query.get("scisbd").map(String::as_str),
            query.get("start").map(String::as_str)
        ),
        (Some(0), Some("2"), Some("20")),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_cursor_conflicts_with_a_target_query_option_or_limit() {
    let cursor = cursor(20, "20", json!({}));
    for extra in [
        &[CITED][..],
        &["--query", "crash"],
        &["--limit", "20"],
        &["--year-from", "2020"],
        &["--year-to", "2024"],
        &["--sort", "date"],
    ] {
        let mut arguments = vec!["--cursor", cursor.as_str()];
        arguments.extend_from_slice(extra);

        let (code, message, requests) = preflight(&config("{url}", &[KEY]), &arguments);

        assert_eq!(
            (code, message.contains("cannot be used with"), requests),
            (Some(2), true, 0),
            "arguments: {arguments:?}\nstderr: {message}"
        );
    }
}

#[test]
fn a_tampered_cursor_is_rejected_before_any_request() {
    for (cursor, expected) in [
        (
            cursor(21, "20", json!({})),
            "--limit must be between 1 and 20",
        ),
        (
            cursor(20, "20", json!({"sort": "date", "year_from": 2020})),
            "--sort date cannot be combined",
        ),
        (
            cursor(20, "1000", json!({})),
            "serpapi cannot page past Google Scholar result 1000",
        ),
        (
            cursor(20, "20", json!({"review_only": true})),
            "the cursor cannot be decoded",
        ),
    ] {
        let (code, message, requests) = preflight(&config("{url}", &[KEY]), &["--cursor", &cursor]);

        assert_eq!(
            (code, message.contains(expected), requests),
            (Some(2), true, 0),
            "expected: {expected}\nstderr: {message}"
        );
    }
}

#[test]
fn search_and_cited_by_cursors_do_not_cross() {
    let search_cursor = encoded(&json!({
        "query": "momentum",
        "limit": 20,
        "options": {"platform": "scholar"},
        "page": "20"
    }));
    let cited_by_cursor = cursor(20, "20", json!({}));
    let fixture = Fixture::start_canary();
    let environment = RunEnvironment::new(&config(&fixture.url, &[KEY]));

    let in_cited_by = cited_by(&environment, &["--cursor", &search_cursor]);
    let in_search = environment.run(&[
        "platform",
        "scholar",
        "search",
        "--cursor",
        &cited_by_cursor,
    ]);
    let in_arxiv = environment.run(&["platform", "arxiv", "search", "--cursor", &cited_by_cursor]);

    assert_eq!(
        [&in_cited_by, &in_search, &in_arxiv].map(|output| (output.status.code(), stderr(output))),
        [
            (
                Some(2),
                "argument_error: the cursor belongs to scholar search, not scholar cited_by\n"
                    .to_owned()
            ),
            (
                Some(2),
                "argument_error: the cursor belongs to scholar cited_by, not scholar search\n"
                    .to_owned()
            ),
            (
                Some(2),
                "argument_error: the cursor belongs to scholar cited_by, not arxiv search\n"
                    .to_owned()
            ),
        ]
    );
    assert_eq!(fixture.finish_all().len(), 0);
}

#[test]
fn a_missing_key_or_an_empty_order_fails_before_any_request_like_search() {
    for (configuration, environment, names_keys) in [
        (config("{url}", &[]), None, true),
        (
            config("{url}", &[KEY]),
            Some(("FORAGER_PLATFORMS__SCHOLAR__ORDER", "[]")),
            false,
        ),
    ] {
        let fixture = Fixture::start_canary();
        let run_environment = RunEnvironment::new(&configuration.replace("{url}", &fixture.url));
        let arguments = ["platform", "scholar", "cited-by", CITED];

        let output = match environment {
            Some(variable) => run_environment.run_with_env(&arguments, &[variable]),
            None => run_environment.run(&arguments),
        };
        let message = stderr(&output);

        assert_eq!(
            (
                output.status.code(),
                message.contains(
                    "platforms.scholar.order has no configured route for scholar cited_by"
                ),
                message.contains("providers.serpapi.keys"),
                fixture.finish_all().len()
            ),
            (Some(3), true, names_keys, 0),
            "stderr: {message}"
        );
    }
}

fn status_error(message: &str) -> Value {
    json!({
        "search_metadata": {"id": "6ac6613da20802ffe5e7c8be", "status": "Error"},
        "error": message
    })
}

#[test]
fn a_status_error_is_retried_under_the_cited_by_target() {
    let (output, requests) = run(
        &[CITED, "--verbose"],
        vec![
            ok(&status_error(
                "Google hasn't returned any results for this query.",
            )),
            ok(&fixture("cited_by.json")),
        ],
    );
    let page = payload(&output);
    let attempts = page["provider_attempts"]
        .as_array()
        .expect("attempts")
        .iter()
        .map(|attempt| {
            (
                attempt["error_kind"].clone(),
                attempt["platform"].clone(),
                attempt["operation"].clone(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        (output.status.code(), requests.len(), attempts),
        (
            Some(0),
            2,
            vec![
                (json!("network"), json!("scholar"), json!("cited_by")),
                (Value::Null, json!("scholar"), json!("cited_by")),
            ]
        ),
        "stderr: {}",
        stderr(&output)
    );
}
