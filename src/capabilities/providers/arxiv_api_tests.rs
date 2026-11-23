use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::{Value, json};

use super::{
    ArxivApi, HtmlAvailability, content_urls, decode_feed, failure_message, html_probe_url,
    read_feed, search_support, select_paper,
};
use crate::config::HttpRouteRuntimeConfig;
use crate::fixture_server::{Fixture, Response};
use crate::net::{RetryPolicy, build_client};
use crate::providers::RouteContext;
use crate::rate_limit::test_pacing::paced_limiters;
use crate::samples::sample;
use crate::types::{
    ArxivRef, ArxivSearchOptions, ArxivSort, AttemptErrorKind, ContentDepth, Deadline,
    FullTextSource, Platform, PlatformFetchRequest, PlatformRef, PlatformSearchOptions,
    PlatformSearchRequest,
};

#[path = "../../../tests/support/arxiv_feed.rs"]
mod arxiv_feed;

use arxiv_feed::{feed_of, page};

fn feed() -> String {
    sample("arxiv_api/feed.xml")
}

fn arxiv_ref(input: &str) -> ArxivRef {
    ArxivRef::parse(input).expect("valid ref")
}

fn atom(status: u16, body: &str) -> Response {
    Response::new(status, "application/atom+xml", body)
}

/// The route against `url`, with three attempts per request and no wait between them.
fn route(url: &str, deadline: Duration) -> ArxivApi {
    ArxivApi::build(
        HttpRouteRuntimeConfig {
            url: format!("{url}/api/query"),
            timeout_seconds: 5,
        },
        RouteContext {
            client: build_client(true).expect("build client"),
            retry_policy: RetryPolicy::new(3, 1.0, Duration::ZERO),
            deadline: Deadline::new(deadline),
        },
    )
}

fn block_on<T>(future: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime")
        .block_on(future)
}

fn search_request(query: &str, options: ArxivSearchOptions, limit: u16) -> PlatformSearchRequest {
    PlatformSearchRequest {
        query: query.to_owned(),
        limit,
        options: PlatformSearchOptions::Arxiv(options),
        page: None,
    }
}

fn fetch_request(reference: &str, depth: ContentDepth) -> PlatformFetchRequest {
    PlatformFetchRequest::new(
        PlatformRef::parse(Platform::Arxiv, reference).expect("valid ref"),
        depth,
    )
}

fn query_pairs(request: &str) -> BTreeMap<String, String> {
    let target = request.split_whitespace().nth(1).expect("request target");
    reqwest::Url::parse(&format!("http://fixture.test{target}"))
        .expect("request URL")
        .query_pairs()
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}

fn items(body: &str) -> Value {
    let page = read_feed(body).expect("read feed");
    serde_json::to_value(page.items).expect("serialize items")
}

#[test]
fn an_unknown_html_probe_result_tries_html_before_the_pdf() {
    let urls = [
        HtmlAvailability::Present,
        HtmlAvailability::Unknown,
        HtmlAvailability::Absent,
    ]
    .map(|availability| content_urls(&arxiv_ref("arxiv:2401.01234v2"), availability));

    assert_eq!(
        urls,
        [
            vec![
                "https://arxiv.org/html/2401.01234v2".to_owned(),
                "https://arxiv.org/pdf/2401.01234v2".to_owned()
            ],
            vec![
                "https://arxiv.org/html/2401.01234v2".to_owned(),
                "https://arxiv.org/pdf/2401.01234v2".to_owned()
            ],
            vec!["https://arxiv.org/pdf/2401.01234v2".to_owned()],
        ]
    );
}

#[test]
fn the_html_probe_goes_to_the_query_api_host() {
    let url = html_probe_url(
        "https://export.arxiv.org/api/query",
        &arxiv_ref("arxiv:hep-th/9901001v3"),
    );

    assert_eq!(
        url.map(String::from).as_deref(),
        Some("https://export.arxiv.org/html/hep-th/9901001v3")
    );
}

#[test]
fn a_returned_paper_with_another_version_is_a_runtime_failure() {
    let page = read_feed(&feed_of(&feed(), "2401.01234v3")).expect("read feed");

    let result = select_paper(page.items, &arxiv_ref("arxiv:2401.01234v2")).map(|_| ());

    assert_eq!(
        result,
        Err((
            AttemptErrorKind::Runtime,
            "arXiv returned arxiv:2401.01234v3 for arxiv:2401.01234v2".to_owned()
        ))
    );
}

#[test]
fn an_empty_feed_for_a_requested_paper_is_the_requests_fault() {
    let result = select_paper(Vec::new(), &arxiv_ref("arxiv:2401.99999")).map(|_| ());

    assert_eq!(
        result,
        Err((
            AttemptErrorKind::Parameter,
            "arXiv item not found: arxiv:2401.99999".to_owned()
        ))
    );
}

#[test]
fn a_full_entry_maps_every_metadata_field() {
    assert_eq!(
        items(&feed()),
        json!([{
            "ref": "arxiv:2401.01234v2",
            "url": "https://arxiv.org/abs/2401.01234v2",
            "depth": "abstract",
            "title": "Dark Matter Halos & Galaxies",
            "authors": ["Ada Lovelace", "Alan Turing"],
            "published": "2024-01-02T18:59:59Z",
            "abstract": "First line\n  second line.",
            "updated": "2024-02-03T04:05:06Z",
            "primary_category": "astro-ph.CO",
            "categories": ["astro-ph.CO", "hep-ph"],
            "doi": "10.1000/example",
            "journal_ref": "Phys. Rev. D 1 (2024)",
            "comment": "12 pages, 3 figures",
            "pdf_url": "https://arxiv.org/pdf/2401.01234v2"
        }])
    );
}

#[test]
fn total_results_are_read_from_the_opensearch_extension() {
    let page = read_feed(&feed()).expect("read feed");

    assert_eq!(page.total_results, 42);
}

#[test]
fn a_successful_body_that_is_not_an_arxiv_feed_is_a_runtime_failure() {
    let result = read_feed(&sample("arxiv_api/maintenance.html")).map(|_| ());

    assert!(
        matches!(result, Err((AttemptErrorKind::Runtime, ref message)) if message.starts_with("invalid arXiv Atom feed")),
        "{result:?}"
    );
}

#[test]
fn an_error_entry_is_the_requests_fault_with_the_arxiv_message() {
    let result = read_feed(&sample("arxiv_api/feed-error.xml")).map(|_| ());

    assert_eq!(
        result,
        Err((
            AttemptErrorKind::Parameter,
            "arXiv rejected the request: Invalid query string: '('".to_owned()
        ))
    );
}

#[test]
fn an_entry_with_an_unrecognized_id_fails_to_map() {
    let feed = decode_feed(
        r#"<feed xmlns="http://www.w3.org/2005/Atom"><totalResults>1</totalResults><entry><id>http://example.org/x</id></entry></feed>"#,
    )
    .expect("decode feed");

    let result = feed
        .entries
        .into_iter()
        .map(super::Entry::into_item)
        .collect::<Result<Vec<_>, _>>();

    assert!(result.is_err());
}

#[test]
fn a_page_position_the_route_did_not_issue_is_refused_before_any_request() {
    let mut request = search_request("x", ArxivSearchOptions::default(), 2);
    request.page = Some("bogus".to_owned());

    assert_eq!(
        search_support(&request),
        Err("invalid arXiv page position `bogus`".to_owned())
    );
}

/// Searches a fresh fixture that answers `body` and returns the outcome and the received query.
fn search_page(
    request: &PlatformSearchRequest,
    body: &str,
) -> (
    crate::types::PlatformSearchOutcome,
    BTreeMap<String, String>,
) {
    let fixture = Fixture::start_sequence(vec![atom(200, body)]);
    let outcome = block_on(route(&fixture.url, Duration::from_secs(10)).search(request))
        .expect("search succeeds");
    (outcome, query_pairs(&fixture.finish()))
}

/// A search, its page size, the `search_query` it sends, and the other query parameters
/// that differ from the defaults.
type WireCase = (
    &'static str,
    ArxivSearchOptions,
    u16,
    &'static str,
    &'static [(&'static str, &'static str)],
);

#[expect(
    clippy::too_many_lines,
    reason = "the table holds one row per encoding"
)]
fn wire_cases() -> [WireCase; 16] {
    let date = |value: &str| chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").ok();
    let options = |change: &dyn Fn(&mut ArxivSearchOptions)| {
        let mut options = ArxivSearchOptions::default();
        change(&mut options);
        options
    };
    [
        (
            "dark matter",
            options(&|_| {}),
            10,
            r#"all:"dark" AND all:"matter""#,
            &[],
        ),
        (
            r#""world  model" robot"#,
            options(&|_| {}),
            10,
            r#"all:"world model" AND all:"robot""#,
            &[],
        ),
        (
            r#"a"b c"d "" "\e""#,
            options(&|_| {}),
            10,
            r#"all:"a" AND all:"b c" AND all:"d" AND all:"e""#,
            &[],
        ),
        (
            r#""ti:(foo) AND bar""#,
            options(&|_| {}),
            10,
            r#"all:"ti:(foo) AND bar""#,
            &[],
        ),
        (
            r#"ti:(foo) AND "bar""#,
            options(&|_| {}),
            10,
            r#"all:"ti:(foo)" AND all:"AND" AND all:"bar""#,
            &[],
        ),
        (
            "",
            options(&|options| options.categories = vec!["q-fin.PM".into()]),
            10,
            "cat:q-fin.PM",
            &[],
        ),
        (
            "",
            options(&|options| options.categories = vec!["cs.AI".into(), "cs.CL".into()]),
            10,
            "(cat:cs.AI OR cat:cs.CL)",
            &[],
        ),
        (
            "",
            options(&|options| options.author = Some("Geoffrey  Hinton".into())),
            10,
            r#"au:"Geoffrey Hinton""#,
            &[],
        ),
        (
            "",
            options(&|options| options.title = Some("attention is all".into())),
            10,
            r#"ti:"attention is all""#,
            &[],
        ),
        (
            "x",
            options(&|options| {
                options.submitted_from = date("2024-01-01");
                options.submitted_to = date("2024-01-31");
            }),
            10,
            r#"all:"x" AND submittedDate:[202401010000 TO 202401312359]"#,
            &[],
        ),
        (
            "x",
            options(&|options| options.submitted_from = date("2024-01-01")),
            10,
            r#"all:"x" AND submittedDate:[202401010000 TO 999912312359]"#,
            &[],
        ),
        (
            "x",
            options(&|options| options.submitted_to = date("2024-01-31")),
            10,
            r#"all:"x" AND submittedDate:[199101010000 TO 202401312359]"#,
            &[],
        ),
        (
            "x",
            options(&|options| options.sort = ArxivSort::Submitted),
            10,
            r#"all:"x""#,
            &[("sortBy", "submittedDate")],
        ),
        (
            "x",
            options(&|options| options.sort = ArxivSort::Updated),
            10,
            r#"all:"x""#,
            &[("sortBy", "lastUpdatedDate")],
        ),
        (
            "x",
            options(&|_| {}),
            25,
            r#"all:"x""#,
            &[("max_results", "25")],
        ),
        (
            "llm",
            options(&|options| {
                options.categories = vec!["cs.CL".into()];
                options.author = Some("Lewis".into());
                options.title = Some("retrieval".into());
                options.submitted_from = date("2020-05-01");
            }),
            10,
            r#"all:"llm" AND cat:cs.CL AND au:"Lewis" AND ti:"retrieval" AND submittedDate:[202005010000 TO 999912312359]"#,
            &[],
        ),
    ]
}

#[test]
fn every_search_parameter_has_a_fixed_wire_encoding() {
    for (query, options, limit, search_query, overrides) in wire_cases() {
        let request = search_request(query, options, limit);
        let mut expected = BTreeMap::from([
            ("search_query".to_owned(), search_query.to_owned()),
            ("start".to_owned(), "0".to_owned()),
            ("max_results".to_owned(), "10".to_owned()),
            ("sortBy".to_owned(), "relevance".to_owned()),
            ("sortOrder".to_owned(), "descending".to_owned()),
        ]);
        for (name, value) in overrides {
            expected.insert((*name).to_owned(), (*value).to_owned());
        }

        let (_, sent) = search_page(&request, &page(&feed(), 0, &[]));

        assert_eq!(sent, expected, "query: {query:?}");
    }
}

#[test]
fn a_full_page_below_the_total_continues_at_the_next_start_and_one_that_reaches_it_ends() {
    let ids = ["2401.00001v1", "2401.00002v1"];
    let request = search_request("neutrino", ArxivSearchOptions::default(), 2);
    let mut later = request.clone();
    later.page = Some("2".to_owned());

    let (below, _) = search_page(&request, &page(&feed(), 5, &ids));
    let (reaching, sent) = search_page(&later, &page(&feed(), 4, &ids));

    assert_eq!(
        (below.next_page, reaching.next_page, sent["start"].as_str()),
        (Some("2".to_owned()), None, "2")
    );
}

#[test]
fn a_legitimate_empty_feed_is_an_empty_page() {
    let request = search_request("no such words", ArxivSearchOptions::default(), 10);

    let (outcome, _) = search_page(&request, &page(&feed(), 0, &[]));

    assert_eq!((outcome.items.len(), outcome.next_page), (0, None));
}

#[test]
fn search_sends_no_credential_and_records_an_anonymous_attempt() {
    let fixture = Fixture::start_sequence(vec![atom(200, &feed())]);
    let request = search_request("electron", ArxivSearchOptions::default(), 10);

    let outcome = block_on(route(&fixture.url, Duration::from_secs(10)).search(&request))
        .expect("search succeeds");
    let sent = fixture.finish();
    let attempt = serde_json::to_value(&outcome.attempts[0]).expect("serialize attempt");

    assert_eq!(
        (
            sent.to_ascii_lowercase().contains("authorization:"),
            &attempt["provider"],
            &attempt["credential_index"],
            &attempt["rotation_count"],
            &attempt["disposition"],
            &attempt["anonymous"],
        ),
        (
            false,
            &json!("arxiv_api"),
            &json!(0),
            &json!(0),
            &json!("succeeded"),
            &json!(true),
        )
    );
}

#[test]
fn an_atom_error_entry_fails_with_the_arxiv_message_whatever_the_status() {
    for status in [400, 200] {
        let fixture =
            Fixture::start_sequence(vec![atom(status, &sample("arxiv_api/feed-error.xml"))]);
        let request = search_request("x", ArxivSearchOptions::default(), 10);

        let error = block_on(route(&fixture.url, Duration::from_secs(10)).search(&request))
            .map(|_| ())
            .expect_err("an error entry fails the search");
        fixture.finish();
        let attempt = serde_json::to_value(&error.attempts[0]).expect("serialize attempt");

        assert_eq!(
            (error.kind, error.message.as_str(), &attempt["platform"]),
            (
                AttemptErrorKind::Parameter,
                "arXiv rejected the request: Invalid query string: '('",
                &json!("arxiv"),
            ),
            "status: {status}"
        );
    }
}

#[test]
fn a_failed_status_reads_the_error_entry_or_names_the_status() {
    let error_feed = r#"<feed xmlns="http://www.w3.org/2005/Atom"><totalResults>1</totalResults><entry><id>http://arxiv.org/api/errors#bad</id><summary>bad id</summary></entry></feed>"#;

    assert_eq!(
        [
            failure_message(error_feed, 400),
            failure_message("<html>busy</html>", 503)
        ],
        [
            "arXiv rejected the request: bad id".to_owned(),
            "arXiv returned HTTP 503".to_owned()
        ]
    );
}

#[test]
fn a_rate_limited_query_api_ends_the_route_without_retry() {
    let fixture = Fixture::start_sequence(vec![atom(429, "Rate exceeded.")]);
    let request = fetch_request("arxiv:2401.01234", ContentDepth::Abstract);

    let error = block_on(route(&fixture.url, Duration::from_secs(10)).fetch(&request))
        .map(|_| ())
        .expect_err("a rate-limited fetch fails");

    assert_eq!(
        (error.kind, fixture.finish_all().len()),
        (AttemptErrorKind::RateLimited, 1)
    );
}

#[test]
fn fetch_by_url_requests_the_version_the_url_names() {
    let fixture = Fixture::start_sequence(vec![atom(200, &feed_of(&feed(), "hep-th/9901001v1"))]);
    let request = fetch_request(
        "https://arxiv.org/pdf/hep-th/9901001v1.pdf",
        ContentDepth::Abstract,
    );

    let outcome = block_on(route(&fixture.url, Duration::from_secs(10)).fetch(&request))
        .expect("fetch succeeds");
    let sent = fixture.finish();

    assert_eq!(
        (
            query_pairs(&sent).get("id_list").cloned(),
            outcome.item.reference.to_string(),
        ),
        (
            Some("hep-th/9901001v1".to_owned()),
            "arxiv:hep-th/9901001v1".to_owned()
        )
    );
}

#[test]
fn an_abstract_fetch_reads_the_metadata_alone_and_declares_no_full_text() {
    let fixture = Fixture::start_sequence(vec![atom(200, &feed())]);
    let request = fetch_request("arxiv:2401.01234v2", ContentDepth::Abstract);

    let outcome = block_on(route(&fixture.url, Duration::from_secs(10)).fetch(&request))
        .expect("fetch succeeds");

    assert_eq!(
        (fixture.finish_all().len(), outcome.content_source),
        (1, FullTextSource::Urls(Vec::new()))
    );
}

#[test]
fn a_full_text_fetch_probes_the_returned_version_and_orders_its_urls() {
    for (probe, urls) in [
        (
            200,
            vec![
                "https://arxiv.org/html/2401.01234v2".to_owned(),
                "https://arxiv.org/pdf/2401.01234v2".to_owned(),
            ],
        ),
        (404, vec!["https://arxiv.org/pdf/2401.01234v2".to_owned()]),
    ] {
        let fixture = Fixture::start_sequence(vec![
            atom(200, &feed()),
            Response::new(probe, "text/html", ""),
        ]);
        let request = fetch_request("arxiv:2401.01234", ContentDepth::FullText);

        let outcome = block_on(route(&fixture.url, Duration::from_secs(10)).fetch(&request))
            .expect("fetch succeeds");
        let sent = fixture.finish_all();

        assert_eq!(
            (
                query_pairs(&sent[0]).get("id_list").cloned(),
                sent[1].lines().next(),
                outcome.content_source,
            ),
            (
                Some("2401.01234".to_owned()),
                Some("HEAD /html/2401.01234v2 HTTP/1.1"),
                FullTextSource::Urls(urls),
            ),
            "probe status: {probe}"
        );
    }
}

#[test]
fn a_probe_window_beyond_the_budget_ends_the_fetch_as_timeout() {
    let fixture = Fixture::start_sequence(vec![atom(200, &feed())]);
    let request = fetch_request("arxiv:2401.01234", ContentDepth::FullText);
    // The probe's window opens a full interval after the metadata request, past this budget,
    // so the reservation fails at once and no test waits.
    let route = {
        let _paced = paced_limiters();
        route(&fixture.url, Duration::from_secs(2))
    };

    let error = block_on(route.fetch(&request))
        .map(|_| ())
        .expect_err("the probe does not fit the budget");
    let attempts = error
        .attempts
        .iter()
        .map(|attempt| (attempt.http_status, attempt.error_kind))
        .collect::<Vec<_>>();

    assert_eq!(
        (error.kind, attempts, fixture.finish_all().len()),
        (
            AttemptErrorKind::Timeout,
            vec![(Some(200), None), (None, Some(AttemptErrorKind::Timeout))],
            1
        )
    );
}

#[test]
fn an_entry_without_optional_metadata_maps_absent_fields_to_null() {
    let body = r#"<feed xmlns="http://www.w3.org/2005/Atom" xmlns:opensearch="http://a9.com/-/spec/opensearch/1.1/">
  <opensearch:totalResults>1</opensearch:totalResults>
  <entry>
<id>http://arxiv.org/abs/hep-th/9901001v1</id>
<title>Old paper</title>
<summary>Abstract.</summary>
<author><name>A. Author</name></author>
  </entry>
</feed>"#;

    assert_eq!(
        items(body),
        json!([{
            "ref": "arxiv:hep-th/9901001v1",
            "url": "https://arxiv.org/abs/hep-th/9901001v1",
            "depth": "abstract",
            "title": "Old paper",
            "authors": ["A. Author"],
            "published": null,
            "abstract": "Abstract.",
            "updated": null,
            "primary_category": null,
            "categories": [],
            "doi": null,
            "journal_ref": null,
            "comment": null,
            "pdf_url": null
        }])
    );
}
