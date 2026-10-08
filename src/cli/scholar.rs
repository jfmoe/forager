//! Google Scholar arguments and request construction.

use clap::{Args, Subcommand, ValueEnum};

use super::{PageInput, PlatformCommonArgs, fetch, scholar_cited_by, search};
use crate::app::args::{DocsOutputFormat, OutputFormat};
use crate::app::dispatch::{AppError, CommandOutput};
use crate::types::{
    ContentDepth, Platform, PlatformFetchRequest, PlatformRef, PlatformSearchOptions,
    PlatformSearchRequest, ScholarCitedByRequest, ScholarCitedBySort, ScholarRef,
    ScholarSearchOptions,
};

#[derive(Debug, Subcommand)]
pub(in crate::app) enum ScholarCommand {
    /// Search Google Scholar; each result carries its cluster ref, byline, citation count, and
    /// links, with a snippet when Google Scholar shows one.
    Search(ScholarSearchArgs),
    /// List the versions Google Scholar groups under one paper; metadata only.
    Fetch(ScholarFetchArgs),
    /// List the works Google Scholar counts as citing one paper, as search results. An unknown
    /// paper and an uncited one both list nothing.
    CitedBy(ScholarCitedByArgs),
}

#[derive(Debug, Args)]
pub(in crate::app) struct ScholarSearchArgs {
    /// Google Scholar query, sent unchanged; its own operators such as "phrase", OR, -term,
    /// author:, and source: apply.
    #[arg(conflicts_with = "cursor")]
    query: Option<String>,
    /// Maximum results on this page; every page costs one search whatever its size.
    #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u16).range(1..=20), conflicts_with = "cursor")]
    limit: u16,
    /// First included publication year.
    #[arg(long, value_name = "YYYY", value_parser = clap::value_parser!(u16).range(1000..=9999), conflicts_with = "cursor")]
    year_from: Option<u16>,
    /// Last included publication year.
    #[arg(long, value_name = "YYYY", value_parser = clap::value_parser!(u16).range(1000..=9999), conflicts_with = "cursor")]
    year_to: Option<u16>,
    /// Only review articles.
    #[arg(long, conflicts_with = "cursor")]
    review_only: bool,
    /// Opaque `next_cursor` from a previous page; it restores the complete original request.
    #[arg(long)]
    cursor: Option<String>,
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    format: OutputFormat,
    #[command(flatten)]
    common: PlatformCommonArgs,
}

#[derive(Debug, Args)]
pub(in crate::app) struct ScholarFetchArgs {
    /// A `scholar:<cluster_id>` ref, or a scholar.google.com/scholar?cluster=<id> URL.
    reference: String,
    /// Google Scholar provides `metadata` only; other depths are rejected before any request.
    #[arg(long, value_enum, default_value_t = ScholarDepthArg::Metadata)]
    depth: ScholarDepthArg,
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    format: OutputFormat,
    #[command(flatten)]
    common: PlatformCommonArgs,
}

#[derive(Debug, Args)]
pub(in crate::app) struct ScholarCitedByArgs {
    /// The cited paper: a `scholar:<cluster_id>` ref, or a scholar.google.com/scholar?cluster=<id>
    /// URL.
    #[arg(required_unless_present = "cursor", conflicts_with = "cursor")]
    reference: Option<String>,
    /// Only citing works that match this Google Scholar query; its own operators apply.
    #[arg(long, conflicts_with = "cursor")]
    query: Option<String>,
    /// Maximum results on this page; every page costs one search whatever its size.
    #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u16).range(1..=20), conflicts_with = "cursor")]
    limit: u16,
    /// First included publication year.
    #[arg(long, value_name = "YYYY", value_parser = clap::value_parser!(u16).range(1000..=9999), conflicts_with = "cursor")]
    year_from: Option<u16>,
    /// Last included publication year.
    #[arg(long, value_name = "YYYY", value_parser = clap::value_parser!(u16).range(1000..=9999), conflicts_with = "cursor")]
    year_to: Option<u16>,
    /// `relevance` keeps Google Scholar's order; `date` lists the most recently indexed citing
    /// works first and takes no year range.
    #[arg(long, value_enum, default_value_t = ScholarCitedBySortArg::Relevance, conflicts_with = "cursor")]
    sort: ScholarCitedBySortArg,
    /// Opaque `next_cursor` from a previous cited-by page; it restores the complete original
    /// request.
    #[arg(long)]
    cursor: Option<String>,
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    format: OutputFormat,
    #[command(flatten)]
    common: PlatformCommonArgs,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ScholarCitedBySortArg {
    Relevance,
    Date,
}

impl From<ScholarCitedBySortArg> for ScholarCitedBySort {
    fn from(value: ScholarCitedBySortArg) -> Self {
        match value {
            ScholarCitedBySortArg::Relevance => Self::Relevance,
            ScholarCitedBySortArg::Date => Self::Date,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ScholarDepthArg {
    Metadata,
    Snippet,
    Abstract,
    #[value(name = "full_text")]
    FullText,
}

impl From<ScholarDepthArg> for ContentDepth {
    fn from(value: ScholarDepthArg) -> Self {
        match value {
            ScholarDepthArg::Metadata => Self::Metadata,
            ScholarDepthArg::Snippet => Self::Snippet,
            ScholarDepthArg::Abstract => Self::Abstract,
            ScholarDepthArg::FullText => Self::FullText,
        }
    }
}

pub(in crate::app) fn run(command: ScholarCommand) -> Result<CommandOutput, AppError> {
    match command {
        ScholarCommand::Search(arguments) => scholar_search(arguments),
        ScholarCommand::Fetch(arguments) => scholar_fetch(arguments),
        ScholarCommand::CitedBy(arguments) => cited_by(arguments),
    }
}

fn scholar_search(arguments: ScholarSearchArgs) -> Result<CommandOutput, AppError> {
    let ScholarSearchArgs {
        query,
        limit,
        year_from,
        year_to,
        review_only,
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
            options: PlatformSearchOptions::Scholar(ScholarSearchOptions {
                year_from,
                year_to,
                review_only,
            }),
            page: None,
        };
        request.validate().map_err(AppError::Argument)?;
        PageInput::Request(request)
    };
    search(Platform::Scholar, input, format, &common)
}

fn cited_by(arguments: ScholarCitedByArgs) -> Result<CommandOutput, AppError> {
    let ScholarCitedByArgs {
        reference,
        query,
        limit,
        year_from,
        year_to,
        sort,
        cursor,
        format,
        common,
    } = arguments;
    let input = if let Some(cursor) = cursor {
        PageInput::Cursor(cursor)
    } else {
        let cited = ScholarRef::parse(&reference.unwrap_or_default())
            .map_err(|error| AppError::Argument(error.to_string()))?;
        let request = ScholarCitedByRequest {
            cited,
            query,
            limit,
            year_from,
            year_to,
            sort: sort.into(),
            page: None,
        };
        request.validate().map_err(AppError::Argument)?;
        PageInput::Request(request)
    };
    scholar_cited_by(input, format, &common)
}

fn scholar_fetch(arguments: ScholarFetchArgs) -> Result<CommandOutput, AppError> {
    let ScholarFetchArgs {
        reference,
        depth,
        format,
        common,
    } = arguments;
    let reference = PlatformRef::parse(Platform::Scholar, &reference)
        .map_err(|error| AppError::Argument(error.to_string()))?;
    let request = PlatformFetchRequest {
        reference,
        depth: depth.into(),
    };
    let format = match format {
        OutputFormat::Json => DocsOutputFormat::Json,
        OutputFormat::Markdown => DocsOutputFormat::Markdown,
    };
    fetch(Platform::Scholar, request, format, None, false, &common)
}
