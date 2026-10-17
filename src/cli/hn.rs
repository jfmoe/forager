//! Hacker News arguments and request construction.

use std::path::PathBuf;
use std::time::Duration;

use chrono::NaiveDate;
use clap::{Args, Subcommand, ValueEnum};

use super::{PageInput, PlatformCommonArgs, fetch, parse_date, preflight_error, search};
use crate::app::args::{DocsOutputFormat, OutputFormat};
use crate::app::dispatch::{AppError, CommandOutput, NetworkDependencies, provider_attempt_log};
use crate::platform_stories;
use crate::types::{
    ContentDepth, Deadline, HnRef, HnSearchOptions, HnSearchType, HnSort, HnStoriesList,
    HnStoriesRequest, Platform, PlatformFetchRequest, PlatformRef, PlatformSearchOptions,
    PlatformSearchRequest,
};

#[derive(Debug, Subcommand)]
pub(in crate::app) enum HnCommand {
    /// Search Hacker News stories, comments, and other items; each result carries its ref,
    /// points, comment count, and its own text when it has one.
    Search(HnSearchArgs),
    /// Fetch one Hacker News item; by default its whole discussion is written to a local
    /// Markdown file, top-level comments in the rank Hacker News shows.
    Fetch(HnFetchArgs),
    /// List the current items of an official Hacker News list in rank order.
    Stories(HnStoriesArgs),
}

#[derive(Debug, Args)]
pub(in crate::app) struct HnSearchArgs {
    /// Words to match; "double quotes" match a phrase and -word excludes a word.
    #[arg(conflicts_with = "cursor")]
    query: Option<String>,
    /// Result order.
    #[arg(long, value_enum, default_value_t = HnSortArg::Relevance, conflicts_with = "cursor")]
    sort: HnSortArg,
    /// Item type; repeat to match any of several.
    #[arg(
        long = "type",
        value_name = "TYPE",
        value_enum,
        conflicts_with = "cursor"
    )]
    types: Vec<HnTypeArg>,
    /// Only items by this Hacker News user.
    #[arg(long, value_name = "USER", conflicts_with = "cursor")]
    author: Option<String>,
    /// Only items in the discussion of this story ID.
    #[arg(long, value_name = "ID", value_parser = parse_item_id, conflicts_with = "cursor")]
    story: Option<u64>,
    /// First included publication day (UTC).
    #[arg(long, value_name = "YYYY-MM-DD", value_parser = parse_date, conflicts_with = "cursor")]
    from: Option<NaiveDate>,
    /// Last included publication day (UTC); the whole day is included.
    #[arg(long, value_name = "YYYY-MM-DD", value_parser = parse_date, conflicts_with = "cursor")]
    to: Option<NaiveDate>,
    /// Only items with at least this many points; comments have none.
    #[arg(long, value_name = "N", conflicts_with = "cursor")]
    min_points: Option<u64>,
    /// Only items with at least this many comments.
    #[arg(long, value_name = "N", conflicts_with = "cursor")]
    min_comments: Option<u64>,
    /// Maximum results on this page; a search reaches at most its first 1000 results.
    #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u16).range(1..=100), conflicts_with = "cursor")]
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
pub(in crate::app) struct HnFetchArgs {
    /// An `hn:<id>` ref or a news.ycombinator.com item URL.
    reference: String,
    /// `thread` writes the discussion to a file; `metadata` returns the item alone with live
    /// counts; `full_text` reads the page a story links to, or the item's own text.
    #[arg(long, value_enum, default_value_t = HnDepthArg::Thread)]
    depth: HnDepthArg,
    /// Directory for the discussion or full-text Markdown file; defaults to a new directory
    /// under the system temporary directory.
    #[arg(long, value_name = "DIR")]
    content_dir: Option<PathBuf>,
    /// `content` prints the discussion or the full text to stdout and writes no file.
    #[arg(long, value_enum, default_value_t = DocsOutputFormat::Json)]
    format: DocsOutputFormat,
    #[command(flatten)]
    common: PlatformCommonArgs,
}

#[derive(Debug, Args)]
pub(in crate::app) struct HnStoriesArgs {
    /// The list: `top` is the front page.
    #[arg(value_enum, default_value_t = HnListArg::Top)]
    list: HnListArg,
    /// Maximum items. The list has no cursor; it changes constantly.
    #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u16).range(1..=100))]
    limit: u16,
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    format: OutputFormat,
    #[command(flatten)]
    common: PlatformCommonArgs,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum HnSortArg {
    Relevance,
    Date,
}

impl From<HnSortArg> for HnSort {
    fn from(value: HnSortArg) -> Self {
        match value {
            HnSortArg::Relevance => Self::Relevance,
            HnSortArg::Date => Self::Date,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum HnTypeArg {
    Story,
    Comment,
    #[value(name = "ask_hn")]
    AskHn,
    #[value(name = "show_hn")]
    ShowHn,
    #[value(name = "launch_hn")]
    LaunchHn,
    Job,
    Poll,
}

impl From<HnTypeArg> for HnSearchType {
    fn from(value: HnTypeArg) -> Self {
        match value {
            HnTypeArg::Story => Self::Story,
            HnTypeArg::Comment => Self::Comment,
            HnTypeArg::AskHn => Self::AskHn,
            HnTypeArg::ShowHn => Self::ShowHn,
            HnTypeArg::LaunchHn => Self::LaunchHn,
            HnTypeArg::Job => Self::Job,
            HnTypeArg::Poll => Self::Poll,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum HnDepthArg {
    Thread,
    Metadata,
    Snippet,
    Abstract,
    #[value(name = "full_text")]
    FullText,
}

impl From<HnDepthArg> for ContentDepth {
    fn from(value: HnDepthArg) -> Self {
        match value {
            HnDepthArg::Thread => Self::Thread,
            HnDepthArg::Metadata => Self::Metadata,
            HnDepthArg::Snippet => Self::Snippet,
            HnDepthArg::Abstract => Self::Abstract,
            HnDepthArg::FullText => Self::FullText,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum HnListArg {
    Top,
    New,
    Best,
    Ask,
    Show,
    Job,
}

impl From<HnListArg> for HnStoriesList {
    fn from(value: HnListArg) -> Self {
        match value {
            HnListArg::Top => Self::Top,
            HnListArg::New => Self::New,
            HnListArg::Best => Self::Best,
            HnListArg::Ask => Self::Ask,
            HnListArg::Show => Self::Show,
            HnListArg::Job => Self::Job,
        }
    }
}

fn parse_item_id(value: &str) -> Result<u64, String> {
    HnRef::from_id(value)
        .map(HnRef::id)
        .ok_or_else(|| format!("`{value}` is not a Hacker News item ID"))
}

pub(in crate::app) fn run(command: HnCommand) -> Result<CommandOutput, AppError> {
    match command {
        HnCommand::Search(arguments) => hn_search(arguments),
        HnCommand::Fetch(arguments) => hn_fetch(arguments),
        HnCommand::Stories(arguments) => hn_stories(arguments),
    }
}

fn hn_search(arguments: HnSearchArgs) -> Result<CommandOutput, AppError> {
    let HnSearchArgs {
        query,
        sort,
        types,
        author,
        story,
        from,
        to,
        min_points,
        min_comments,
        limit,
        cursor,
        format,
        common,
    } = arguments;
    let input = if let Some(cursor) = cursor {
        PageInput::Cursor(cursor)
    } else {
        let options = HnSearchOptions {
            sort: sort.into(),
            types: types.into_iter().map(HnSearchType::from).collect(),
            author,
            story,
            from,
            to,
            min_points,
            min_comments,
        };
        let request = PlatformSearchRequest {
            query: query.unwrap_or_default(),
            limit,
            options: PlatformSearchOptions::Hn(options.normalized()),
            page: None,
        };
        request.validate().map_err(AppError::Argument)?;
        PageInput::Request(request)
    };
    search(Platform::Hn, input, format, &common)
}

fn hn_fetch(arguments: HnFetchArgs) -> Result<CommandOutput, AppError> {
    let HnFetchArgs {
        reference,
        depth,
        content_dir,
        format,
        common,
    } = arguments;
    let reference = PlatformRef::parse(Platform::Hn, &reference)
        .map_err(|error| AppError::Argument(error.to_string()))?;
    let request = PlatformFetchRequest {
        reference,
        depth: depth.into(),
        access: None,
    };
    fetch(Platform::Hn, request, format, content_dir, false, &common)
}

fn hn_stories(arguments: HnStoriesArgs) -> Result<CommandOutput, AppError> {
    let HnStoriesArgs {
        list,
        limit,
        format,
        common,
    } = arguments;
    let request = HnStoriesRequest {
        list: list.into(),
        limit,
    };
    let dependencies = NetworkDependencies::load()?;
    let plan =
        platform_stories::plan_stories(dependencies.config.platforms.get(Platform::Hn), request)
            .map_err(preflight_error)?;
    let result = dependencies.runtime.block_on(platform_stories::run_stories(
        plan,
        dependencies.client,
        dependencies.retry_policy,
        Deadline::new(Duration::from_secs(common.timeout)),
        common.verbose,
    ));
    let attempt_log = provider_attempt_log(dependencies.config.log_level, &result, |listing| {
        &listing.attempts
    });
    Ok(CommandOutput::HnStories {
        result,
        format,
        output: common.output.target(),
        attempt_log,
    })
}
