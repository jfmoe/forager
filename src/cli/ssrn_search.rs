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
    search(Platform::Ssrn, input, format, &common)
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
