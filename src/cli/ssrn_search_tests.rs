use chrono::NaiveDate;
use clap::error::ErrorKind;

use super::super::{PageInput, PlatformCommand, SsrnCommand, parse_platform};
use super::{SsrnSearchArgs, ssrn_search_input};
use crate::app::AppError;
use crate::types::{
    PlatformSearchOptions, PlatformSearchRequest, SsrnDateRange, SsrnSearchOptions,
    SsrnSearchScope, SsrnSort, SsrnSortOrder, SsrnWorkType,
};

const CURSOR: &str = "v1.ssrn_crossref.eyJxdWVyeSI6Im1vbWVudHVtIiwibGltaXQiOjIsIm9wdGlvbnMiOnsicGxhdGZvcm0iOiJzc3JuIn0sInBhZ2UiOiIyIn0";

fn search_args(arguments: &[&str]) -> Result<SsrnSearchArgs, clap::Error> {
    let search = ["ssrn", "search"]
        .into_iter()
        .chain(arguments.iter().copied());
    match parse_platform(&search.collect::<Vec<_>>())? {
        PlatformCommand::Ssrn {
            command: SsrnCommand::Search(arguments),
        } => Ok(arguments),
        _ => unreachable!("the arguments name ssrn search"),
    }
}

/// The first page a search asks for, or the argument error its checks raise.
fn search_request(arguments: &[&str]) -> Result<PlatformSearchRequest, AppError> {
    let Ok(parsed) = search_args(arguments) else {
        panic!("the parser accepts an ssrn search of {arguments:?}");
    };
    match ssrn_search_input(parsed)?.0 {
        PageInput::Request(request) => Ok(request),
        PageInput::Cursor(_) => panic!("the arguments name no cursor"),
    }
}

fn day(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()
}

#[test]
fn search_criteria_become_one_request_and_defaults_fill_the_rest() {
    let filtered = search_request(&[
        "risk & return",
        "--scope",
        "bibliographic",
        "--author",
        "A B",
        "--affiliation",
        "Example University",
        "--published-from",
        "2020-01-01",
        "--published-to",
        "2024-12-31",
        "--created-from",
        "2019-01-01",
        "--created-to",
        "2025-01-01",
        "--updated-from",
        "2024-01-01",
        "--updated-to",
        "2025-12-31",
        "--has-abstract",
        "--type",
        "journal-article",
        "--orcid",
        "0000-0002-1825-0097",
        "--funder",
        "10.13039/100000001",
        "--sort",
        "citations",
        "--order",
        "asc",
        "--limit",
        "1",
    ]);
    let plain = search_request(&["dual momentum"]);

    assert_eq!(
        [filtered.ok(), plain.ok()],
        [
            Some(PlatformSearchRequest {
                query: "risk & return".to_owned(),
                limit: 1,
                options: PlatformSearchOptions::Ssrn(SsrnSearchOptions {
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
                }),
                page: None,
            }),
            Some(PlatformSearchRequest {
                query: "dual momentum".to_owned(),
                limit: 10,
                options: PlatformSearchOptions::Ssrn(SsrnSearchOptions {
                    scope: SsrnSearchScope::All,
                    sort: SsrnSort::Relevance,
                    order: SsrnSortOrder::Desc,
                    ..SsrnSearchOptions::default()
                }),
                page: None,
            }),
        ]
    );
}

#[test]
fn searches_the_criteria_cannot_express_are_argument_errors_before_any_request() {
    for (arguments, message) in [
        (&[][..], "ssrn search needs a query"),
        (&["   "], "ssrn search needs a query"),
        (&["momentum", "--author", " "], "must not be empty"),
        (
            &["momentum", "--orcid", "0000-0002-1825-0098"],
            "valid bare ORCID",
        ),
        (
            &[
                "momentum",
                "--created-from",
                "2025-01-01",
                "--created-to",
                "2024-01-01",
            ],
            "must not be after",
        ),
    ] {
        let result = search_request(arguments);

        assert!(
            matches!(&result, Err(AppError::Argument(found)) if found.contains(message)),
            "expected an argument error about {message:?}, got {result:?}"
        );
    }
}

#[test]
fn values_outside_their_definition_are_refused_while_parsing() {
    for (flag, value, message) in [
        ("--scope", "unknown", "invalid value"),
        ("--sort", "unknown", "invalid value"),
        ("--order", "up", "invalid value"),
        ("--type", "fiction", "invalid value"),
        ("--published-from", "2023-02-29", "calendar date"),
    ] {
        let error = search_args(&["momentum", flag, value])
            .map(|_| ())
            .expect_err("the parser refuses the value");

        assert!(
            error.to_string().contains(message),
            "{flag} {value}: {error}"
        );
    }
}

#[test]
fn a_cursor_conflicts_with_the_query_the_limit_and_every_criterion_even_at_its_default() {
    for criterion in [
        &["x"][..],
        &["--limit", "10"],
        &["--scope", "all"],
        &["--author", "A"],
        &["--affiliation", "U"],
        &["--published-from", "2024-01-01"],
        &["--published-to", "2024-01-01"],
        &["--created-from", "2024-01-01"],
        &["--created-to", "2024-01-01"],
        &["--updated-from", "2024-01-01"],
        &["--updated-to", "2024-01-01"],
        &["--has-abstract"],
        &["--type", "posted-content"],
        &["--orcid", "0000-0002-1825-0097"],
        &["--funder", "10.13039/100000001"],
        &["--sort", "relevance"],
        &["--order", "desc"],
    ] {
        let mut arguments = vec!["--cursor", CURSOR];
        arguments.extend_from_slice(criterion);

        let error = search_args(&arguments)
            .map(|_| ())
            .expect_err("the parser refuses the combination");

        assert_eq!(
            error.kind(),
            ErrorKind::ArgumentConflict,
            "arguments: {arguments:?}\n{error}"
        );
    }
}
