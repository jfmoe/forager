use clap::error::ErrorKind;

use super::super::{PageInput, PlatformCommand, parse_platform};
use super::{
    ScholarCitedByArgs, ScholarCommand, ScholarFetchArgs, ScholarSearchArgs, cited_by_input,
    scholar_fetch_input, scholar_search_input,
};
use crate::app::AppError;
use crate::types::{
    ContentDepth, PlatformSearchOptions, PlatformSearchRequest, ScholarCitedByRequest,
    ScholarCitedBySort, ScholarSearchOptions,
};

const CITED: &str = "scholar:18208131694456651388";
const SEARCH_CURSOR: &str = "v1.serpapi.eyJxdWVyeSI6Im1vbWVudHVtIiwibGltaXQiOjIwLCJvcHRpb25zIjp7InBsYXRmb3JtIjoic2Nob2xhciJ9LCJwYWdlIjoiMjAifQ";
const CITED_BY_CURSOR: &str =
    "v1.serpapi.eyJjaXRlZCI6MTgyMDgxMzE2OTQ0NTY2NTEzODgsImxpbWl0IjoyMCwicGFnZSI6IjIwIn0";

fn scholar(arguments: &[&str]) -> Result<ScholarCommand, clap::Error> {
    let command = std::iter::once("scholar").chain(arguments.iter().copied());
    match parse_platform(&command.collect::<Vec<_>>())? {
        PlatformCommand::Scholar { command } => Ok(command),
        _ => unreachable!("the arguments name scholar"),
    }
}

fn search_args(arguments: &[&str]) -> Result<ScholarSearchArgs, clap::Error> {
    let search = std::iter::once("search").chain(arguments.iter().copied());
    match scholar(&search.collect::<Vec<_>>())? {
        ScholarCommand::Search(arguments) => Ok(arguments),
        _ => unreachable!("the arguments name scholar search"),
    }
}

fn cited_by_args(arguments: &[&str]) -> Result<ScholarCitedByArgs, clap::Error> {
    let cited_by = std::iter::once("cited-by").chain(arguments.iter().copied());
    match scholar(&cited_by.collect::<Vec<_>>())? {
        ScholarCommand::CitedBy(arguments) => Ok(arguments),
        _ => unreachable!("the arguments name scholar cited-by"),
    }
}

fn fetch_args(arguments: &[&str]) -> Result<ScholarFetchArgs, clap::Error> {
    let fetch = std::iter::once("fetch").chain(arguments.iter().copied());
    match scholar(&fetch.collect::<Vec<_>>())? {
        ScholarCommand::Fetch(arguments) => Ok(arguments),
        _ => unreachable!("the arguments name scholar fetch"),
    }
}

/// The first page a search asks for, or the argument error its checks raise.
fn search_request(arguments: &[&str]) -> Result<PlatformSearchRequest, AppError> {
    let Ok(parsed) = search_args(arguments) else {
        panic!("the parser accepts a scholar search of {arguments:?}");
    };
    match scholar_search_input(parsed)?.request {
        PageInput::Request(request) => Ok(request),
        PageInput::Cursor(_) => panic!("the arguments name no cursor"),
    }
}

/// The first page of citing works a command asks for, or the argument error its checks raise.
fn cited_by_request(arguments: &[&str]) -> Result<ScholarCitedByRequest, AppError> {
    let Ok(parsed) = cited_by_args(arguments) else {
        panic!("the parser accepts a scholar cited-by of {arguments:?}");
    };
    match cited_by_input(parsed)?.request {
        PageInput::Request(request) => Ok(request),
        PageInput::Cursor(_) => panic!("the arguments name no cursor"),
    }
}

fn argument_error<T: std::fmt::Debug>(result: Result<T, AppError>) -> String {
    match result {
        Err(AppError::Argument(message)) => message,
        other => panic!("expected an argument error, got {other:?}"),
    }
}

// Search.

#[test]
fn a_search_asks_for_a_first_page_of_twenty_results_by_default() {
    let request = search_request(&["momentum"]).expect("a request");

    assert_eq!(
        request,
        PlatformSearchRequest {
            query: "momentum".to_owned(),
            limit: 20,
            options: PlatformSearchOptions::Scholar(ScholarSearchOptions::default()),
            page: None,
        }
    );
}

#[test]
fn page_sizes_and_years_outside_their_definition_are_refused_while_parsing() {
    for (arguments, flag) in [
        (&["momentum", "--limit", "0"][..], "--limit"),
        (&["momentum", "--limit", "21"], "--limit"),
        (&["momentum", "--year-from", "999"], "--year-from"),
        (&["momentum", "--year-to", "10000"], "--year-to"),
    ] {
        let error = search_args(arguments).expect_err("the parser refuses the value");

        assert_eq!(
            (error.kind(), error.to_string().contains(flag)),
            (ErrorKind::ValueValidation, true),
            "arguments: {arguments:?}\n{error}"
        );
    }
}

#[test]
fn a_blank_query_or_reversed_years_are_argument_errors_before_any_request() {
    for (arguments, message) in [
        (&["  "][..], "scholar search needs a query"),
        (
            &["momentum", "--year-from", "2025", "--year-to", "2024"],
            "--year-from must not be later than --year-to",
        ),
    ] {
        let found = argument_error(search_request(arguments));

        assert!(
            found.contains(message),
            "arguments: {arguments:?}\nmessage: {found}"
        );
    }
}

#[test]
fn a_cursor_conflicts_with_an_explicit_query_option_or_limit() {
    for extra in [
        &["momentum"][..],
        &["--limit", "20"],
        &["--year-from", "2020"],
        &["--year-to", "2024"],
        &["--review-only"],
    ] {
        let mut arguments = vec!["--cursor", SEARCH_CURSOR];
        arguments.extend_from_slice(extra);

        let error = search_args(&arguments).expect_err("the parser refuses the combination");

        assert_eq!(
            error.kind(),
            ErrorKind::ArgumentConflict,
            "arguments: {arguments:?}\n{error}"
        );
    }
}

// Cited-by.

#[test]
fn a_cited_by_page_asks_for_twenty_results_in_relevance_order_by_default() {
    let request = cited_by_request(&[CITED]).expect("a request");

    assert_eq!(
        (
            request.cited.to_string(),
            request.query,
            request.limit,
            (request.year_from, request.year_to),
            request.sort,
            request.page
        ),
        (
            "18208131694456651388".to_owned(),
            None,
            20,
            (None, None),
            ScholarCitedBySort::Relevance,
            None
        )
    );
}

#[test]
fn a_missing_target_or_a_page_size_outside_one_to_twenty_is_refused_while_parsing() {
    for (arguments, kind) in [
        (&[][..], ErrorKind::MissingRequiredArgument),
        (&[CITED, "--limit", "0"], ErrorKind::ValueValidation),
        (&[CITED, "--limit", "21"], ErrorKind::ValueValidation),
    ] {
        let error = cited_by_args(arguments).expect_err("the parser refuses the arguments");

        assert_eq!(error.kind(), kind, "arguments: {arguments:?}\n{error}");
    }
}

#[test]
fn cited_by_targets_and_options_google_scholar_cannot_run_are_argument_errors() {
    for (arguments, message) in [
        (
            &["https://scholar.google.com/scholar?cites=18208131694456651388"][..],
            "unrecognized scholar reference",
        ),
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
        let found = argument_error(cited_by_request(arguments));

        assert!(
            found.contains(message),
            "arguments: {arguments:?}\nmessage: {found}"
        );
    }
}

#[test]
fn a_cited_by_cursor_conflicts_with_a_target_query_option_or_limit() {
    for extra in [
        &[CITED][..],
        &["--query", "crash"],
        &["--limit", "20"],
        &["--year-from", "2020"],
        &["--year-to", "2024"],
        &["--sort", "date"],
    ] {
        let mut arguments = vec!["--cursor", CITED_BY_CURSOR];
        arguments.extend_from_slice(extra);

        let error = cited_by_args(&arguments).expect_err("the parser refuses the combination");

        assert_eq!(
            error.kind(),
            ErrorKind::ArgumentConflict,
            "arguments: {arguments:?}\n{error}"
        );
    }
}

// Fetch.

#[test]
fn a_cluster_ref_or_url_fetches_the_cluster_it_names_at_the_depth_asked() {
    for (arguments, depth) in [
        (&[CITED][..], ContentDepth::Metadata),
        (
            &[
                "https://scholar.google.com/scholar?cluster=18208131694456651388&hl=en&num=20&as_sdt=0,27",
            ],
            ContentDepth::Metadata,
        ),
        (&[CITED, "--depth", "abstract"], ContentDepth::Abstract),
    ] {
        let parsed = fetch_args(arguments).expect("the parser accepts the arguments");

        let request = scholar_fetch_input(parsed).expect("a request").request;

        assert_eq!(
            (request.reference.to_string(), request.depth),
            ("scholar:18208131694456651388".to_owned(), depth),
            "arguments: {arguments:?}"
        );
    }
}

#[test]
fn urls_that_name_no_single_cluster_are_argument_errors_before_any_request() {
    for input in [
        "https://scholar.google.com/scholar?cites=18208131694456651388",
        "https://scholar.google.com/scholar?cluster=18208131694456651388&cites=18208131694456651388",
        "https://scholar.google.com/scholar?cluster=1&cluster=2",
        "https://scholar.google.com/scholar?cluster=18446744073709551616",
        "https://scholar.google.com/citations?user=zz9fakeAAAAJ&hl=en",
    ] {
        let parsed = fetch_args(&[input]).expect("the parser accepts any reference");

        let message = argument_error(scholar_fetch_input(parsed).map(|_| ()));

        assert!(
            message.contains("unrecognized scholar reference"),
            "{input}: {message}"
        );
    }
}
