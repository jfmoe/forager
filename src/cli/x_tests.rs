use chrono::NaiveDate;
use clap::error::ErrorKind;

use super::super::{PlatformCommand, parse_platform};
use super::{
    XAskArgs, XCommand, XFetchArgs, XSearchArgs, x_ask_input, x_fetch_input, x_search_input,
};
use crate::app::AppError;
use crate::types::{
    ContentDepth, PlatformSearchOptions, PlatformSearchRequest, XAskRequest, XSearchOptions,
};

fn x(arguments: &[&str]) -> Result<XCommand, clap::Error> {
    let command = std::iter::once("x").chain(arguments.iter().copied());
    match parse_platform(&command.collect::<Vec<_>>())? {
        PlatformCommand::X { command } => Ok(command),
        _ => unreachable!("the arguments name x"),
    }
}

fn search_args(arguments: &[&str]) -> Result<XSearchArgs, clap::Error> {
    let search = std::iter::once("search").chain(arguments.iter().copied());
    match x(&search.collect::<Vec<_>>())? {
        XCommand::Search(arguments) => Ok(arguments),
        _ => unreachable!("the arguments name x search"),
    }
}

fn fetch_args(arguments: &[&str]) -> Result<XFetchArgs, clap::Error> {
    let fetch = std::iter::once("fetch").chain(arguments.iter().copied());
    match x(&fetch.collect::<Vec<_>>())? {
        XCommand::Fetch(arguments) => Ok(arguments),
        _ => unreachable!("the arguments name x fetch"),
    }
}

fn ask_args(arguments: &[&str]) -> Result<XAskArgs, clap::Error> {
    let ask = std::iter::once("ask").chain(arguments.iter().copied());
    match x(&ask.collect::<Vec<_>>())? {
        XCommand::Ask(arguments) => Ok(arguments),
        _ => unreachable!("the arguments name x ask"),
    }
}

/// The search a command asks for, or the argument error its checks raise.
fn search_request(arguments: &[&str]) -> Result<PlatformSearchRequest, AppError> {
    let Ok(parsed) = search_args(arguments) else {
        panic!("the parser accepts an x search of {arguments:?}");
    };
    x_search_input(parsed).map(|(request, _, _)| request)
}

/// The question a command asks, or the argument error its checks raise.
fn ask_request(arguments: &[&str]) -> Result<XAskRequest, AppError> {
    let Ok(parsed) = ask_args(arguments) else {
        panic!("the parser accepts an x ask of {arguments:?}");
    };
    x_ask_input(parsed).map(|(request, _, _)| request)
}

fn argument_error<T: std::fmt::Debug>(result: Result<T, AppError>) -> String {
    match result {
        Err(AppError::Argument(message)) => message,
        other => panic!("expected an argument error, got {other:?}"),
    }
}

fn day(value: &str) -> NaiveDate {
    value.parse().expect("a calendar day")
}

fn handles(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

// Search.

#[test]
fn a_search_carries_its_handles_days_and_limit_into_the_request() {
    let request = search_request(&[
        "Rust 2027 edition",
        "--handle",
        "@rustlang",
        "--handle",
        "RustLang",
        "--handle",
        "nikomatsakis",
        "--from",
        "2026-09-01",
        "--to",
        "2026-10-15",
        "--limit",
        "5",
    ])
    .expect("a request");

    assert_eq!(
        request,
        PlatformSearchRequest {
            query: "Rust 2027 edition".to_owned(),
            limit: 5,
            options: PlatformSearchOptions::X(XSearchOptions::new(
                handles(&["rustlang", "nikomatsakis"]),
                Vec::new(),
                Some(day("2026-09-01")),
                Some(day("2026-10-15")),
            )),
            page: None,
        }
    );
}

#[test]
fn a_search_asks_for_ten_posts_without_filters_by_default() {
    let request = search_request(&["rust"]).expect("a request");

    assert_eq!(
        request,
        PlatformSearchRequest {
            query: "rust".to_owned(),
            limit: 10,
            options: PlatformSearchOptions::X(XSearchOptions::default()),
            page: None,
        }
    );
}

#[test]
fn a_malformed_day_or_a_limit_outside_one_to_twenty_is_refused_while_parsing() {
    for (arguments, message) in [
        (&["rust", "--from", "2026-13-01"][..], "not a calendar date"),
        (&["rust", "--limit", "0"], "--limit"),
        (&["rust", "--limit", "21"], "--limit"),
    ] {
        let error = search_args(arguments).expect_err("the parser refuses the value");

        assert_eq!(
            (error.kind(), error.to_string().contains(message)),
            (ErrorKind::ValueValidation, true),
            "arguments: {arguments:?}\n{error}"
        );
    }
}

#[test]
fn invalid_search_filters_or_a_blank_query_are_argument_errors_before_any_request() {
    for (arguments, message) in [
        (
            &["rust", "--handle", "a", "--exclude-handle", "b"][..],
            "--handle cannot be combined with --exclude-handle",
        ),
        (
            &["rust", "--handle", "space-x"],
            "invalid X handle `space-x`",
        ),
        (
            &["rust", "--from", "2026-10-02", "--to", "2026-10-01"],
            "--from must not be later than --to",
        ),
        (&["  "], "x search needs a query"),
    ] {
        let found = argument_error(search_request(arguments));

        assert!(
            found.contains(message),
            "arguments: {arguments:?}\nmessage: {found}"
        );
    }
}

// Fetch.

#[test]
fn a_post_url_or_ref_is_fetched_at_thread_depth_unless_another_depth_is_given() {
    for (arguments, depth) in [
        (
            &["https://twitter.com/SpaceX/status/2110126570233094527/photo/1?s=20"][..],
            ContentDepth::Thread,
        ),
        (
            &["x:2110126570233094527", "--depth", "metadata"],
            ContentDepth::Metadata,
        ),
        (
            &["x:2110126570233094527", "--depth", "snippet"],
            ContentDepth::Snippet,
        ),
        (
            &["x:2110126570233094527", "--depth", "full_text"],
            ContentDepth::FullText,
        ),
    ] {
        let parsed = fetch_args(arguments).expect("the parser accepts the arguments");

        let (request, _, _) = x_fetch_input(parsed).expect("a request");

        assert_eq!(
            (request.reference.to_string(), request.depth),
            ("x:2110126570233094527".to_owned(), depth),
            "arguments: {arguments:?}"
        );
    }
}

#[test]
fn short_links_profiles_and_bare_ids_are_argument_errors_before_any_request() {
    for (input, message) in [
        ("https://t.co/AbCdEf", "t.co short links"),
        ("https://x.com/SpaceX", "unrecognized x reference"),
        ("2110126570233094527", "unrecognized x reference"),
    ] {
        let parsed = fetch_args(&[input]).expect("the parser accepts any reference");

        let found = argument_error(x_fetch_input(parsed).map(|_| ()));

        assert!(found.contains(message), "{input}: {found}");
    }
}

#[test]
fn article_links_point_to_web_fetch_before_any_request() {
    for url in [
        "https://x.com/SpaceX/article/2110126570233094527",
        "https://x.com/i/article/2110126570233094527/",
        "https://x.com/SpaceX/status/2110126570233094527/article?s=20",
        "https://mobile.twitter.com/SpaceX/article/2110126570233094527#top",
    ] {
        let parsed = fetch_args(&[url]).expect("the parser accepts any reference");

        let found = argument_error(x_fetch_input(parsed).map(|_| ()));

        assert!(
            found.contains(&format!("forager fetch '{url}'")),
            "{url}: {found}"
        );
    }
}

// Ask.

#[test]
fn a_question_carries_its_handles_and_days_into_the_request() {
    let request = ask_request(&[
        "What did SpaceX say about Starship in early October?",
        "--handle",
        "SpaceX",
        "--from",
        "2026-10-01",
        "--to",
        "2026-10-14",
    ])
    .expect("a request");

    assert_eq!(
        (request.query.as_str(), request.options),
        (
            "What did SpaceX say about Starship in early October?",
            XSearchOptions::new(
                handles(&["SpaceX"]),
                Vec::new(),
                Some(day("2026-10-01")),
                Some(day("2026-10-14")),
            )
        )
    );
}

#[test]
fn a_day_that_is_not_on_the_calendar_is_refused_while_parsing_a_question() {
    let error = ask_args(&["q", "--to", "2026-02-30"]).expect_err("the parser refuses it");

    assert_eq!(
        (
            error.kind(),
            error.to_string().contains("not a calendar date")
        ),
        (ErrorKind::ValueValidation, true),
        "{error}"
    );
}

#[test]
fn invalid_question_filters_or_a_blank_question_are_argument_errors_before_any_request() {
    for (arguments, message) in [
        (
            &["q", "--handle", "a", "--exclude-handle", "b"][..],
            "--handle cannot be combined with --exclude-handle",
        ),
        (&[" "], "x ask needs a query"),
    ] {
        let found = argument_error(ask_request(arguments));

        assert!(
            found.contains(message),
            "arguments: {arguments:?}\nmessage: {found}"
        );
    }
}
