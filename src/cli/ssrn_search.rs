//! SSRN search arguments and request construction.

use chrono::NaiveDate;
use clap::{Args, ValueEnum};

use super::{PageInput, PlatformCommonArgs, parse_date, search};
use crate::app::args::OutputFormat;
use crate::app::dispatch::{AppError, CommandOutput};
use crate::types::{Platform, PlatformSearchOptions, PlatformSearchRequest, SsrnSearchOptions};
use crate::types::{SsrnDateRange, SsrnSearchScope, SsrnSort, SsrnSortOrder, SsrnWorkType};
use clap::builder::{PossibleValuesParser, TypedValueParser};

#[derive(Debug, Args)]
pub(in crate::app) struct SsrnSearchArgs {
    /// Query expression.
    #[arg(conflicts_with = "cursor")]
    query: Option<String>,
    /// Search fields: all, title, or Crossref bibliographic fields.
    #[arg(long, value_enum, default_value_t = ScopeArg::All, conflicts_with = "cursor", help_heading = "Query")]
    scope: ScopeArg,
    /// Author text query, not an author ID.
    #[arg(long, conflicts_with = "cursor", help_heading = "Query")]
    author: Option<String>,
    /// Author affiliation query.
    #[arg(long, conflicts_with = "cursor", help_heading = "Query")]
    affiliation: Option<String>,
    /// First included publication day.
    #[arg(long, value_name = "YYYY-MM-DD", value_parser = parse_date, conflicts_with = "cursor", help_heading = "Dates")]
    published_from: Option<NaiveDate>,
    /// Last included publication day.
    #[arg(long, value_name = "YYYY-MM-DD", value_parser = parse_date, conflicts_with = "cursor", help_heading = "Dates")]
    published_to: Option<NaiveDate>,
    /// First included crossref first registration day.
    #[arg(long, value_name = "YYYY-MM-DD", value_parser = parse_date, conflicts_with = "cursor", help_heading = "Dates")]
    created_from: Option<NaiveDate>,
    /// Last included crossref first registration day.
    #[arg(long, value_name = "YYYY-MM-DD", value_parser = parse_date, conflicts_with = "cursor", help_heading = "Dates")]
    created_to: Option<NaiveDate>,
    /// First included crossref metadata deposit/update day.
    #[arg(long, value_name = "YYYY-MM-DD", value_parser = parse_date, conflicts_with = "cursor", help_heading = "Dates")]
    updated_from: Option<NaiveDate>,
    /// Last included crossref metadata deposit/update day.
    #[arg(long, value_name = "YYYY-MM-DD", value_parser = parse_date, conflicts_with = "cursor", help_heading = "Dates")]
    updated_to: Option<NaiveDate>,
    /// Require an upstream abstract field; this does not fetch or guarantee usable text.
    #[arg(long, conflicts_with = "cursor", help_heading = "Metadata")]
    has_abstract: bool,
    /// Require this registered work type; no type filter by default.
    #[arg(long = "type", value_parser = PossibleValuesParser::new(SsrnWorkType::NAMES).try_map(|value| value.parse::<SsrnWorkType>()), conflicts_with = "cursor", help_heading = "Metadata")]
    work_type: Option<SsrnWorkType>,
    /// Contributor ORCID (bare ID); records without this deposited ID are excluded.
    #[arg(long, conflicts_with = "cursor", help_heading = "Metadata")]
    orcid: Option<String>,
    /// Funder DOI (10.13039/<digits>); records without funding metadata are excluded.
    #[arg(long, conflicts_with = "cursor", help_heading = "Metadata")]
    funder: Option<String>,
    /// Ranking metric.
    #[arg(long, value_enum, default_value_t = SortArg::Relevance, conflicts_with = "cursor", help_heading = "Sorting")]
    sort: SortArg,
    /// Ranking direction.
    #[arg(long, value_enum, default_value_t = OrderArg::Desc, conflicts_with = "cursor", help_heading = "Sorting")]
    order: OrderArg,
    /// Maximum results on this page.
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u16).range(1..=100), conflicts_with = "cursor")]
    limit: u16,
    /// Opaque `next_cursor` from a previous page; it restores the complete original request.
    #[arg(long)]
    cursor: Option<String>,
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    format: OutputFormat,
    #[command(flatten)]
    common: PlatformCommonArgs,
}

pub(super) fn ssrn_search(arguments: SsrnSearchArgs) -> Result<CommandOutput, AppError> {
    let (input, format, common) = ssrn_search_input(arguments)?;
    search(Platform::Ssrn, input, format, &common)
}

/// Builds the page a search asks for and checks its criteria before any request.
fn ssrn_search_input(
    arguments: SsrnSearchArgs,
) -> Result<
    (
        PageInput<PlatformSearchRequest>,
        OutputFormat,
        PlatformCommonArgs,
    ),
    AppError,
> {
    let SsrnSearchArgs {
        query,
        scope,
        author,
        affiliation,
        published_from,
        published_to,
        created_from,
        created_to,
        updated_from,
        updated_to,
        has_abstract,
        work_type,
        orcid,
        funder,
        sort,
        order,
        limit,
        cursor,
        format,
        common,
    } = arguments;
    let input = if let Some(cursor) = cursor {
        PageInput::Cursor(cursor)
    } else {
        let request = PlatformSearchRequest {
            query: query.unwrap_or_default(),
            limit,
            options: PlatformSearchOptions::Ssrn(SsrnSearchOptions {
                scope: scope.into(),
                author,
                affiliation,
                published: SsrnDateRange {
                    from: published_from,
                    to: published_to,
                },
                created: SsrnDateRange {
                    from: created_from,
                    to: created_to,
                },
                updated: SsrnDateRange {
                    from: updated_from,
                    to: updated_to,
                },
                has_abstract,
                work_type,
                orcid,
                funder,
                sort: sort.into(),
                order: order.into(),
            }),
            page: None,
        };
        request.validate().map_err(AppError::Argument)?;
        PageInput::Request(request)
    };
    Ok((input, format, common))
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ScopeArg {
    All,
    Title,
    Bibliographic,
}

impl From<ScopeArg> for SsrnSearchScope {
    fn from(value: ScopeArg) -> Self {
        match value {
            ScopeArg::All => Self::All,
            ScopeArg::Title => Self::Title,
            ScopeArg::Bibliographic => Self::Bibliographic,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum SortArg {
    Relevance,
    Published,
    Created,
    Updated,
    Citations,
}

impl From<SortArg> for SsrnSort {
    fn from(value: SortArg) -> Self {
        match value {
            SortArg::Relevance => Self::Relevance,
            SortArg::Published => Self::Published,
            SortArg::Created => Self::Created,
            SortArg::Updated => Self::Updated,
            SortArg::Citations => Self::Citations,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OrderArg {
    Asc,
    Desc,
}

impl From<OrderArg> for SsrnSortOrder {
    fn from(value: OrderArg) -> Self {
        match value {
            OrderArg::Asc => Self::Asc,
            OrderArg::Desc => Self::Desc,
        }
    }
}

/// The SSRN search command's argument rules, proven by parsing its definition in process.
#[cfg(test)]
mod tests {
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
}
