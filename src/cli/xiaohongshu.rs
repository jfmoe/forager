//! Xiaohongshu arguments and request construction.

use clap::{Args, Subcommand, ValueEnum};

use super::{PageInput, PlatformCommonArgs, search};
use crate::app::args::OutputFormat;
use crate::app::dispatch::{AppError, CommandOutput};
use crate::types::{
    Platform, PlatformSearchOptions, PlatformSearchRequest, XiaohongshuNoteType,
    XiaohongshuPublishTime, XiaohongshuSearchOptions, XiaohongshuSort,
};

#[derive(Debug, Subcommand)]
pub(in crate::app) enum XiaohongshuCommand {
    /// Search Xiaohongshu notes in your own logged-in Chrome; each result carries the note card
    /// and an `access_url` that opens the note. Needs `xiaohongshu_browser` in
    /// `platforms.xiaohongshu.order`.
    Search(XiaohongshuSearchArgs),
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
    }
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
