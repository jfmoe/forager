//! Xiaohongshu arguments and request construction.

use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};

use super::{PageInput, PlatformCommonArgs, fetch, search};
use crate::app::args::{DocsOutputFormat, OutputFormat};
use crate::app::dispatch::{AppError, CommandOutput};
use crate::types::{
    ContentDepth, Platform, PlatformFetchRequest, PlatformRef, PlatformSearchOptions,
    PlatformSearchRequest, XiaohongshuNoteType, XiaohongshuPublishTime, XiaohongshuRef,
    XiaohongshuSearchOptions, XiaohongshuSort,
};

#[derive(Debug, Subcommand)]
pub(in crate::app) enum XiaohongshuCommand {
    /// Search Xiaohongshu notes in your own logged-in Chrome; each result carries the note card
    /// and an `access_url` that opens the note. Needs `xiaohongshu_browser` in
    /// `platforms.xiaohongshu.order`.
    Search(XiaohongshuSearchArgs),
    /// Read one Xiaohongshu note in your own logged-in Chrome; by default its text is written to
    /// a local Markdown file. Needs the note's access token, as in a search result's
    /// `access_url`.
    Fetch(XiaohongshuFetchArgs),
}

#[derive(Debug, Args)]
pub(in crate::app) struct XiaohongshuFetchArgs {
    /// The `access_url` of a search result, or a xiaohongshu.com note URL with its
    /// `xsec_token`.
    reference: String,
    /// `full_text` writes the note text to a Markdown file; `metadata` returns only the note
    /// fields.
    #[arg(long, value_enum, default_value_t = FetchDepthArg::FullText)]
    depth: FetchDepthArg,
    /// Directory for the full-text Markdown file; defaults to a new directory under the system
    /// temporary directory.
    #[arg(long, value_name = "DIR")]
    content_dir: Option<PathBuf>,
    /// `content` prints the note text to stdout and writes no file.
    #[arg(long, value_enum, default_value_t = DocsOutputFormat::Json)]
    format: DocsOutputFormat,
    #[command(flatten)]
    common: PlatformCommonArgs,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum FetchDepthArg {
    Metadata,
    #[value(name = "full_text")]
    FullText,
}

impl From<FetchDepthArg> for ContentDepth {
    fn from(value: FetchDepthArg) -> Self {
        match value {
            FetchDepthArg::Metadata => Self::Metadata,
            FetchDepthArg::FullText => Self::FullText,
        }
    }
}

#[derive(Debug, Args)]
pub(in crate::app) struct XiaohongshuSearchArgs {
    /// Search words, typed into Xiaohongshu unchanged; no phrase or operator syntax is promised.
    query: String,
    /// Maximum results; one command reads up to five pages of 20 and cannot continue later.
    #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u16).range(1..=100))]
    limit: u16,
    /// Result order.
    #[arg(long, value_enum, default_value_t = SortArg::Comprehensive)]
    sort: SortArg,
    /// Only image notes or only video notes.
    #[arg(long, value_enum, default_value_t = NoteTypeArg::All)]
    note_type: NoteTypeArg,
    /// Only notes published within this period.
    #[arg(long, value_enum, default_value_t = PublishTimeArg::Any)]
    publish_time: PublishTimeArg,
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    format: OutputFormat,
    #[command(flatten)]
    common: PlatformCommonArgs,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum SortArg {
    Comprehensive,
    Latest,
    MostLiked,
    MostCommented,
    MostCollected,
}

impl From<SortArg> for XiaohongshuSort {
    fn from(value: SortArg) -> Self {
        match value {
            SortArg::Comprehensive => Self::Comprehensive,
            SortArg::Latest => Self::Latest,
            SortArg::MostLiked => Self::MostLiked,
            SortArg::MostCommented => Self::MostCommented,
            SortArg::MostCollected => Self::MostCollected,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum NoteTypeArg {
    All,
    Image,
    Video,
}

impl From<NoteTypeArg> for XiaohongshuNoteType {
    fn from(value: NoteTypeArg) -> Self {
        match value {
            NoteTypeArg::All => Self::All,
            NoteTypeArg::Image => Self::Image,
            NoteTypeArg::Video => Self::Video,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum PublishTimeArg {
    Any,
    Day,
    Week,
    HalfYear,
}

impl From<PublishTimeArg> for XiaohongshuPublishTime {
    fn from(value: PublishTimeArg) -> Self {
        match value {
            PublishTimeArg::Any => Self::Any,
            PublishTimeArg::Day => Self::Day,
            PublishTimeArg::Week => Self::Week,
            PublishTimeArg::HalfYear => Self::HalfYear,
        }
    }
}

pub(in crate::app) fn run(command: XiaohongshuCommand) -> Result<CommandOutput, AppError> {
    match command {
        XiaohongshuCommand::Search(arguments) => xiaohongshu_search(arguments),
        XiaohongshuCommand::Fetch(arguments) => xiaohongshu_fetch(arguments),
    }
}

fn xiaohongshu_fetch(arguments: XiaohongshuFetchArgs) -> Result<CommandOutput, AppError> {
    let XiaohongshuFetchArgs {
        reference,
        depth,
        content_dir,
        format,
        common,
    } = arguments;
    let (reference, token) =
        XiaohongshuRef::parse_accessible(&reference).map_err(AppError::Argument)?;
    let request = PlatformFetchRequest {
        reference: PlatformRef::Xiaohongshu(reference),
        depth: depth.into(),
        access: Some(token),
    };
    fetch(
        Platform::Xiaohongshu,
        request,
        format,
        content_dir,
        false,
        &common,
    )
}

fn xiaohongshu_search(arguments: XiaohongshuSearchArgs) -> Result<CommandOutput, AppError> {
    let XiaohongshuSearchArgs {
        query,
        limit,
        sort,
        note_type,
        publish_time,
        format,
        common,
    } = arguments;
    let request = PlatformSearchRequest {
        query,
        limit,
        options: PlatformSearchOptions::Xiaohongshu(XiaohongshuSearchOptions {
            sort: sort.into(),
            note_type: note_type.into(),
            publish_time: publish_time.into(),
        }),
        page: None,
    };
    request.validate().map_err(AppError::Argument)?;
    search(
        Platform::Xiaohongshu,
        PageInput::Request(request),
        format,
        &common,
    )
}
