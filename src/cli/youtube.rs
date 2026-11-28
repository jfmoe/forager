//! YouTube arguments and request construction.

use std::path::PathBuf;
use std::time::Duration;

use chrono::NaiveDate;
use clap::{Args, Subcommand, ValueEnum};

use super::{
    ContentInput, PageInput, PlatformCommonArgs, PlatformInput, ask, comments, content_file_stem,
    deliver, fetch, parse_date, preflight_error, run_page, search,
};
use crate::app::args::{ContentOutputFormat, OutputArgs, OutputFormat};
use crate::app::dispatch::{
    AppError, CommandOutput, NetworkDependencies, Presentation, provider_attempt_log,
};
use crate::platform_operation;
use crate::types::{
    ContentDepth, Deadline, Platform, PlatformAskRequest, PlatformCommentsRequest,
    PlatformFetchOptions, PlatformFetchRequest, PlatformRef, PlatformSearchOptions,
    PlatformSearchRequest, PlatformTranscribeResult, YoutubeAskRequest, YoutubeCollection,
    YoutubeCommentSort, YoutubeCommentsRequest, YoutubeDuration, YoutubeFetchOptions, YoutubeKind,
    YoutubeRef, YoutubeSearchOptions, YoutubeSort, YoutubeTranscribeRequest, YoutubeVideosRequest,
};

// A model reads a long video for minutes, so the deadline leaves room for `gemini_api`'s own
// five-minute request timeout after the precheck.
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
    #[arg(long, value_enum, default_value_t = ContentOutputFormat::Json)]
    format: ContentOutputFormat,
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
    #[arg(long, value_enum, default_value_t = ContentOutputFormat::Json)]
    format: ContentOutputFormat,
    #[command(flatten)]
    common: ModelCommonArgs,
}

/// The common flags of a model operation, whose deadline is longer than other platform
/// commands'.
#[derive(Debug, Args)]
struct ModelCommonArgs {
    /// Whole-command deadline in seconds, including the precheck that reads the video
    /// first and waits for the platform request window.
    #[arg(long, default_value_t = MODEL_TIMEOUT_SECONDS, value_parser = clap::value_parser!(u64).range(1..))]
    timeout: u64,
    #[command(flatten)]
    output: OutputArgs,
    /// Include every provider attempt, including skipped routes and the precheck.
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
    let PlatformInput {
        request: input,
        format,
        common,
    } = youtube_search_input(arguments)?;
    search(Platform::Youtube, input, format, &common)
}

/// Builds the page a search asks for and checks its cross-field rules before any request.
fn youtube_search_input(
    arguments: YoutubeSearchArgs,
) -> Result<PlatformInput<PageInput<PlatformSearchRequest>, OutputFormat>, AppError> {
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
    Ok(PlatformInput {
        request: input,
        format,
        common,
    })
}

fn youtube_fetch(arguments: YoutubeFetchArgs) -> Result<CommandOutput, AppError> {
    let ContentInput {
        request,
        format,
        content_dir,
        common,
    } = youtube_fetch_input(arguments)?;
    fetch(Platform::Youtube, request, format, content_dir, &common)
}

/// The fetch a YouTube fetch asks for, checked before any configuration is read.
fn youtube_fetch_input(
    arguments: YoutubeFetchArgs,
) -> Result<ContentInput<PlatformFetchRequest>, AppError> {
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
    Ok(ContentInput {
        request,
        format,
        content_dir,
        common,
    })
}

fn youtube_comments(arguments: YoutubeCommentsArgs) -> Result<CommandOutput, AppError> {
    let PlatformInput {
        request,
        format,
        common,
    } = youtube_comments_input(arguments)?;
    comments(PlatformCommentsRequest::Youtube(request), format, &common)
}

/// The comments page a command asks for, checked before any configuration is read.
fn youtube_comments_input(
    arguments: YoutubeCommentsArgs,
) -> Result<PlatformInput<YoutubeCommentsRequest, OutputFormat>, AppError> {
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
    Ok(PlatformInput {
        request,
        format,
        common,
    })
}

fn youtube_videos(arguments: YoutubeVideosArgs) -> Result<CommandOutput, AppError> {
    let PlatformInput {
        request: input,
        format,
        common,
    } = youtube_videos_input(arguments)?;
    run_page(
        Platform::Youtube,
        |config, routes| match input {
            PageInput::Request(request) => platform_operation::plan_videos(config, routes, request),
            PageInput::Cursor(cursor) => {
                platform_operation::plan_cursor_videos(config, routes, &cursor)
            }
        },
        platform_operation::run_videos,
        format,
        &common,
    )
}

/// The page of videos a command asks for, checked before any configuration is read.
fn youtube_videos_input(
    arguments: YoutubeVideosArgs,
) -> Result<PlatformInput<PageInput<YoutubeVideosRequest>, OutputFormat>, AppError> {
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
    Ok(PlatformInput {
        request: input,
        format,
        common,
    })
}

fn youtube_ask(arguments: YoutubeAskArgs) -> Result<CommandOutput, AppError> {
    let PlatformInput {
        request,
        format,
        common,
    } = youtube_ask_input(arguments)?;
    ask(PlatformAskRequest::Youtube(request), format, &common)
}

fn youtube_ask_input(
    arguments: YoutubeAskArgs,
) -> Result<PlatformInput<YoutubeAskRequest, OutputFormat>, AppError> {
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
    Ok(PlatformInput {
        request,
        format,
        common: common.into(),
    })
}

/// Plans the transcription over YouTube's configuration, runs it, and delivers its transcript
/// like a full text, under a name that marks it as transcribed.
fn youtube_transcribe(arguments: YoutubeTranscribeArgs) -> Result<CommandOutput, AppError> {
    let ContentInput {
        request,
        format,
        content_dir,
        common,
    } = youtube_transcribe_input(arguments)?;
    let file_name = format!(
        "{}-transcribed.md",
        content_file_stem(&PlatformRef::Youtube(request.video.clone()))
    );
    let dependencies = NetworkDependencies::load()?;
    let plan = platform_operation::plan_transcribe(
        dependencies.platform(Platform::Youtube)?,
        &dependencies.config.platform_routes,
        request,
    )
    .map_err(preflight_error)?;
    let result = dependencies
        .runtime
        .block_on(platform_operation::run_transcribe(
            plan,
            dependencies.client,
            dependencies.retry_policy,
            Deadline::new(Duration::from_secs(common.timeout)),
        ))
        .and_then(|transcribed| {
            let PlatformTranscribeResult {
                fetched,
                model,
                usage,
            } = transcribed;
            deliver(fetched, format, content_dir, &file_name).map(|fetched| {
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
        presentation: Presentation {
            format,
            output: common.output.target(),
            verbose: common.verbose,
            attempt_log,
        },
    })
}

fn youtube_transcribe_input(
    arguments: YoutubeTranscribeArgs,
) -> Result<ContentInput<YoutubeTranscribeRequest>, AppError> {
    let YoutubeTranscribeArgs {
        reference,
        content_dir,
        format,
        common,
    } = arguments;
    let video = parse_video(&reference, "transcribe")?;
    Ok(ContentInput {
        request: YoutubeTranscribeRequest { video },
        format,
        content_dir,
        common: common.into(),
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

/// The `--lang` rules of `platform youtube fetch`: caption languages exist for `youtube_web`'s
/// full text alone. Proven by parsing the command's definition in process.
#[cfg(test)]
mod youtube_web_tests {
    use super::super::{PlatformCommand, parse_platform};
    use super::{YoutubeCommand, YoutubeFetchArgs, youtube_fetch_input};
    use crate::app::AppError;
    use crate::types::PlatformFetchOptions;

    fn fetch_args(arguments: &[&str]) -> YoutubeFetchArgs {
        let fetch = ["youtube", "fetch", "youtube:jNQXAC9IVRw"]
            .into_iter()
            .chain(arguments.iter().copied());
        match parse_platform(&fetch.collect::<Vec<_>>()).expect("the parser accepts the arguments")
        {
            PlatformCommand::Youtube {
                command: YoutubeCommand::Fetch(arguments),
            } => arguments,
            _ => unreachable!("the arguments name youtube fetch"),
        }
    }

    #[test]
    fn repeated_lang_options_reach_the_request_in_their_priority_order() {
        let request = youtube_fetch_input(fetch_args(&["--lang", "fr", "--lang", "de"]))
            .expect("a full-text fetch takes languages")
            .request;

        let PlatformFetchOptions::Youtube(options) = request.options else {
            panic!("a YouTube fetch carries YouTube options")
        };
        assert_eq!(options.languages, ["fr", "de"]);
    }

    #[test]
    fn lang_outside_full_text_and_a_malformed_lang_are_argument_errors_before_any_request() {
        for arguments in [
            &["--depth", "metadata", "--lang", "en"][..],
            &["--lang", "english"],
        ] {
            let result = youtube_fetch_input(fetch_args(arguments)).map(|_| ());

            assert!(
                matches!(result, Err(AppError::Argument(_))),
                "arguments: {arguments:?}\n{result:?}"
            );
        }
    }
}

/// The argument rules of the `youtube_api` commands, proven by parsing their definition in
/// process.
#[cfg(test)]
mod youtube_api_tests {
    use chrono::NaiveDate;
    use clap::error::ErrorKind;

    use super::super::{PageInput, PlatformCommand, parse_platform};
    use super::{
        YoutubeCommand, youtube_comments_input, youtube_fetch_input, youtube_search_input,
        youtube_videos_input,
    };
    use crate::app::AppError;
    use crate::types::{
        ContentDepth, PlatformRef, PlatformSearchOptions, PlatformSearchRequest, YoutubeCollection,
        YoutubeCommentSort, YoutubeCommentsRequest, YoutubeDuration, YoutubeRef,
        YoutubeSearchOptions, YoutubeSort, YoutubeVideosRequest,
    };

    const ZOO: &str = "jNQXAC9IVRw";
    const CHANNEL: &str = "UCExampleMathsChannel001";
    const PLAYLIST: &str = "PLZHQObOWTQDPD3MizzM2xVFitgF8hE_ab";

    fn youtube(arguments: &[&str]) -> Result<YoutubeCommand, clap::Error> {
        let command = std::iter::once("youtube").chain(arguments.iter().copied());
        match parse_platform(&command.collect::<Vec<_>>())? {
            PlatformCommand::Youtube { command } => Ok(command),
            _ => unreachable!("the arguments name youtube"),
        }
    }

    fn parsed(arguments: &[&str]) -> YoutubeCommand {
        youtube(arguments)
            .unwrap_or_else(|error| panic!("the parser accepts {arguments:?}: {error}"))
    }

    /// The first page a search asks for, or the argument error its checks raise.
    fn search_request(arguments: &[&str]) -> Result<PlatformSearchRequest, AppError> {
        let search = std::iter::once("search").chain(arguments.iter().copied());
        let YoutubeCommand::Search(parsed) = parsed(&search.collect::<Vec<_>>()) else {
            unreachable!("the arguments name youtube search")
        };
        match youtube_search_input(parsed)?.request {
            PageInput::Request(request) => Ok(request),
            PageInput::Cursor(_) => panic!("the arguments name no cursor"),
        }
    }

    fn comments_request(arguments: &[&str]) -> Result<YoutubeCommentsRequest, AppError> {
        let comments = std::iter::once("comments").chain(arguments.iter().copied());
        let YoutubeCommand::Comments(parsed) = parsed(&comments.collect::<Vec<_>>()) else {
            unreachable!("the arguments name youtube comments")
        };
        youtube_comments_input(parsed).map(|input| input.request)
    }

    fn videos_request(arguments: &[&str]) -> Result<YoutubeVideosRequest, AppError> {
        let videos = std::iter::once("videos").chain(arguments.iter().copied());
        let YoutubeCommand::Videos(parsed) = parsed(&videos.collect::<Vec<_>>()) else {
            unreachable!("the arguments name youtube videos")
        };
        match youtube_videos_input(parsed)?.request {
            PageInput::Request(request) => Ok(request),
            PageInput::Cursor(_) => panic!("the arguments name no cursor"),
        }
    }

    fn fetch_reference_and_depth(
        arguments: &[&str],
    ) -> Result<(PlatformRef, ContentDepth), AppError> {
        let fetch = std::iter::once("fetch").chain(arguments.iter().copied());
        let YoutubeCommand::Fetch(parsed) = parsed(&fetch.collect::<Vec<_>>()) else {
            unreachable!("the arguments name youtube fetch")
        };
        youtube_fetch_input(parsed).map(|input| (input.request.reference, input.request.depth))
    }

    fn is_argument_error<T: std::fmt::Debug>(result: &Result<T, AppError>) -> bool {
        matches!(result, Err(AppError::Argument(_)))
    }

    fn video(id: &str) -> YoutubeRef {
        YoutubeRef::video(id).expect("a video ID")
    }

    // Search.

    #[test]
    fn every_search_option_reaches_the_request() {
        let request = search_request(&[
            "attention -music",
            "--sort",
            "views",
            "--from",
            "2024-01-01",
            "--to",
            "2024-01-31",
            "--duration",
            "long",
            "--channel",
            &format!("https://www.youtube.com/channel/{CHANNEL}"),
            "--has-captions",
            "--limit",
            "10",
        ])
        .expect("valid options");

        assert_eq!(
            (request.query, request.limit, request.options, request.page),
            (
                "attention -music".to_owned(),
                10,
                PlatformSearchOptions::Youtube(YoutubeSearchOptions {
                    sort: YoutubeSort::Views,
                    from: NaiveDate::from_ymd_opt(2024, 1, 1),
                    to: NaiveDate::from_ymd_opt(2024, 1, 31),
                    duration: Some(YoutubeDuration::Long),
                    channel: Some(CHANNEL.to_owned()),
                    has_captions: true,
                }),
                None,
            )
        );
    }

    #[test]
    fn a_search_without_options_asks_for_twenty_results_by_relevance() {
        let request = search_request(&["attention"]).expect("a query alone is valid");

        assert_eq!(
            (request.limit, request.options),
            (
                20,
                PlatformSearchOptions::Youtube(YoutubeSearchOptions::default())
            )
        );
    }

    #[test]
    fn malformed_dates_fail_to_parse_and_reversed_dates_are_argument_errors() {
        let malformed = [
            &["search", "attention", "--from", "2024-02-30"][..],
            &["search", "attention", "--to", "2024-1-31"],
        ]
        .map(|arguments| youtube(arguments).err().map(|error| error.kind()));
        let reversed = search_request(&["attention", "--from", "2024-02-01", "--to", "2024-01-31"]);

        assert_eq!(
            (malformed, is_argument_error(&reversed)),
            ([Some(ErrorKind::ValueValidation); 2], true),
            "{reversed:?}"
        );
    }

    // Comments.

    #[test]
    fn comments_read_the_video_of_a_watch_url_with_their_limit_and_order() {
        let request = comments_request(&[
            &format!("https://www.youtube.com/watch?v={ZOO}&t=4"),
            "--limit",
            "2",
            "--sort",
            "time",
        ])
        .expect("valid arguments");
        let defaults = comments_request(&[&format!("youtube:{ZOO}")]).expect("a ref alone");

        assert_eq!(
            [
                (request.video, request.limit, request.sort),
                (defaults.video, defaults.limit, defaults.sort),
            ],
            [
                (video(ZOO), 2, YoutubeCommentSort::Time),
                (video(ZOO), 20, YoutubeCommentSort::Relevance),
            ]
        );
    }

    #[test]
    fn comments_of_a_playlist_or_a_channel_are_argument_errors() {
        let results = [
            comments_request(&[&format!("youtube:playlist/{PLAYLIST}")]),
            comments_request(&[&format!("youtube:channel/{CHANNEL}")]),
        ];

        assert!(results.iter().all(is_argument_error), "{results:?}");
    }

    // Videos.

    #[test]
    fn videos_read_a_playlist_url_or_a_handle_with_their_limit() {
        let playlist = videos_request(&[
            &format!("https://www.youtube.com/playlist?list={PLAYLIST}"),
            "--limit",
            "2",
        ])
        .expect("a playlist URL");
        let handle =
            videos_request(&["https://www.youtube.com/@examplemaths/videos"]).expect("a handle");

        assert_eq!(
            [
                (playlist.collection, playlist.limit, playlist.page),
                (handle.collection, handle.limit, handle.page),
            ],
            [
                (
                    YoutubeCollection::Ref(YoutubeRef::playlist(PLAYLIST).expect("playlist")),
                    2,
                    None
                ),
                (
                    YoutubeCollection::Handle("@examplemaths".to_owned()),
                    20,
                    None
                ),
            ]
        );
    }

    #[test]
    fn videos_of_a_video_or_an_unknown_url_are_argument_errors() {
        let results = [
            videos_request(&[&format!("youtube:{ZOO}")]),
            videos_request(&["https://www.youtube.com/c/examplemaths"]),
        ];

        assert!(results.iter().all(is_argument_error), "{results:?}");
    }

    // Fetch.

    #[test]
    fn a_fetch_reads_the_video_of_a_short_url_at_full_text_by_default() {
        let short = fetch_reference_and_depth(&[&format!("https://youtu.be/{ZOO}?t=3")]);
        let metadata =
            fetch_reference_and_depth(&[&format!("youtube:{ZOO}"), "--depth", "metadata"]);

        assert_eq!(
            [short.ok(), metadata.ok()],
            [
                Some((PlatformRef::Youtube(video(ZOO)), ContentDepth::FullText)),
                Some((PlatformRef::Youtube(video(ZOO)), ContentDepth::Metadata)),
            ]
        );
    }

    #[test]
    fn a_fetch_of_a_playlist_a_channel_or_a_handle_is_an_argument_error() {
        let results = [
            fetch_reference_and_depth(&[&format!(
                "https://www.youtube.com/playlist?list={PLAYLIST}"
            )]),
            fetch_reference_and_depth(&[&format!("youtube:channel/{CHANNEL}")]),
            fetch_reference_and_depth(&["https://www.youtube.com/@examplemaths"]),
        ];

        assert!(results.iter().all(is_argument_error), "{results:?}");
    }
}

/// The argument rules of the `gemini_api` commands, ask and transcribe, proven by parsing their
/// definition in process.
#[cfg(test)]
mod gemini_api_tests {
    use std::path::PathBuf;

    use super::super::{PlatformCommand, parse_platform};
    use super::{YoutubeCommand, youtube_ask_input, youtube_transcribe_input};
    use crate::app::AppError;
    use crate::app::args::{ContentOutputFormat, OutputFormat};
    use crate::types::YoutubeRef;

    const ZOO: &str = "jNQXAC9IVRw";
    const QUESTION: &str = "What animals stand behind the speaker?";

    fn parsed(arguments: &[&str]) -> YoutubeCommand {
        let command = std::iter::once("youtube").chain(arguments.iter().copied());
        match parse_platform(&command.collect::<Vec<_>>())
            .unwrap_or_else(|error| panic!("the parser accepts {arguments:?}: {error}"))
        {
            PlatformCommand::Youtube { command } => command,
            _ => unreachable!("the arguments name youtube"),
        }
    }

    /// The video and question an ask reads, with its deadline, or its argument error.
    fn ask(arguments: &[&str]) -> Result<(YoutubeRef, String, OutputFormat, u64), AppError> {
        let ask = std::iter::once("ask").chain(arguments.iter().copied());
        let YoutubeCommand::Ask(parsed) = parsed(&ask.collect::<Vec<_>>()) else {
            unreachable!("the arguments name youtube ask")
        };
        youtube_ask_input(parsed).map(|input| {
            (
                input.request.video,
                input.request.question,
                input.format,
                input.common.timeout,
            )
        })
    }

    /// The video a transcription reads, its content directory, format and deadline, or its
    /// argument error.
    fn transcribe(
        arguments: &[&str],
    ) -> Result<(YoutubeRef, Option<PathBuf>, ContentOutputFormat, u64), AppError> {
        let transcribe = std::iter::once("transcribe").chain(arguments.iter().copied());
        let YoutubeCommand::Transcribe(parsed) = parsed(&transcribe.collect::<Vec<_>>()) else {
            unreachable!("the arguments name youtube transcribe")
        };
        youtube_transcribe_input(parsed).map(|input| {
            (
                input.request.video,
                input.content_dir,
                input.format,
                input.common.timeout,
            )
        })
    }

    fn video() -> YoutubeRef {
        YoutubeRef::video(ZOO).expect("a video ID")
    }

    fn is_argument_error<T: std::fmt::Debug>(result: &Result<T, AppError>) -> bool {
        matches!(result, Err(AppError::Argument(_)))
    }

    #[test]
    fn an_ask_reads_the_video_of_a_short_url_and_its_question_with_the_model_deadline() {
        let asked = ask(&[&format!("https://youtu.be/{ZOO}?t=3"), QUESTION]);

        assert_eq!(
            asked.ok(),
            Some((video(), QUESTION.to_owned(), OutputFormat::Json, 600))
        );
    }

    #[test]
    fn a_transcription_reads_its_video_content_directory_and_format_with_the_model_deadline() {
        let transcribed = transcribe(&[
            &format!("youtube:{ZOO}"),
            "--content-dir",
            "transcripts",
            "--format",
            "content",
        ]);

        assert_eq!(
            transcribed.ok(),
            Some((
                video(),
                Some(PathBuf::from("transcripts")),
                ContentOutputFormat::Content,
                600
            ))
        );
    }

    #[test]
    fn a_blank_question_or_a_collection_is_an_argument_error() {
        let playlist = "https://www.youtube.com/playlist?list=PLZHQObOWTQDPD3MizzM2xVFitgF8hE_ab";
        let asks = [
            ask(&[&format!("youtube:{ZOO}"), "  "]),
            ask(&[playlist, QUESTION]),
        ];
        let transcriptions = [
            transcribe(&[playlist]),
            transcribe(&["youtube:channel/UCExampleMathsChannel001"]),
        ];

        assert!(asks.iter().all(is_argument_error), "{asks:?}");
        assert!(
            transcriptions.iter().all(is_argument_error),
            "{transcriptions:?}"
        );
    }
}
