//! YouTube arguments and request construction.

use std::path::PathBuf;
use std::time::Duration;

use chrono::NaiveDate;
use clap::{Args, Subcommand, ValueEnum};

use super::{
    PageInput, PlatformCommonArgs, ask, comments, content_file_stem, deliver, fetch, parse_date,
    preflight_error, run_page, search,
};
use crate::app::args::{DocsOutputFormat, OutputArgs, OutputFormat};
use crate::app::dispatch::{
    AppError, CommandOutput, NetworkDependencies, invocation_temp_dir, provider_attempt_log,
};
use crate::platform_chain;
use crate::platform_transcribe;
use crate::types::{
    ContentDepth, Deadline, Platform, PlatformAskRequest, PlatformCommentsRequest,
    PlatformFetchOptions, PlatformFetchRequest, PlatformRef, PlatformSearchOptions,
    PlatformSearchRequest, PlatformTranscribeResult, YoutubeAskRequest, YoutubeCollection,
    YoutubeCommentSort, YoutubeCommentsRequest, YoutubeDuration, YoutubeFetchOptions, YoutubeKind,
    YoutubeRef, YoutubeSearchOptions, YoutubeSort, YoutubeTranscribeRequest, YoutubeVideosRequest,
};

// A model reads a long video for minutes, so the deadline leaves room for `gemini_api`'s own
// five-minute request timeout after the metadata check.
const MODEL_TIMEOUT_SECONDS: u64 = 600;

#[derive(Debug, Subcommand)]
pub(in crate::app) enum YoutubeCommand {
    /// Search YouTube videos through the official Data API; each result carries its ref,
    /// channel, description excerpt, duration, view count, and whether it has captions.
    Search(YoutubeSearchArgs),
    /// Fetch one YouTube video: its metadata, its full description and chapters, or the
    /// transcript of one of its caption tracks.
    Fetch(YoutubeFetchArgs),
    /// List the top-level comments of one YouTube video through the official Data API, with
    /// the replies each thread carries.
    Comments(YoutubeCommentsArgs),
    /// List the videos of a YouTube playlist in its order, or of a channel newest first,
    /// through the official Data API; each item carries the fields of a search result and its
    /// position.
    Videos(YoutubeVideosArgs),
    /// Answer a question about one YouTube video from its picture and sound, through a Gemini
    /// model; the answer marks moments as mm:ss, and the whole answer is relayed by the model.
    Ask(YoutubeAskArgs),
    /// Transcribe one YouTube video's speech with a Gemini model, for a video without captions;
    /// the transcript is not verbatim and is written to a local Markdown file.
    Transcribe(YoutubeTranscribeArgs),
}

#[derive(Debug, Args)]
pub(in crate::app) struct YoutubeSearchArgs {
    /// Words to match, as YouTube interprets them: `-word` excludes a word and `a|b` matches
    /// either.
    #[arg(conflicts_with = "cursor")]
    query: Option<String>,
    /// Result order.
    #[arg(long, value_enum, default_value_t = YoutubeSortArg::Relevance, conflicts_with = "cursor")]
    sort: YoutubeSortArg,
    /// First included publication day (UTC).
    #[arg(long, value_name = "YYYY-MM-DD", value_parser = parse_date, conflicts_with = "cursor")]
    from: Option<NaiveDate>,
    /// Last included publication day (UTC); the whole day is included.
    #[arg(long, value_name = "YYYY-MM-DD", value_parser = parse_date, conflicts_with = "cursor")]
    to: Option<NaiveDate>,
    /// Length class: `short` is under 4 minutes, `medium` 4 to 20, `long` over 20.
    #[arg(long, value_enum, conflicts_with = "cursor")]
    duration: Option<YoutubeDurationArg>,
    /// Only videos of this channel: a `youtube:channel/<UC…>` ref or a /channel/<UC…> URL.
    #[arg(long, value_name = "CHANNEL", value_parser = parse_channel, conflicts_with = "cursor")]
    channel: Option<String>,
    /// Only videos that have captions, in any language.
    #[arg(long, conflicts_with = "cursor")]
    has_captions: bool,
    /// Maximum results on this page.
    #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u16).range(1..=50), conflicts_with = "cursor")]
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
pub(in crate::app) struct YoutubeFetchArgs {
    /// A `youtube:<video_id>` ref or a YouTube video URL (watch, youtu.be, shorts, live, or
    /// embed).
    reference: String,
    /// `metadata` returns the video's bibliographic fields; `abstract` adds the full
    /// description and its chapters; `full_text` renders one caption track as a transcript.
    #[arg(long, value_enum, default_value_t = YoutubeDepthArg::FullText)]
    depth: YoutubeDepthArg,
    /// Caption language to read at `full_text`, such as `en` or `pt-BR`; repeat it in priority
    /// order. Every publisher track in these languages comes before any automatic one; machine
    /// translations never count. Without it, the video's original language is read.
    #[arg(long = "lang", value_name = "LANG")]
    languages: Vec<String>,
    /// Directory for the full-text Markdown file; defaults to a new directory under the system
    /// temporary directory.
    #[arg(long, value_name = "DIR")]
    content_dir: Option<PathBuf>,
    /// `content` prints the description (or the full text) to stdout and writes no file.
    #[arg(long, value_enum, default_value_t = DocsOutputFormat::Json)]
    format: DocsOutputFormat,
    #[command(flatten)]
    common: PlatformCommonArgs,
}

#[derive(Debug, Args)]
pub(in crate::app) struct YoutubeCommentsArgs {
    /// A `youtube:<video_id>` ref or a YouTube video URL.
    reference: String,
    /// Maximum top-level comments; one command reads one page and cannot continue later.
    #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u16).range(1..=100))]
    limit: u16,
    /// Comment order; `time` is newest first, though a pinned comment may still lead.
    #[arg(long, value_enum, default_value_t = YoutubeCommentSortArg::Relevance)]
    sort: YoutubeCommentSortArg,
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    format: OutputFormat,
    #[command(flatten)]
    common: PlatformCommonArgs,
}

#[derive(Debug, Args)]
pub(in crate::app) struct YoutubeVideosArgs {
    /// A `youtube:playlist/<id>` or `youtube:channel/<UC…>` ref, or a playlist URL, a watch URL
    /// with `list=`, a /channel/<UC…> URL, or an /@<handle> URL.
    #[arg(required_unless_present = "cursor", conflicts_with = "cursor")]
    collection: Option<String>,
    /// Maximum videos on this page.
    #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u16).range(1..=50), conflicts_with = "cursor")]
    limit: u16,
    /// Opaque `next_cursor` from a previous page; it restores the collection and the limit.
    #[arg(long)]
    cursor: Option<String>,
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    format: OutputFormat,
    #[command(flatten)]
    common: PlatformCommonArgs,
}

#[derive(Debug, Args)]
pub(in crate::app) struct YoutubeAskArgs {
    /// A `youtube:<video_id>` ref or a YouTube video URL.
    reference: String,
    /// The question to answer from the video.
    question: String,
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    format: OutputFormat,
    #[command(flatten)]
    common: ModelCommonArgs,
}

#[derive(Debug, Args)]
pub(in crate::app) struct YoutubeTranscribeArgs {
    /// A `youtube:<video_id>` ref or a YouTube video URL.
    reference: String,
    /// Directory for the transcript Markdown file; defaults to a new directory under the system
    /// temporary directory.
    #[arg(long, value_name = "DIR")]
    content_dir: Option<PathBuf>,
    /// `content` prints the transcript to stdout and writes no file.
    #[arg(long, value_enum, default_value_t = DocsOutputFormat::Json)]
    format: DocsOutputFormat,
    #[command(flatten)]
    common: ModelCommonArgs,
}

/// The common flags of a model operation, whose deadline is longer than other platform
/// commands'.
#[derive(Debug, Args)]
struct ModelCommonArgs {
    /// Whole-command deadline in seconds, including the metadata check and waits for the
    /// platform request window.
    #[arg(long, default_value_t = MODEL_TIMEOUT_SECONDS, value_parser = clap::value_parser!(u64).range(1..))]
    timeout: u64,
    #[command(flatten)]
    output: OutputArgs,
    /// Include every provider attempt, including skipped routes and the metadata check.
    #[arg(long)]
    verbose: bool,
}

impl From<ModelCommonArgs> for PlatformCommonArgs {
    fn from(common: ModelCommonArgs) -> Self {
        Self {
            timeout: common.timeout,
            output: common.output,
            verbose: common.verbose,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum YoutubeCommentSortArg {
    Relevance,
    Time,
}

impl From<YoutubeCommentSortArg> for YoutubeCommentSort {
    fn from(value: YoutubeCommentSortArg) -> Self {
        match value {
            YoutubeCommentSortArg::Relevance => Self::Relevance,
            YoutubeCommentSortArg::Time => Self::Time,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum YoutubeSortArg {
    Relevance,
    Date,
    Views,
    Rating,
}

impl From<YoutubeSortArg> for YoutubeSort {
    fn from(value: YoutubeSortArg) -> Self {
        match value {
            YoutubeSortArg::Relevance => Self::Relevance,
            YoutubeSortArg::Date => Self::Date,
            YoutubeSortArg::Views => Self::Views,
            YoutubeSortArg::Rating => Self::Rating,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum YoutubeDurationArg {
    Short,
    Medium,
    Long,
}

impl From<YoutubeDurationArg> for YoutubeDuration {
    fn from(value: YoutubeDurationArg) -> Self {
        match value {
            YoutubeDurationArg::Short => Self::Short,
            YoutubeDurationArg::Medium => Self::Medium,
            YoutubeDurationArg::Long => Self::Long,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum YoutubeDepthArg {
    Metadata,
    Abstract,
    #[value(name = "full_text")]
    FullText,
}

impl From<YoutubeDepthArg> for ContentDepth {
    fn from(value: YoutubeDepthArg) -> Self {
        match value {
            YoutubeDepthArg::Metadata => Self::Metadata,
            YoutubeDepthArg::Abstract => Self::Abstract,
            YoutubeDepthArg::FullText => Self::FullText,
        }
    }
}

/// Reads a channel ref or channel URL to its `UC…` ID.
fn parse_channel(value: &str) -> Result<String, String> {
    YoutubeRef::parse(value)
        .ok()
        .filter(|reference| reference.kind() == YoutubeKind::Channel)
        .map(|reference| reference.id().to_owned())
        .ok_or_else(|| {
            format!(
                "`{value}` is not a YouTube channel; pass a `youtube:channel/<UC…>` ref or a https://www.youtube.com/channel/<UC…> URL"
            )
        })
}

pub(in crate::app) fn run(command: YoutubeCommand) -> Result<CommandOutput, AppError> {
    match command {
        YoutubeCommand::Search(arguments) => youtube_search(arguments),
        YoutubeCommand::Fetch(arguments) => youtube_fetch(arguments),
        YoutubeCommand::Comments(arguments) => youtube_comments(arguments),
        YoutubeCommand::Videos(arguments) => youtube_videos(arguments),
        YoutubeCommand::Ask(arguments) => youtube_ask(arguments),
        YoutubeCommand::Transcribe(arguments) => youtube_transcribe(arguments),
    }
}

fn youtube_search(arguments: YoutubeSearchArgs) -> Result<CommandOutput, AppError> {
    let YoutubeSearchArgs {
        query,
        sort,
        from,
        to,
        duration,
        channel,
        has_captions,
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
            options: PlatformSearchOptions::Youtube(YoutubeSearchOptions {
                sort: sort.into(),
                from,
                to,
                duration: duration.map(YoutubeDuration::from),
                channel,
                has_captions,
            }),
            page: None,
        };
        request.validate().map_err(AppError::Argument)?;
        PageInput::Request(request)
    };
    search(Platform::Youtube, input, format, &common)
}

fn youtube_fetch(arguments: YoutubeFetchArgs) -> Result<CommandOutput, AppError> {
    let YoutubeFetchArgs {
        reference,
        depth,
        languages,
        content_dir,
        format,
        common,
    } = arguments;
    let video = parse_video(&reference, "fetch")?;
    let request = PlatformFetchRequest {
        options: PlatformFetchOptions::Youtube(YoutubeFetchOptions {
            languages,
            ..YoutubeFetchOptions::default()
        }),
        ..PlatformFetchRequest::new(PlatformRef::Youtube(video), depth.into())
    };
    request.validate().map_err(AppError::Argument)?;
    fetch(
        Platform::Youtube,
        request,
        format,
        content_dir,
        false,
        &common,
    )
}

fn youtube_comments(arguments: YoutubeCommentsArgs) -> Result<CommandOutput, AppError> {
    let YoutubeCommentsArgs {
        reference,
        limit,
        sort,
        format,
        common,
    } = arguments;
    let request = YoutubeCommentsRequest {
        video: parse_video(&reference, "comments")?,
        limit,
        sort: sort.into(),
    };
    comments(PlatformCommentsRequest::Youtube(request), format, &common)
}

fn youtube_videos(arguments: YoutubeVideosArgs) -> Result<CommandOutput, AppError> {
    let YoutubeVideosArgs {
        collection,
        limit,
        cursor,
        format,
        common,
    } = arguments;
    let input = if let Some(cursor) = cursor {
        PageInput::Cursor(cursor)
    } else {
        let collection = YoutubeCollection::parse(&collection.unwrap_or_default())
            .map_err(|error| AppError::Argument(error.to_string()))?;
        PageInput::Request(YoutubeVideosRequest {
            collection,
            limit,
            page: None,
        })
    };
    run_page(
        Platform::Youtube,
        |config| match input {
            PageInput::Request(request) => platform_chain::plan_videos(config, request),
            PageInput::Cursor(cursor) => platform_chain::plan_cursor_videos(config, &cursor),
        },
        format,
        &common,
    )
}

fn youtube_ask(arguments: YoutubeAskArgs) -> Result<CommandOutput, AppError> {
    let YoutubeAskArgs {
        reference,
        question,
        format,
        common,
    } = arguments;
    let request = YoutubeAskRequest {
        video: parse_video(&reference, "ask")?,
        question,
    };
    request.validate().map_err(AppError::Argument)?;
    ask(PlatformAskRequest::Youtube(request), format, &common.into())
}

/// Plans the transcription over YouTube's configuration, runs it, and delivers its transcript
/// like a full text, under a name that marks it as transcribed.
fn youtube_transcribe(arguments: YoutubeTranscribeArgs) -> Result<CommandOutput, AppError> {
    let YoutubeTranscribeArgs {
        reference,
        content_dir,
        format,
        common,
    } = arguments;
    let common = PlatformCommonArgs::from(common);
    let video = parse_video(&reference, "transcribe")?;
    let file_name = format!(
        "{}-transcribed.md",
        content_file_stem(&PlatformRef::Youtube(video.clone()))
    );
    let dependencies = NetworkDependencies::load()?;
    let plan = platform_transcribe::plan_transcribe(
        dependencies.config.platforms.get(Platform::Youtube),
        YoutubeTranscribeRequest { video },
    )
    .map_err(preflight_error)?;
    let result = dependencies
        .runtime
        .block_on(platform_transcribe::run_transcribe(
            plan,
            dependencies.client,
            dependencies.retry_policy,
            Deadline::new(Duration::from_secs(common.timeout)),
            common.verbose,
        ))
        .and_then(|transcribed| {
            if format == DocsOutputFormat::Content {
                return Ok(transcribed);
            }
            let PlatformTranscribeResult {
                fetched,
                model,
                usage,
            } = transcribed;
            let directory = content_dir.unwrap_or_else(|| invocation_temp_dir("forager-platform"));
            deliver(fetched, &directory, &file_name, false, true, common.verbose).map(|fetched| {
                PlatformTranscribeResult {
                    fetched,
                    model,
                    usage,
                }
            })
        });
    let attempt_log = provider_attempt_log(dependencies.config.log_level, &result, |transcribed| {
        &transcribed.fetched.attempts
    });
    Ok(CommandOutput::PlatformTranscribe {
        result: Box::new(result),
        format,
        output: common.output.target(),
        attempt_log,
    })
}

/// Reads the one video an operation reads; a playlist or channel input is an argument error.
fn parse_video(input: &str, operation: &str) -> Result<YoutubeRef, AppError> {
    let reference =
        YoutubeRef::parse(input).map_err(|error| AppError::Argument(error.to_string()))?;
    if reference.kind() != YoutubeKind::Video {
        return Err(AppError::Argument(format!(
            "`youtube:{reference}` is a {}; youtube {operation} reads one video; list its videos with `forager platform youtube videos`",
            reference.kind().as_str()
        )));
    }
    Ok(reference)
}
