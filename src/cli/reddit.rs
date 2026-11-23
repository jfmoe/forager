//! Reddit arguments and request construction.

use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};

use super::{PageInput, PlatformCommonArgs, fetch, search};
use crate::app::args::{ContentOutputFormat, OutputFormat};
use crate::app::dispatch::{AppError, CommandOutput};
use crate::types::{
    ContentDepth, Platform, PlatformFetchRequest, PlatformRef, PlatformSearchOptions,
    PlatformSearchRequest, RedditSearchOptions, RedditSort, RedditTime,
};

#[derive(Debug, Subcommand)]
pub(in crate::app) enum RedditCommand {
    /// Search Reddit posts in the logged-in Browser Profile; each result carries its ref,
    /// subreddit, score, comment count, and the excerpt the search shows. Needs
    /// `reddit_browser` in `platforms.reddit.order`.
    Search(RedditSearchArgs),
    /// Read one Reddit post in the logged-in Browser Profile; by default the post and the
    /// comments its page loads are written to a local Markdown file. Needs `reddit_browser` in
    /// `platforms.reddit.order`.
    Fetch(RedditFetchArgs),
}

#[derive(Debug, Args)]
pub(in crate::app) struct RedditSearchArgs {
    /// Words to search for, in Reddit's own search syntax.
    query: String,
    /// Only posts in this subreddit, with or without the `r/` prefix.
    #[arg(long, value_name = "SUBREDDIT")]
    subreddit: Option<String>,
    /// Result order.
    #[arg(long, value_enum, default_value_t = RedditSortArg::Relevance)]
    sort: RedditSortArg,
    /// How far back to search; applies to the relevance, top, and comments orders.
    #[arg(long, value_enum)]
    time: Option<RedditTimeArg>,
    /// Maximum results; one command scrolls the results page up to six times and cannot
    /// continue later.
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u16).range(1..=25))]
    limit: u16,
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    format: OutputFormat,
    #[command(flatten)]
    common: PlatformCommonArgs,
}

#[derive(Debug, Args)]
pub(in crate::app) struct RedditFetchArgs {
    /// A `reddit:<id>` ref, a reddit.com post URL, or a redd.it short link.
    reference: String,
    /// `thread` writes the post and the comments its page loads to a file; `metadata`
    /// returns the post with its own text; `full_text` reads the page a link post points to,
    /// or the post's own text.
    #[arg(long, value_enum, default_value_t = RedditDepthArg::Thread)]
    depth: RedditDepthArg,
    /// Directory for the discussion or full-text Markdown file; defaults to a new directory
    /// under the system temporary directory.
    #[arg(long, value_name = "DIR")]
    content_dir: Option<PathBuf>,
    /// `content` prints the discussion or the full text to stdout and writes no file.
    #[arg(long, value_enum, default_value_t = ContentOutputFormat::Json)]
    format: ContentOutputFormat,
    #[command(flatten)]
    common: PlatformCommonArgs,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum RedditSortArg {
    Relevance,
    Hot,
    Top,
    New,
    Comments,
}

impl From<RedditSortArg> for RedditSort {
    fn from(value: RedditSortArg) -> Self {
        match value {
            RedditSortArg::Relevance => Self::Relevance,
            RedditSortArg::Hot => Self::Hot,
            RedditSortArg::Top => Self::Top,
            RedditSortArg::New => Self::New,
            RedditSortArg::Comments => Self::Comments,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum RedditTimeArg {
    Hour,
    Day,
    Week,
    Month,
    Year,
    All,
}

impl From<RedditTimeArg> for RedditTime {
    fn from(value: RedditTimeArg) -> Self {
        match value {
            RedditTimeArg::Hour => Self::Hour,
            RedditTimeArg::Day => Self::Day,
            RedditTimeArg::Week => Self::Week,
            RedditTimeArg::Month => Self::Month,
            RedditTimeArg::Year => Self::Year,
            RedditTimeArg::All => Self::All,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum RedditDepthArg {
    Thread,
    Metadata,
    Snippet,
    Abstract,
    #[value(name = "full_text")]
    FullText,
}

impl From<RedditDepthArg> for ContentDepth {
    fn from(value: RedditDepthArg) -> Self {
        match value {
            RedditDepthArg::Thread => Self::Thread,
            RedditDepthArg::Metadata => Self::Metadata,
            RedditDepthArg::Snippet => Self::Snippet,
            RedditDepthArg::Abstract => Self::Abstract,
            RedditDepthArg::FullText => Self::FullText,
        }
    }
}

pub(in crate::app) fn run(command: RedditCommand) -> Result<CommandOutput, AppError> {
    match command {
        RedditCommand::Search(arguments) => reddit_search(arguments),
        RedditCommand::Fetch(arguments) => reddit_fetch(arguments),
    }
}

fn reddit_search(arguments: RedditSearchArgs) -> Result<CommandOutput, AppError> {
    let RedditSearchArgs {
        query,
        subreddit,
        sort,
        time,
        limit,
        format,
        common,
    } = arguments;
    let options = RedditSearchOptions {
        subreddit: subreddit.map(|name| {
            let name = name.trim();
            name.strip_prefix("r/")
                .or_else(|| name.strip_prefix("/r/"))
                .unwrap_or(name)
                .trim_end_matches('/')
                .to_owned()
        }),
        sort: sort.into(),
        time: time.map(RedditTime::from),
    };
    let request = PlatformSearchRequest {
        query,
        limit,
        options: PlatformSearchOptions::Reddit(options),
        page: None,
    };
    request.validate().map_err(AppError::Argument)?;
    search(
        Platform::Reddit,
        PageInput::Request(request),
        format,
        &common,
    )
}

fn reddit_fetch(arguments: RedditFetchArgs) -> Result<CommandOutput, AppError> {
    let RedditFetchArgs {
        reference,
        depth,
        content_dir,
        format,
        common,
    } = arguments;
    let reference = PlatformRef::parse(Platform::Reddit, &reference)
        .map_err(|error| AppError::Argument(error.to_string()))?;
    let request = PlatformFetchRequest::new(reference, depth.into());
    fetch(Platform::Reddit, request, format, content_dir, &common)
}
