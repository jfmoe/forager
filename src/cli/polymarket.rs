//! Polymarket arguments and request construction.

use std::time::Duration;

use chrono::NaiveDate;
use clap::{Args, Subcommand, ValueEnum};

use super::{PageInput, PlatformCommonArgs, fetch, parse_date, preflight_error, search};
use crate::app::args::{DocsOutputFormat, OutputFormat};
use crate::app::dispatch::{AppError, CommandOutput, NetworkDependencies, provider_attempt_log};
use crate::platform_history;
use crate::types::{
    ContentDepth, Deadline, Platform, PlatformFetchRequest, PlatformRef, PlatformSearchOptions,
    PlatformSearchRequest, PolymarketBucket, PolymarketHistoryRequest, PolymarketInterval,
    PolymarketRef, PolymarketSearchOptions, PolymarketWindow,
};

#[derive(Debug, Subcommand)]
pub(in crate::app) enum PolymarketCommand {
    /// Search Polymarket events by topic; each result carries its most probable markets, priced
    /// from Gamma's cache, which may lag up to 5 minutes.
    Search(PolymarketSearchArgs),
    /// Fetch one event or market with the live probability of every outcome: the order-book
    /// midpoint when both sides have orders, or the payout once it resolved.
    Fetch(PolymarketFetchArgs),
    /// List the price history of one outcome of a market, oldest first; a resolved market's
    /// payout is the separate `settlement`.
    History(PolymarketHistoryArgs),
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

#[derive(Debug, Args)]
pub(in crate::app) struct PolymarketHistoryArgs {
    /// A `polymarket:market/<market_slug>` ref or polymarket.com market URL; an event ref or URL
    /// works when the event holds one market.
    market: String,
    /// The outcome by name, in any letter case; by default the first outcome (Yes).
    #[arg(long)]
    outcome: Option<String>,
    /// A window that ends now: a day, a week, a month, or the market's whole life. By default
    /// `1m`.
    #[arg(long, value_enum, conflicts_with_all = ["from", "to"])]
    interval: Option<PolymarketIntervalArg>,
    /// First UTC day of an explicit window, included; the window spans at most 15 days.
    #[arg(long, value_name = "YYYY-MM-DD", value_parser = parse_date, requires = "to")]
    from: Option<NaiveDate>,
    /// Last UTC day of an explicit window, included.
    #[arg(long, value_name = "YYYY-MM-DD", value_parser = parse_date, requires = "from")]
    to: Option<NaiveDate>,
    /// The width of one point (`1m` is a minute); by default the window keeps the series to
    /// tens or a few hundred points.
    #[arg(long, value_enum)]
    bucket: Option<PolymarketBucketArg>,
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    format: OutputFormat,
    #[command(flatten)]
    common: PlatformCommonArgs,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum PolymarketIntervalArg {
    #[value(name = "1d")]
    Day,
    #[value(name = "1w")]
    Week,
    #[value(name = "1m")]
    Month,
    Max,
}

impl From<PolymarketIntervalArg> for PolymarketInterval {
    fn from(value: PolymarketIntervalArg) -> Self {
        match value {
            PolymarketIntervalArg::Day => Self::Day,
            PolymarketIntervalArg::Week => Self::Week,
            PolymarketIntervalArg::Month => Self::Month,
            PolymarketIntervalArg::Max => Self::Max,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum PolymarketBucketArg {
    #[value(name = "1m")]
    OneMinute,
    #[value(name = "5m")]
    FiveMinutes,
    #[value(name = "30m")]
    ThirtyMinutes,
    #[value(name = "1h")]
    OneHour,
    #[value(name = "3h")]
    ThreeHours,
    #[value(name = "12h")]
    TwelveHours,
    #[value(name = "1d")]
    OneDay,
}

impl From<PolymarketBucketArg> for PolymarketBucket {
    fn from(value: PolymarketBucketArg) -> Self {
        match value {
            PolymarketBucketArg::OneMinute => Self::OneMinute,
            PolymarketBucketArg::FiveMinutes => Self::FiveMinutes,
            PolymarketBucketArg::ThirtyMinutes => Self::ThirtyMinutes,
            PolymarketBucketArg::OneHour => Self::OneHour,
            PolymarketBucketArg::ThreeHours => Self::ThreeHours,
            PolymarketBucketArg::TwelveHours => Self::TwelveHours,
            PolymarketBucketArg::OneDay => Self::OneDay,
        }
    }
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
        PolymarketCommand::History(arguments) => polymarket_history(arguments),
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

fn polymarket_history(arguments: PolymarketHistoryArgs) -> Result<CommandOutput, AppError> {
    let PolymarketHistoryArgs {
        market,
        outcome,
        interval,
        from,
        to,
        bucket,
        format,
        common,
    } = arguments;
    let market =
        PolymarketRef::parse(&market).map_err(|error| AppError::Argument(error.to_string()))?;
    let window = match (from, to) {
        (Some(from), Some(to)) => PolymarketWindow::Range { from, to },
        _ => PolymarketWindow::Interval(interval.map_or(PolymarketInterval::Month, Into::into)),
    };
    let request = PolymarketHistoryRequest::new(market, outcome, window, bucket.map(Into::into))
        .map_err(AppError::Argument)?;
    let dependencies = NetworkDependencies::load()?;
    let plan = platform_history::plan_history(
        dependencies.config.platforms.get(Platform::Polymarket),
        request,
    )
    .map_err(preflight_error)?;
    let result = dependencies.runtime.block_on(platform_history::run_history(
        plan,
        dependencies.client,
        dependencies.retry_policy,
        Deadline::new(Duration::from_secs(common.timeout)),
        common.verbose,
    ));
    let attempt_log = provider_attempt_log(dependencies.config.log_level, &result, |history| {
        &history.attempts
    });
    Ok(CommandOutput::PolymarketHistory {
        result,
        format,
        output: common.output.target(),
        attempt_log,
    })
}
