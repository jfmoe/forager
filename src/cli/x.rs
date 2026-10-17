//! X arguments and request construction.

use chrono::NaiveDate;
use clap::{Args, Subcommand, ValueEnum};

use super::{PageInput, PlatformCommonArgs, ask, fetch, parse_date, search};
use crate::app::args::{DocsOutputFormat, OutputFormat};
use crate::app::dispatch::{AppError, CommandOutput};
use crate::types::{
    ContentDepth, Platform, PlatformAskRequest, PlatformFetchRequest, PlatformRef,
    PlatformSearchOptions, PlatformSearchRequest, XAskRequest, XSearchOptions,
};

#[derive(Debug, Subcommand)]
pub(in crate::app) enum XCommand {
    /// Search X posts; each result carries the post ref, author handle, publication time, and
    /// the text as the model relayed it, which may differ from the original.
    Search(XSearchArgs),
    /// Fetch one X post and, by default, its thread.
    Fetch(XFetchArgs),
    /// Answer a question from X posts alone; the answer cites posts inline as [[N]](url), and
    /// the whole answer is relayed by the model.
    Ask(XAskArgs),
}

/// The filters xAI enforces on its server.
#[derive(Debug, Args)]
struct XFilterArgs {
    /// Only posts by this handle, with or without `@`; repeat for up to 20 handles.
    #[arg(long = "handle", value_name = "HANDLE")]
    handles: Vec<String>,
    /// No posts by this handle; repeat for up to 20 handles. Cannot be combined with --handle.
    #[arg(long = "exclude-handle", value_name = "HANDLE")]
    excluded_handles: Vec<String>,
    /// First included publication day (UTC).
    #[arg(long, value_name = "YYYY-MM-DD", value_parser = parse_date)]
    from: Option<NaiveDate>,
    /// Last included publication day (UTC); the whole day is included.
    #[arg(long, value_name = "YYYY-MM-DD", value_parser = parse_date)]
    to: Option<NaiveDate>,
}

impl From<XFilterArgs> for XSearchOptions {
    fn from(filters: XFilterArgs) -> Self {
        Self::new(
            filters.handles,
            filters.excluded_handles,
            filters.from,
            filters.to,
        )
    }
}

#[derive(Debug, Args)]
pub(in crate::app) struct XSearchArgs {
    /// What to find on X, in natural language or X search syntax; the model chooses how to
    /// search.
    query: String,
    #[command(flatten)]
    filters: XFilterArgs,
    /// Maximum posts. X search has no cursor: rewrite the query or raise the limit for more.
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u16).range(1..=20))]
    limit: u16,
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    format: OutputFormat,
    #[command(flatten)]
    common: PlatformCommonArgs,
}

#[derive(Debug, Args)]
pub(in crate::app) struct XFetchArgs {
    /// An `x:<post_id>` ref, or an x.com or twitter.com post URL.
    reference: String,
    /// `thread` adds the posts it replies to, its replies, and the posts it quotes; `metadata`
    /// returns the post alone.
    #[arg(long, value_enum, default_value_t = XDepthArg::Thread)]
    depth: XDepthArg,
    /// `content` prints the post text, or the thread as Markdown.
    #[arg(long, value_enum, default_value_t = DocsOutputFormat::Json)]
    format: DocsOutputFormat,
    #[command(flatten)]
    common: PlatformCommonArgs,
}

#[derive(Debug, Args)]
pub(in crate::app) struct XAskArgs {
    /// The question to answer from X posts.
    query: String,
    #[command(flatten)]
    filters: XFilterArgs,
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    format: OutputFormat,
    #[command(flatten)]
    common: PlatformCommonArgs,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum XDepthArg {
    Thread,
    Metadata,
    Snippet,
    Abstract,
    #[value(name = "full_text")]
    FullText,
}

impl From<XDepthArg> for ContentDepth {
    fn from(value: XDepthArg) -> Self {
        match value {
            XDepthArg::Thread => Self::Thread,
            XDepthArg::Metadata => Self::Metadata,
            XDepthArg::Snippet => Self::Snippet,
            XDepthArg::Abstract => Self::Abstract,
            XDepthArg::FullText => Self::FullText,
        }
    }
}

pub(in crate::app) fn run(command: XCommand) -> Result<CommandOutput, AppError> {
    match command {
        XCommand::Search(arguments) => x_search(arguments),
        XCommand::Fetch(arguments) => x_fetch(arguments),
        XCommand::Ask(arguments) => x_ask(arguments),
    }
}

fn x_search(arguments: XSearchArgs) -> Result<CommandOutput, AppError> {
    let XSearchArgs {
        query,
        filters,
        limit,
        format,
        common,
    } = arguments;
    let request = PlatformSearchRequest {
        query,
        limit,
        options: PlatformSearchOptions::X(filters.into()),
        page: None,
    };
    request.validate().map_err(AppError::Argument)?;
    search(Platform::X, PageInput::Request(request), format, &common)
}

fn x_fetch(arguments: XFetchArgs) -> Result<CommandOutput, AppError> {
    let XFetchArgs {
        reference,
        depth,
        format,
        common,
    } = arguments;
    let reference = PlatformRef::parse(Platform::X, &reference)
        .map_err(|error| AppError::Argument(error.to_string()))?;
    let request = PlatformFetchRequest::new(reference, depth.into());
    fetch(Platform::X, request, format, None, false, &common)
}

fn x_ask(arguments: XAskArgs) -> Result<CommandOutput, AppError> {
    let XAskArgs {
        query,
        filters,
        format,
        common,
    } = arguments;
    let request = XAskRequest {
        query,
        options: filters.into(),
    };
    request.validate().map_err(AppError::Argument)?;
    ask(PlatformAskRequest::X(request), format, &common)
}
