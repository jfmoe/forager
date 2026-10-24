//! Polymarket arguments and request construction.

use clap::{Args, Subcommand, ValueEnum};

use super::{PageInput, PlatformCommonArgs, fetch, search};
use crate::app::args::{DocsOutputFormat, OutputFormat};
use crate::app::dispatch::{AppError, CommandOutput};
use crate::types::{
    ContentDepth, Platform, PlatformFetchRequest, PlatformRef, PlatformSearchOptions,
    PlatformSearchRequest, PolymarketSearchOptions,
};

#[derive(Debug, Subcommand)]
pub(in crate::app) enum PolymarketCommand {
    /// Search Polymarket events by topic; each result carries its most probable markets, priced
    /// from Gamma's cache, which may lag up to 5 minutes.
    Search(PolymarketSearchArgs),
    /// Fetch one event or market with the live probability of every outcome: the order-book
    /// midpoint when both sides have orders, or the payout once it resolved.
    Fetch(PolymarketFetchArgs),
}

#[derive(Debug, Args)]
pub(in crate::app) struct PolymarketSearchArgs {
    /// Words to match, sent as written.
    #[arg(conflicts_with = "cursor")]
    query: Option<String>,
    /// Match closed events too; by default only open events match.
    #[arg(long, conflicts_with = "cursor")]
    include_closed: bool,
    /// Maximum events on this page.
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u16).range(1..=20), conflicts_with = "cursor")]
    limit: u16,
    /// Opaque `next_cursor` from a previous page; it restores the complete original request.
    #[arg(long)]
    cursor: Option<String>,
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    format: OutputFormat,
    #[command(flatten)]
    common: PlatformCommonArgs,
}

#[derive(Debug, Args)]
pub(in crate::app) struct PolymarketFetchArgs {
    /// A `polymarket:<event_slug>` or `polymarket:market/<market_slug>` ref, or a
    /// polymarket.com event or market URL.
    reference: String,
    /// `metadata` returns status and prices; `abstract` adds the resolution rules.
    #[arg(long, value_enum, default_value_t = PolymarketDepthArg::Metadata)]
    depth: PolymarketDepthArg,
    /// `content` prints the resolution rules of an `abstract` fetch.
    #[arg(long, value_enum, default_value_t = DocsOutputFormat::Json)]
    format: DocsOutputFormat,
    #[command(flatten)]
    common: PlatformCommonArgs,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum PolymarketDepthArg {
    Metadata,
    Snippet,
    Abstract,
    #[value(name = "full_text")]
    FullText,
    Thread,
}

impl From<PolymarketDepthArg> for ContentDepth {
    fn from(value: PolymarketDepthArg) -> Self {
        match value {
            PolymarketDepthArg::Metadata => Self::Metadata,
            PolymarketDepthArg::Snippet => Self::Snippet,
            PolymarketDepthArg::Abstract => Self::Abstract,
            PolymarketDepthArg::FullText => Self::FullText,
            PolymarketDepthArg::Thread => Self::Thread,
        }
    }
}

pub(in crate::app) fn run(command: PolymarketCommand) -> Result<CommandOutput, AppError> {
    match command {
        PolymarketCommand::Search(arguments) => polymarket_search(arguments),
        PolymarketCommand::Fetch(arguments) => polymarket_fetch(arguments),
    }
}

fn polymarket_search(arguments: PolymarketSearchArgs) -> Result<CommandOutput, AppError> {
    let PolymarketSearchArgs {
        query,
        include_closed,
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
            options: PlatformSearchOptions::Polymarket(PolymarketSearchOptions { include_closed }),
            page: None,
        };
        request.validate().map_err(AppError::Argument)?;
        PageInput::Request(request)
    };
    search(Platform::Polymarket, input, format, &common)
}

fn polymarket_fetch(arguments: PolymarketFetchArgs) -> Result<CommandOutput, AppError> {
    let PolymarketFetchArgs {
        reference,
        depth,
        format,
        common,
    } = arguments;
    let reference = PlatformRef::parse(Platform::Polymarket, &reference)
        .map_err(|error| AppError::Argument(error.to_string()))?;
    let request = PlatformFetchRequest::new(reference, depth.into());
    fetch(Platform::Polymarket, request, format, None, false, &common)
}
