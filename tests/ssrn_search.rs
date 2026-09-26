mod support;

use std::collections::BTreeMap;
use std::process::Output;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Value, json};

use support::{Fixture, Response, RunEnvironment};

const SELECT: &str = "DOI,title,author,abstract,published,type,created,resource";

fn work(id: &str) -> Value {
    json!({
        "DOI": format!("10.2139/ssrn.{id}"),
        "title": [format!("Paper {id}")],
        "author": [{"given": "Gary", "family": "Antonacci", "sequence": "first"}],
        "abstract": format!("<jats:p>Abstract of {id}.</jats:p>"),
        "published": {"date-parts": [[2012]]},
        "type": "posted-content",
        "created": {"date-parts": [[2012, 4, 25]], "date-time": "2012-04-25T11:01:44Z"},
        "resource": {"primary": {"URL": format!("https://www.ssrn.com/abstract={id}")}}
    })
}

fn work_list(total: u64, items: &[Value]) -> Response {
    Response::json(
        200,
        &json!({
            "status": "ok",
            "message-type": "work-list",
            "message-version": "1.0.0",
            "message": {"total-results": total, "items": items, "items-per-page": items.len()}
        })
        .to_string(),
    )
}

fn works(ids: &[&str]) -> Vec<Value> {
    ids.iter().map(|id| work(id)).collect()
}

fn config(url: &str) -> String {
    format!("[providers.ssrn_crossref]\nurl = \"{url}\"\n")
}

fn search(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "ssrn", "search"];
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

fn request_target(request: &str) -> reqwest::Url {
    let target = request.split_whitespace().nth(1).expect("request target");
    reqwest::Url::parse(&format!("http://fixture.test{target}")).expect("request URL")
}

fn query_pairs(request: &str) -> BTreeMap<String, String> {
    request_target(request)
        .query_pairs()
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}

/// Runs one page in its own environment and returns the output and the received request.
fn page(arguments: &[&str], response: Response) -> (Output, String) {
    let fixture = Fixture::start_sequence(vec![response]);
    let environment = RunEnvironment::new(&config(&fixture.url));
    let output = search(&environment, arguments);
    (output, fixture.finish())
}

fn cursor(query: &str, limit: u16, page: &str) -> String {
    let payload = json!({
        "query": query,
        "limit": limit,
        "options": {"platform": "ssrn"},
        "page": page
    });
    format!(
        "v1.ssrn_crossref.{}",
        URL_SAFE_NO_PAD.encode(payload.to_string())
    )
}

#[test]
fn search_sends_the_prefix_query_with_fixed_ranking_and_fields() {
    let (output, request) = page(&["dual momentum"], work_list(0, &[]));

    assert_eq!(
        (
            output.status.code(),
            request_target(&request).path().to_owned(),
            query_pairs(&request),
        ),
        (
            Some(0),
            "/prefixes/10.2139/works".to_owned(),
            BTreeMap::from([
                ("query".to_owned(), "dual momentum".to_owned()),
                ("rows".to_owned(), "10".to_owned()),
                ("offset".to_owned(), "0".to_owned()),
                ("sort".to_owned(), "score".to_owned()),
                ("order".to_owned(), "desc".to_owned()),
                ("select".to_owned(), SELECT.to_owned()),
            ])
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn search_returns_abstract_and_metadata_depth_items_and_skips_non_ssrn_dois() {
    let mut without_abstract = work("2");
    without_abstract["abstract"] = Value::Null;
    without_abstract["type"] = json!("journal-article");
    let items = [
        work("1"),
        json!({"DOI": "10.1016/j.jfineco.2020.01.001", "title": ["Elsewhere"]}),
        without_abstract,
    ];
    let (output, _) = page(&["momentum"], work_list(3, &items));

    assert_eq!(
        (
            output.status.code(),
            payload(&output),
            String::from_utf8_lossy(&output.stderr).contains(
                "ssrn_crossref skipped 1 Crossref records without an SSRN DOI: 10.1016/j.jfineco.2020.01.001"
            ),
        ),
        (
            Some(0),
            json!({
                "platform": "ssrn",
                "provider": "ssrn_crossref",
                "items": [
                    {
                        "ref": "ssrn:1",
                        "url": "https://papers.ssrn.com/sol3/papers.cfm?abstract_id=1",
                        "depth": "abstract",
                        "title": "Paper 1",
                        "authors": ["Gary Antonacci"],
                        "published": "2012",
                        "abstract": "Abstract of 1.",
                        "snippet": null,
                        "doi": "10.2139/ssrn.1",
                        "crossref_type": "posted-content",
                        "crossref_created": "2012-04-25T11:01:44Z",
                        "posted": null,
                        "last_revised": null,
                        "date_written": null
                    },
                    {
                        "ref": "ssrn:2",
                        "url": "https://papers.ssrn.com/sol3/papers.cfm?abstract_id=2",
                        "depth": "metadata",
                        "title": "Paper 2",
                        "authors": ["Gary Antonacci"],
                        "published": "2012",
                        "abstract": null,
                        "snippet": null,
                        "doi": "10.2139/ssrn.2",
                        "crossref_type": "journal-article",
                        "crossref_created": "2012-04-25T11:01:44Z",
                        "posted": null,
                        "last_revised": null,
                        "date_written": null
                    }
                ],
                "next_cursor": null
            }),
            true,
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn cursors_page_by_absolute_offset_and_restore_the_original_request() {
    let (first, first_request) = page(
        &["momentum", "--limit", "2"],
        work_list(5, &works(&["1", "2"])),
    );
    let first_cursor = payload(&first)["next_cursor"]
        .as_str()
        .expect("first page cursor")
        .to_owned();
    let (middle, middle_request) = page(
        &["--cursor", &first_cursor],
        work_list(5, &works(&["3", "4"])),
    );
    let middle_cursor = payload(&middle)["next_cursor"]
        .as_str()
        .expect("middle page cursor")
        .to_owned();
    let (last, last_request) = page(&["--cursor", &middle_cursor], work_list(5, &works(&["5"])));
    let without_offset = |request: &str| {
        let mut query = query_pairs(request);
        query.remove("offset");
        query
    };

    assert_eq!(
        (
            first_cursor.starts_with("v1.ssrn_crossref."),
            [&first_request, &middle_request, &last_request]
                .map(|request| query_pairs(request)["offset"].clone()),
            without_offset(&middle_request) == without_offset(&first_request),
            without_offset(&last_request) == without_offset(&first_request),
            payload(&last)["next_cursor"].clone(),
        ),
        (
            true,
            ["0".to_owned(), "2".to_owned(), "4".to_owned()],
            true,
            true,
            Value::Null,
        )
    );
}

#[test]
fn a_full_page_that_reaches_the_total_has_no_next_cursor() {
    let (output, _) = page(
        &["momentum", "--limit", "2"],
        work_list(2, &works(&["1", "2"])),
    );

    assert_eq!(payload(&output)["next_cursor"], Value::Null);
}

#[test]
fn the_last_page_is_judged_before_non_ssrn_records_are_dropped() {
    let items = [work("1"), json!({"DOI": "10.1016/j.jfineco.2020.01.001"})];
    let (output, _) = page(&["momentum", "--limit", "2"], work_list(10, &items));
    let page = payload(&output);

    assert_eq!(
        (
            page["items"].as_array().map(Vec::len),
            page["next_cursor"].is_string()
        ),
        (Some(1), true)
    );
}

#[test]
fn no_cursor_is_issued_for_a_page_past_the_crossref_offset_limit() {
    let ids = (0..20)
        .map(|index| (9980 + index).to_string())
        .collect::<Vec<_>>();
    let ids = ids.iter().map(String::as_str).collect::<Vec<_>>();
    let (before_limit, _) = page(
        &["--cursor", &cursor("momentum", 20, "9960")],
        work_list(50_000, &works(&ids)),
    );
    let (at_limit, request) = page(
        &["--cursor", &cursor("momentum", 20, "9980")],
        work_list(50_000, &works(&ids)),
    );

    assert_eq!(
        (
            payload(&before_limit)["next_cursor"].is_string(),
            query_pairs(&request)["offset"].as_str(),
            &payload(&at_limit)["next_cursor"],
        ),
        (true, "9980", &Value::Null)
    );
}

#[test]
fn a_legitimate_empty_result_is_an_empty_success() {
    let (output, _) = page(&["no such words"], work_list(0, &[]));

    assert_eq!(
        (
            output.status.code(),
            &payload(&output)["items"],
            &payload(&output)["next_cursor"]
        ),
        (Some(0), &json!([]), &Value::Null)
    );
}

#[test]
fn a_response_of_another_message_type_is_a_runtime_failure() {
    let body = json!({"status": "ok", "message-type": "work", "message": work("1")});
    let (output, _) = page(&["momentum"], Response::json(200, &body.to_string()));
    let payload = payload(&output);

    assert_eq!(
        (output.status.code(), &payload["error_kind"]),
        (Some(4), &Value::String("runtime".into()))
    );
}

#[test]
fn search_runs_without_credentials_and_records_credential_index_zero() {
    let (output, request) = page(&["momentum", "--verbose"], work_list(1, &works(&["1"])));
    let attempt = &payload(&output)["provider_attempts"][0];

    assert_eq!(
        (
            output.status.code(),
            request.to_ascii_lowercase().contains("authorization:"),
            &attempt["provider"],
            &attempt["platform"],
            &attempt["credential_index"],
            &attempt["disposition"],
        ),
        (
            Some(0),
            false,
            &Value::String("ssrn_crossref".into()),
            &Value::String("ssrn".into()),
            &json!(0),
            &Value::String("succeeded".into()),
        )
    );
}

#[test]
fn markdown_shows_an_abstract_only_for_items_that_have_one() {
    let mut without_abstract = work("2");
    without_abstract["abstract"] = Value::Null;
    let (output, _) = page(
        &["momentum", "--format", "markdown"],
        work_list(2, &[without_abstract, work("1")]),
    );
    let markdown = String::from_utf8_lossy(&output.stdout);

    assert_eq!(
        (
            markdown.contains("  Abstract of 1."),
            markdown.contains("`ssrn:2` — Gary Antonacci — 2012\n- [Paper 1]"),
        ),
        (true, true),
        "{markdown}"
    );
}

fn assert_preflight_exit(config: &str, arguments: &[&str], exit_code: i32, message: &str) {
    let fixture = Fixture::start_canary();
    let environment = RunEnvironment::new(&config.replace("{url}", &fixture.url));

    let output = search(&environment, arguments);
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
fn a_search_without_a_query_fails_before_any_request() {
    for arguments in [&[][..], &["   "]] {
        assert_preflight_exit(ROUTE_CONFIG, arguments, 2, "ssrn search needs a query");
    }
}

#[test]
fn a_cursor_rejects_an_explicit_query_or_limit() {
    let cursor = cursor("momentum", 2, "2");
    for arguments in [
        &["x", "--cursor", cursor.as_str()][..],
        &["--cursor", cursor.as_str(), "--limit", "10"],
    ] {
        assert_preflight_exit(ROUTE_CONFIG, arguments, 2, "cannot be used with");
    }
}

#[test]
fn a_cursor_past_the_crossref_offset_limit_fails_before_any_request() {
    assert_preflight_exit(
        ROUTE_CONFIG,
        &["--cursor", &cursor("momentum", 20, "9981")],
        2,
        "ssrn_crossref cannot page past result 10000",
    );
}

#[test]
fn an_arxiv_cursor_is_rejected() {
    let payload = json!({"query": "x", "limit": 2, "options": {"platform": "arxiv"}, "page": "2"});
    let arxiv_cursor = format!(
        "v1.arxiv_api.{}",
        URL_SAFE_NO_PAD.encode(payload.to_string())
    );

    assert_preflight_exit(
        ROUTE_CONFIG,
        &["--cursor", &arxiv_cursor],
        2,
        "the cursor belongs to arxiv search, not ssrn search",
    );
}

#[test]
fn an_empty_platform_order_is_a_configuration_error() {
    assert_preflight_exit(
        "[providers.ssrn_crossref]\nurl = \"{url}\"\n[platforms.ssrn]\norder = []\n",
        &["momentum"],
        3,
        "platforms.ssrn.order has no configured route for ssrn search",
    );
}
