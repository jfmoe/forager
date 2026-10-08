//! Google Scholar arguments and request construction.

use clap::{Args, Subcommand, ValueEnum};

use super::{PlatformCommonArgs, SearchInput, fetch, search};
use crate::app::args::{DocsOutputFormat, OutputFormat};
use crate::app::dispatch::{AppError, CommandOutput};
use crate::types::{
    ContentDepth, Platform, PlatformFetchRequest, PlatformRef, PlatformSearchOptions,
    PlatformSearchRequest, ScholarSearchOptions,
};

#[derive(Debug, Subcommand)]
pub(in crate::app) enum ScholarCommand {
    /// Search Google Scholar; each result carries its cluster ref, byline, citation count, and
    /// links, with a snippet when Google Scholar shows one.
    Search(ScholarSearchArgs),
    /// List the versions Google Scholar groups under one paper; metadata only.
    Fetch(ScholarFetchArgs),
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
        SearchInput::Cursor(cursor)
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
        SearchInput::Request(request)
    };
    search(Platform::Scholar, input, format, &common)
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
