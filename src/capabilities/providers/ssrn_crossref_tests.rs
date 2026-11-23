use std::collections::BTreeMap;
use std::time::Duration;

use chrono::NaiveDate;
use serde_json::{Value, json};

use super::search::SearchPage;
use super::{
    SsrnCrossref, Work, WorkList, decode_message, failure_message, fetch_support, search_support,
    select_paper,
};
use crate::config::HttpRouteRuntimeConfig;
use crate::fixture_server::{Fixture, Response, block_on, path, query};
use crate::net::{RetryPolicy, build_client};
use crate::providers::RouteContext;
// The stored bodies below read the sample store through this name.
use crate::samples;
use crate::types::{
    AttemptDisposition, AttemptErrorKind, AttemptTarget, ContentDepth, Deadline, FullTextSource,
    PlatformFetchOutcome, PlatformFetchRequest, PlatformRef, PlatformSearchOptions,
    PlatformSearchOutcome, PlatformSearchRequest, ProviderError, SsrnDateRange, SsrnRef,
    SsrnSearchOptions, SsrnSearchScope, SsrnSort, SsrnSortOrder, SsrnWorkType,
};

#[path = "../../../tests/support/ssrn_samples.rs"]
mod stored;

use stored::{listed_work, stored_json, work_list, work_with};

const SELECT: &str = "DOI,title,author,abstract,published,type,created,resource";

/// The route against the fixture at `url`, built as its factory does. Each request has
/// `attempts` attempts with no wait between them.
fn route_with_attempts(url: &str, attempts: usize) -> SsrnCrossref {
    SsrnCrossref::build(
        HttpRouteRuntimeConfig {
            url: url.to_owned(),
            timeout_seconds: 5,
        },
        RouteContext {
            client: build_client(true).expect("build client"),
            retry_policy: RetryPolicy::new(attempts, 1.0, Duration::ZERO),
            deadline: Deadline::new(Duration::from_secs(10)),
        },
    )
}

fn json_response(body: &Value) -> Response {
    Response::json(200, &body.to_string())
}

fn to_json<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("serialize")
}

fn pairs(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

fn failure<T>(result: Result<T, ProviderError>) -> (AttemptErrorKind, String) {
    match result {
        Ok(_) => panic!("expected a failure"),
        Err(error) => (error.kind, error.message),
    }
}

fn works(ids: &[&str]) -> Vec<Value> {
    ids.iter().map(|id| listed_work(id)).collect()
}

fn first_page(query: &str, limit: u16, options: SsrnSearchOptions) -> PlatformSearchRequest {
    PlatformSearchRequest {
        query: query.to_owned(),
        limit,
        options: PlatformSearchOptions::Ssrn(options),
        page: None,
    }
}

fn at_page(request: PlatformSearchRequest, page: &str) -> PlatformSearchRequest {
    PlatformSearchRequest {
        page: Some(page.to_owned()),
        ..request
    }
}

/// Searches with `request` against a fixture that answers `answers`, and returns the outcome
/// and the requests the fixture received.
fn search(
    request: &PlatformSearchRequest,
    answers: Vec<Response>,
) -> (Result<PlatformSearchOutcome, ProviderError>, Vec<String>) {
    let fixture = Fixture::start_sequence(answers);
    let outcome = block_on(route_with_attempts(&fixture.url, 1).search(request));
    (outcome, fixture.finish_all())
}

fn next_page(
    outcome: Result<PlatformSearchOutcome, ProviderError>,
) -> Result<Option<String>, String> {
    outcome
        .map(|outcome| outcome.next_page)
        .map_err(|error| error.message)
}

fn paper() -> SsrnRef {
    SsrnRef::parse("ssrn:2042750").expect("valid ref")
}

/// Fetches `ssrn:2042750` at `depth` against a fixture that answers `answer`, and returns the
/// outcome and the requests the fixture received.
fn fetch(
    depth: ContentDepth,
    answer: Response,
) -> (Result<PlatformFetchOutcome, ProviderError>, Vec<String>) {
    let fixture = Fixture::start_sequence(vec![answer]);
    let request = PlatformFetchRequest::new(PlatformRef::Ssrn(paper()), depth);
    let outcome = block_on(route_with_attempts(&fixture.url, 1).fetch(&request));
    (outcome, fixture.finish_all())
}

// Search.

#[test]
fn a_search_page_decodes_its_ssrn_records_and_names_the_records_it_drops() {
    let list = decode_message::<WorkList>(&stored::stored("work-list.json"), "work-list")
        .expect("a work list");

    let page = SearchPage::from_list(list, 0, 10);

    assert_eq!(
        (to_json(&page.items), page.next_page, page.skipped_dois),
        (
            json!([
                {
                    "ref": "ssrn:1",
                    "url": "https://papers.ssrn.com/sol3/papers.cfm?abstract_id=1",
                    "depth": "abstract",
                    "title": "Paper 1",
                    "authors": ["Gary Antonacci"],
                    "published": "2012",
                    "abstract": "Abstract of 1.",
                    "doi": "10.2139/ssrn.1",
                    "crossref_type": "posted-content",
                    "crossref_created": "2012-04-25T11:01:44Z"
                },
                {
                    "ref": "ssrn:2",
                    "url": "https://papers.ssrn.com/sol3/papers.cfm?abstract_id=2",
                    "depth": "metadata",
                    "title": "Paper 2",
                    "authors": ["Gary Antonacci"],
                    "published": "2012",
                    "abstract": null,
                    "doi": "10.2139/ssrn.2",
                    "crossref_type": "journal-article",
                    "crossref_created": "2012-04-25T11:01:44Z"
                }
            ]),
            None,
            vec!["10.1016/j.jfineco.2020.01.001".to_owned()]
        )
    );
}

#[test]
fn a_search_sends_the_prefix_query_with_fixed_ranking_and_fields() {
    let request = first_page("dual momentum", 10, SsrnSearchOptions::default());

    let (outcome, requests) = search(&request, vec![json_response(&work_list(0, &[]))]);

    assert_eq!(
        (
            outcome
                .map(|outcome| outcome.items.len())
                .map_err(|error| error.message),
            path(&requests[0]),
            query(&requests[0])
        ),
        (
            Ok(0),
            "/prefixes/10.2139/works".to_owned(),
            pairs(&[
                ("query", "dual momentum"),
                ("rows", "10"),
                ("offset", "0"),
                ("sort", "score"),
                ("order", "desc"),
                ("select", SELECT),
            ])
        )
    );
}

#[test]
fn records_without_an_ssrn_doi_are_named_in_one_diagnostic() {
    let request = first_page("momentum", 10, SsrnSearchOptions::default());

    let (outcome, _) = search(
        &request,
        vec![Response::json(200, &stored::stored("work-list.json"))],
    );

    assert_eq!(
        outcome
            .map(|outcome| (outcome.items.len(), outcome.diagnostic))
            .map_err(|error| error.message),
        Ok((
            2,
            Some(
                "ssrn_crossref skipped 1 Crossref records without an SSRN DOI: 10.1016/j.jfineco.2020.01.001"
                    .to_owned()
            )
        ))
    );
}

fn every_criterion() -> SsrnSearchOptions {
    let day = |value: &str| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok();
    SsrnSearchOptions {
        scope: SsrnSearchScope::Bibliographic,
        author: Some("A B".to_owned()),
        affiliation: Some("Example University".to_owned()),
        published: SsrnDateRange {
            from: day("2020-01-01"),
            to: day("2024-12-31"),
        },
        created: SsrnDateRange {
            from: day("2019-01-01"),
            to: day("2025-01-01"),
        },
        updated: SsrnDateRange {
            from: day("2024-01-01"),
            to: day("2025-12-31"),
        },
        has_abstract: true,
        work_type: Some(SsrnWorkType::JournalArticle),
        orcid: Some("0000-0002-1825-0097".to_owned()),
        funder: Some("10.13039/100000001".to_owned()),
        sort: SsrnSort::Citations,
        order: SsrnSortOrder::Asc,
    }
}

#[test]
fn every_criterion_maps_to_its_crossref_parameter_and_a_page_position_changes_only_the_offset() {
    let request = first_page("risk & return", 1, every_criterion());

    let (first, first_requests) =
        search(&request, vec![json_response(&work_list(2, &works(&["1"])))]);
    let (_, next_requests) = search(
        &at_page(request, "1"),
        vec![json_response(&work_list(2, &works(&["2"])))],
    );

    let expected = pairs(&[
        ("query.bibliographic", "risk & return"),
        ("query.author", "A B"),
        ("query.affiliation", "Example University"),
        ("rows", "1"),
        ("offset", "0"),
        ("sort", "is-referenced-by-count"),
        ("order", "asc"),
        ("select", SELECT),
        (
            "filter",
            "from-pub-date:2020-01-01,until-pub-date:2024-12-31,from-created-date:2019-01-01,until-created-date:2025-01-01,from-update-date:2024-01-01,until-update-date:2025-12-31,has-abstract:true,type:journal-article,orcid:0000-0002-1825-0097,funder:10.13039/100000001",
        ),
    ]);
    let mut next = expected.clone();
    next.insert("offset".to_owned(), "1".to_owned());
    assert_eq!(
        (
            next_page(first),
            query(&first_requests[0]),
            query(&next_requests[0])
        ),
        (Ok(Some("1".to_owned())), expected, next)
    );
}

#[test]
fn a_title_scope_queries_the_title_field_beside_an_author_query() {
    let request = first_page(
        "dual momentum",
        10,
        SsrnSearchOptions {
            scope: SsrnSearchScope::Title,
            author: Some("Antonacci".to_owned()),
            ..SsrnSearchOptions::default()
        },
    );

    let (outcome, requests) = search(&request, vec![json_response(&work_list(0, &[]))]);
    let sent = query(&requests[0]);

    assert_eq!(
        (
            outcome.map(|_| ()).map_err(|error| error.message),
            sent.get("query.title").map(String::as_str),
            sent.get("query.author").map(String::as_str),
            sent.contains_key("query")
        ),
        (Ok(()), Some("dual momentum"), Some("Antonacci"), false)
    );
}

#[test]
fn each_date_sort_maps_to_its_own_crossref_field() {
    let sent = [SsrnSort::Published, SsrnSort::Created, SsrnSort::Updated].map(|sort| {
        let request = first_page(
            "momentum",
            10,
            SsrnSearchOptions {
                sort,
                ..SsrnSearchOptions::default()
            },
        );
        let (_, requests) = search(&request, vec![json_response(&work_list(0, &[]))]);
        query(&requests[0])["sort"].clone()
    });

    assert_eq!(sent, ["published", "created", "updated"]);
}

#[test]
fn a_full_first_page_short_of_the_total_names_the_offset_after_it() {
    let request = first_page("momentum", 2, SsrnSearchOptions::default());

    let (outcome, requests) = search(
        &request,
        vec![json_response(&work_list(5, &works(&["1", "2"])))],
    );

    assert_eq!(
        (next_page(outcome), query(&requests[0])["offset"].as_str()),
        (Ok(Some("2".to_owned())), "0")
    );
}

#[test]
fn a_page_position_is_sent_as_the_crossref_offset() {
    let request = first_page("momentum", 2, SsrnSearchOptions::default());

    let (middle, middle_requests) = search(
        &at_page(request.clone(), "2"),
        vec![json_response(&work_list(5, &works(&["3", "4"])))],
    );
    let (last, last_requests) = search(
        &at_page(request, "4"),
        vec![json_response(&work_list(5, &works(&["5"])))],
    );

    assert_eq!(
        (
            [&middle_requests[0], &last_requests[0]]
                .map(|request| query(request)["offset"].clone()),
            next_page(middle),
            next_page(last)
        ),
        (
            ["2".to_owned(), "4".to_owned()],
            Ok(Some("4".to_owned())),
            Ok(None)
        )
    );
}

#[test]
fn a_full_page_that_reaches_the_total_names_no_next_page() {
    let request = first_page("momentum", 2, SsrnSearchOptions::default());

    let (outcome, _) = search(
        &request,
        vec![json_response(&work_list(2, &works(&["1", "2"])))],
    );

    assert_eq!(next_page(outcome), Ok(None));
}

#[test]
fn the_last_page_is_judged_before_non_ssrn_records_are_dropped() {
    let elsewhere = stored_json("work-list.json")["message"]["items"][1].clone();
    let request = first_page("momentum", 2, SsrnSearchOptions::default());

    let (outcome, _) = search(
        &request,
        vec![json_response(&work_list(
            10,
            &[listed_work("1"), elsewhere],
        ))],
    );

    assert_eq!(
        outcome
            .map(|outcome| (outcome.items.len(), outcome.next_page))
            .map_err(|error| error.message),
        Ok((1, Some("2".to_owned())))
    );
}

#[test]
fn no_next_page_is_named_for_a_page_that_would_end_past_the_crossref_offset_limit() {
    let ids = (0..20)
        .map(|index| (9980 + index).to_string())
        .collect::<Vec<_>>();
    let ids = ids.iter().map(String::as_str).collect::<Vec<_>>();
    let request = first_page("momentum", 20, SsrnSearchOptions::default());

    let (before_limit, _) = search(
        &at_page(request.clone(), "9960"),
        vec![json_response(&work_list(50_000, &works(&ids)))],
    );
    let (at_limit, requests) = search(
        &at_page(request, "9980"),
        vec![json_response(&work_list(50_000, &works(&ids)))],
    );

    assert_eq!(
        (
            next_page(before_limit),
            query(&requests[0])["offset"].as_str(),
            next_page(at_limit)
        ),
        (Ok(Some("9980".to_owned())), "9980", Ok(None))
    );
}

#[test]
fn a_page_past_the_crossref_offset_limit_is_refused_before_any_request() {
    let request = at_page(
        first_page("momentum", 20, SsrnSearchOptions::default()),
        "9981",
    );

    let (outcome, requests) = search(&request, Vec::new());

    assert_eq!(
        (search_support(&request), failure(outcome), requests.len()),
        (
            Err("ssrn_crossref cannot page past result 10000".to_owned()),
            (
                AttemptErrorKind::Parameter,
                "ssrn_crossref cannot page past result 10000".to_owned()
            ),
            0
        )
    );
}

#[test]
fn a_legitimate_empty_result_is_an_empty_success() {
    let request = first_page("no such words", 10, SsrnSearchOptions::default());

    let (outcome, _) = search(&request, vec![json_response(&work_list(0, &[]))]);

    assert_eq!(
        outcome
            .map(|outcome| (outcome.items.len(), outcome.next_page))
            .map_err(|error| error.message),
        Ok((0, None))
    );
}

#[test]
fn a_search_answered_with_another_message_type_is_a_runtime_failure() {
    let request = first_page("momentum", 10, SsrnSearchOptions::default());

    let (outcome, _) = search(
        &request,
        vec![Response::json(200, &stored::stored("work.json"))],
    );

    assert_eq!(
        failure(outcome),
        (
            AttemptErrorKind::Runtime,
            "Crossref returned message type `work`, not `work-list`".to_owned()
        )
    );
}

#[test]
fn a_search_sends_no_credential_and_records_one_anonymous_attempt() {
    let request = first_page("momentum", 10, SsrnSearchOptions::default());

    let (outcome, requests) = search(&request, vec![json_response(&work_list(1, &works(&["1"])))]);

    let outcome = outcome.expect("a page");
    let attempts = outcome
        .attempts
        .iter()
        .map(|attempt| {
            (
                attempt.provider,
                attempt.target,
                attempt.disposition,
                attempt.credential_index,
                attempt.anonymous,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        (
            requests[0].to_ascii_lowercase().contains("authorization:"),
            attempts
        ),
        (
            false,
            vec![(
                "ssrn_crossref",
                AttemptTarget::platform("ssrn", "search"),
                AttemptDisposition::Succeeded,
                0,
                true
            )]
        )
    );
}

// Fetch.

#[test]
fn a_work_decodes_to_the_paper_with_its_abstract() {
    let work = decode_message::<Work>(&stored::stored("work.json"), "work").expect("a work");

    let item = select_paper(work, &paper()).expect("the requested paper");

    assert_eq!(
        to_json(&item),
        json!({
            "ref": "ssrn:2042750",
            "url": "https://papers.ssrn.com/sol3/papers.cfm?abstract_id=2042750",
            "depth": "abstract",
            "title": "Risk Premia Harvesting Through Dual Momentum",
            "authors": ["Gary Antonacci"],
            "published": "2012-04",
            "abstract": "Momentum is the premier market anomaly.",
            "doi": "10.2139/ssrn.2042750",
            "crossref_type": "posted-content",
            "crossref_created": "2012-04-25T11:01:44Z"
        })
    );
}

#[test]
fn a_fetch_reads_the_work_of_the_papers_ssrn_doi() {
    let (outcome, requests) = fetch(
        ContentDepth::Metadata,
        Response::json(200, &stored::stored("work.json")),
    );

    let outcome = outcome.expect("a paper");
    assert_eq!(
        (
            path(&requests[0]),
            outcome.item.reference.to_string(),
            outcome.item.depth,
            outcome.content_source
        ),
        (
            "/works/10.2139/ssrn.2042750".to_owned(),
            "ssrn:2042750".to_owned(),
            ContentDepth::Abstract,
            FullTextSource::Urls(Vec::new())
        )
    );
}

#[test]
fn a_record_without_an_abstract_succeeds_at_metadata_depth() {
    let (outcome, _) = fetch(
        ContentDepth::Metadata,
        json_response(&work_with(&json!({"abstract": null}))),
    );

    let item = to_json(&outcome.expect("a paper").item);
    assert_eq!(
        (&item["depth"], &item["abstract"]),
        (&json!("metadata"), &Value::Null)
    );
}

#[test]
fn an_abstract_request_succeeds_only_with_an_abstract() {
    let (with_abstract, _) = fetch(
        ContentDepth::Abstract,
        Response::json(200, &stored::stored("work.json")),
    );
    let (without_abstract, _) = fetch(
        ContentDepth::Abstract,
        json_response(&work_with(&json!({"abstract": null}))),
    );

    assert_eq!(
        (
            with_abstract
                .map(|outcome| outcome.item.depth)
                .map_err(|error| error.message),
            failure(without_abstract)
        ),
        (
            Ok(ContentDepth::Abstract),
            (
                AttemptErrorKind::Quality,
                "Crossref has no abstract for ssrn:2042750".to_owned()
            )
        )
    );
}

#[test]
fn a_missing_doi_names_crossref_as_the_source() {
    let (outcome, _) = fetch(
        ContentDepth::Metadata,
        Response::new(404, "text/plain", &stored::stored("not-found.txt")),
    );

    assert_eq!(
        failure(outcome),
        (
            AttemptErrorKind::Parameter,
            "SSRN paper not found in Crossref: ssrn:2042750".to_owned()
        )
    );
}

#[test]
fn a_rate_limited_request_ends_the_route_without_retrying() {
    let fixture = Fixture::start_sequence(vec![Response::new(429, "text/plain", "")]);
    let request = PlatformFetchRequest::new(PlatformRef::Ssrn(paper()), ContentDepth::Metadata);

    let outcome = block_on(route_with_attempts(&fixture.url, 3).fetch(&request));

    let error = outcome.err().expect("a rate limit ends the route");
    assert_eq!(
        (error.kind, error.attempts.len(), fixture.finish_all().len()),
        (AttemptErrorKind::RateLimited, 1, 1)
    );
}

#[test]
fn a_record_for_another_doi_is_a_runtime_failure() {
    let (outcome, _) = fetch(
        ContentDepth::Metadata,
        json_response(&work_with(&json!({"DOI": "10.2139/ssrn.1"}))),
    );

    assert_eq!(
        failure(outcome),
        (
            AttemptErrorKind::Runtime,
            "Crossref returned DOI 10.2139/ssrn.1 for ssrn:2042750".to_owned()
        )
    );
}

#[test]
fn a_fetch_answered_with_another_message_type_is_a_runtime_failure() {
    let (outcome, _) = fetch(
        ContentDepth::Metadata,
        Response::json(200, &stored::stored("work-list.json")),
    );

    assert_eq!(
        failure(outcome),
        (
            AttemptErrorKind::Runtime,
            "Crossref returned message type `work-list`, not `work`".to_owned()
        )
    );
}

// Mapping and support.

fn work(value: serde_json::Value) -> Work {
    serde_json::from_value(value).expect("decode work")
}

fn item(value: serde_json::Value) -> serde_json::Value {
    let item = work(value).into_item().expect("SSRN DOI");
    serde_json::to_value(item).expect("serialize item")
}

#[test]
fn a_full_record_maps_every_crossref_field() {
    let mapped = item(json!({
        "DOI": "10.2139/ssrn.2042750",
        "title": ["Risk Premia  Harvesting Through <i>Dual</i> Momentum"],
        "author": [
            {"given": "Gary", "family": "Antonacci", "sequence": "first"},
            {"name": "Example  Institute"}
        ],
        "abstract": "<jats:p>Momentum is the premier\n market anomaly.</jats:p><jats:p>Second &amp; last.</jats:p>",
        "published": {"date-parts": [[2012, 4, 19]]},
        "type": "posted-content",
        "created": {"date-parts": [[2012, 4, 25]], "date-time": "2012-04-25T11:01:44Z"},
        "resource": {"primary": {"URL": "https://www.ssrn.com/abstract=2042750"}}
    }));

    assert_eq!(
        mapped,
        json!({
            "ref": "ssrn:2042750",
            "url": "https://papers.ssrn.com/sol3/papers.cfm?abstract_id=2042750",
            "depth": "abstract",
            "title": "Risk Premia Harvesting Through Dual Momentum",
            "authors": ["Gary Antonacci", "Example Institute"],
            "published": "2012-04-19",
            "abstract": "Momentum is the premier market anomaly.\n\nSecond & last.",
            "doi": "10.2139/ssrn.2042750",
            "crossref_type": "posted-content",
            "crossref_created": "2012-04-25T11:01:44Z"
        })
    );
}

#[test]
fn a_record_without_an_abstract_has_metadata_depth_and_a_null_abstract() {
    let mapped = item(json!({"DOI": "10.2139/ssrn.1", "title": ["T"]}));

    assert_eq!(
        (&mapped["depth"], &mapped["abstract"], &mapped["published"]),
        (&json!("metadata"), &json!(null), &json!(null))
    );
}

#[test]
fn an_abstract_that_is_empty_after_cleaning_counts_as_missing() {
    let mapped = item(json!({
        "DOI": "10.2139/ssrn.1",
        "abstract": "<jats:p> &#x20; </jats:p>"
    }));

    assert_eq!(
        (&mapped["depth"], &mapped["abstract"]),
        (&json!("metadata"), &json!(null))
    );
}

#[test]
fn published_dates_keep_their_original_precision() {
    let dates = [
        json!([[2012]]),
        json!([[2012, 4]]),
        json!([[2012, 4, 9]]),
        json!([[null]]),
    ]
    .map(|parts| {
        item(json!({"DOI": "10.2139/ssrn.1", "published": {"date-parts": parts}}))["published"]
            .clone()
    });

    assert_eq!(
        dates,
        [
            json!("2012"),
            json!("2012-04"),
            json!("2012-04-09"),
            json!(null)
        ]
    );
}

#[test]
fn the_crossref_created_date_never_fills_the_published_date() {
    let mapped = item(json!({
        "DOI": "10.2139/ssrn.1",
        "created": {"date-time": "2012-04-25T11:01:44Z"}
    }));

    assert_eq!(mapped["published"], json!(null));
}

#[test]
fn a_record_without_an_ssrn_doi_yields_its_doi() {
    let result = work(json!({"DOI": "10.1016/j.jfineco.2020.01.001"})).into_item();

    assert_eq!(
        result.map(|_| ()),
        Err("10.1016/j.jfineco.2020.01.001".to_owned())
    );
}

#[test]
fn a_returned_record_for_another_paper_is_rejected() {
    let requested = SsrnRef::parse("ssrn:2042750").expect("valid ref");

    let result = select_paper(work(json!({"DOI": "10.2139/ssrn.1"})), &requested);

    assert_eq!(
        result.map(|_| ()),
        Err("Crossref returned DOI 10.2139/ssrn.1 for ssrn:2042750".to_owned())
    );
}

#[test]
fn a_returned_doi_in_another_case_is_the_same_paper() {
    let requested = SsrnRef::parse("ssrn:2042750").expect("valid ref");

    let result = select_paper(work(json!({"DOI": "10.2139/SSRN.2042750"})), &requested);

    assert!(result.is_ok());
}

#[test]
fn an_unexpected_message_type_fails_to_decode() {
    let result = decode_message::<WorkList>(
        r#"{"status":"ok","message-type":"work","message":{"DOI":"10.2139/ssrn.1"}}"#,
        "work-list",
    );

    assert_eq!(
        result.map(|_| ()).unwrap_err(),
        "Crossref returned message type `work`, not `work-list`"
    );
}

#[test]
fn a_body_that_is_not_json_fails_to_decode() {
    let result = decode_message::<WorkList>("<html>busy</html>", "work-list");

    assert!(result.is_err());
}

#[test]
fn a_failed_status_reads_the_validation_message_or_names_the_status() {
    assert_eq!(
        [
            failure_message(&stored::stored("validation-failure.json"), 400),
            failure_message(&stored::stored("not-found.txt"), 404)
        ],
        [
            "Crossref rejected the request: Offset specified as 10001 is too large".to_owned(),
            "Crossref returned HTTP 404".to_owned()
        ]
    );
}

fn search_request(limit: u16, page: Option<&str>) -> PlatformSearchRequest {
    PlatformSearchRequest {
        query: "momentum".into(),
        limit,
        options: PlatformSearchOptions::Ssrn(SsrnSearchOptions::default()),
        page: page.map(str::to_owned),
    }
}

#[test]
fn search_support_rejects_pages_that_end_past_the_crossref_offset_limit() {
    let results = [
        search_request(20, Some("9980")),
        search_request(20, Some("9981")),
        search_request(100, Some("9900")),
    ]
    .map(|request| search_support(&request).is_ok());

    assert_eq!(results, [true, false, true]);
}

#[test]
fn search_support_rejects_a_page_position_it_did_not_issue() {
    let result = search_support(&search_request(10, Some("bogus")));

    assert_eq!(
        result,
        Err("invalid Crossref page position `bogus`".to_owned())
    );
}

#[test]
fn fetch_supports_metadata_and_abstract_depths_only() {
    let reference = PlatformRef::parse(crate::types::Platform::Ssrn, "ssrn:1").expect("ref");
    let results = [
        ContentDepth::Metadata,
        ContentDepth::Abstract,
        ContentDepth::FullText,
    ]
    .map(|depth| fetch_support(&PlatformFetchRequest::new(reference.clone(), depth)).is_ok());

    assert_eq!(results, [true, true, false]);
}
