//! The `forager platform <id> <op>` command group: one static argument tree per platform.

use std::fs;
use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;

use chrono::NaiveDate;
use clap::{Args, Subcommand, ValueEnum};
use reqwest::Client;

use super::args::{ContentOutputFormat, OutputArgs, OutputFormat};
use super::dispatch::{
    AppError, CommandOutput, NetworkDependencies, Presentation, invocation_temp_dir,
    provider_attempt_log,
};
use crate::catalog::PlatformOperation;
use crate::config::{ConfigError, PlatformRoutesRuntimeConfig, PlatformRuntimeConfig};
use crate::net::RetryPolicy;
use crate::platform_chain::{self, PlatformPreflightError};
use crate::platform_fetch;
use crate::platform_operation;
use crate::types::{
    ASK, ArxivSearchOptions, ArxivSort, AttemptErrorKind, CITED_BY, COMMENTS, ContentDepth,
    Deadline, HISTORY, Platform, PlatformAskRequest, PlatformCommentsRequest, PlatformFetchRequest,
    PlatformFetchResult, PlatformRef, PlatformSearchOptions, PlatformSearchPage,
    PlatformSearchRequest, ProviderError, STORIES, ScholarCitedByRequest, TRANSCRIBE, VIDEOS,
};

#[path = "ssrn_search.rs"]
mod ssrn_search;
use ssrn_search::{SsrnSearchArgs, ssrn_search};

#[path = "scholar.rs"]
mod scholar;
use scholar::ScholarCommand;

#[path = "xiaohongshu.rs"]
mod xiaohongshu;
use xiaohongshu::XiaohongshuCommand;

#[path = "x.rs"]
mod x;
use x::XCommand;

#[path = "hn.rs"]
mod hn;
use hn::HnCommand;

#[path = "polymarket.rs"]
mod polymarket;
use polymarket::PolymarketCommand;

#[path = "reddit.rs"]
mod reddit;
use reddit::RedditCommand;

#[path = "youtube.rs"]
mod youtube;
use youtube::YoutubeCommand;

const DEFAULT_TIMEOUT_SECONDS: u64 = 120;

#[derive(Debug, Subcommand)]
pub(super) enum PlatformCommand {
    /// Preprints in physics, maths, computer science and quantitative finance, with full text;
    /// needs no key.
    Arxiv {
        #[command(subcommand)]
        command: ArxivCommand,
    },
    /// Working papers in finance, economics and law, with their abstracts; needs no key.
    Ssrn {
        #[command(subcommand)]
        command: SsrnCommand,
    },
    /// Papers across publishers on Google Scholar, their citation counts, and the papers citing
    /// a paper; each request spends your SerpApi quota.
    Scholar {
        #[command(subcommand)]
        command: ScholarCommand,
    },
    /// Chinese consumer reviews and lifestyle notes on Xiaohongshu (小红书), read in a browser
    /// logged into your account; off until you enable its route.
    Xiaohongshu {
        #[command(subcommand)]
        command: XiaohongshuCommand,
    },
    /// What accounts post on X (formerly Twitter) now, read by a model with main search's xAI
    /// key; post text is relayed and may differ from the original.
    X {
        #[command(subcommand)]
        command: XCommand,
    },
    /// What practitioners say on Hacker News: searches, whole discussions and the front page;
    /// needs no key.
    Hn {
        #[command(subcommand)]
        command: HnCommand,
    },
    /// Talks, lectures and tutorials on YouTube: videos, comments and playlists with your YouTube
    /// Data API key, captions through an opt-in route, and what a video shows with your Gemini
    /// key.
    Youtube {
        #[command(subcommand)]
        command: YoutubeCommand,
    },
    /// How likely an event is: Polymarket's live market probabilities for each outcome; needs no
    /// key.
    Polymarket {
        #[command(subcommand)]
        command: PolymarketCommand,
    },
    /// User experience and community discussion on Reddit, read in a browser logged into your
    /// account; off until you enable its route.
    Reddit {
        #[command(subcommand)]
        command: RedditCommand,
    },
}

#[derive(Debug, Subcommand)]
pub(super) enum ArxivCommand {
    /// Search arXiv papers; each result carries its metadata and full abstract.
    Search(ArxivSearchArgs),
    /// Fetch one arXiv paper; by default its full text is written to a local Markdown file.
    Fetch(ArxivFetchArgs),
}

#[derive(Debug, Args)]
pub(super) struct ArxivSearchArgs {
    /// Keywords and double-quoted phrases that must all match; arXiv query syntax in them is
    /// literal text.
    #[arg(conflicts_with = "cursor")]
    query: Option<String>,
    /// arXiv category code, for example q-fin.PM; repeat to match any of several.
    #[arg(long = "category", value_name = "CATEGORY", conflicts_with = "cursor")]
    categories: Vec<String>,
    /// Author name phrase.
    #[arg(long, conflicts_with = "cursor")]
    author: Option<String>,
    /// Title phrase.
    #[arg(long, conflicts_with = "cursor")]
    title: Option<String>,
    /// First included submission date (UTC).
    #[arg(long, value_name = "YYYY-MM-DD", value_parser = parse_date, conflicts_with = "cursor")]
    submitted_from: Option<NaiveDate>,
    /// Last included submission date (UTC); the whole day is included.
    #[arg(long, value_name = "YYYY-MM-DD", value_parser = parse_date, conflicts_with = "cursor")]
    submitted_to: Option<NaiveDate>,
    /// Result order; every order is descending.
    #[arg(long, value_enum, default_value_t = ArxivSortArg::Relevance, conflicts_with = "cursor")]
    sort: ArxivSortArg,
    /// Maximum results on this page.
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u16).range(1..=100), conflicts_with = "cursor")]
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
pub(super) struct ArxivFetchArgs {
    /// An `arxiv:<id>[v<n>]` ref, or an arxiv.org abs, pdf, or html URL.
    reference: String,
    /// `full_text` reads the paper body; `abstract` returns only metadata and the abstract.
    #[arg(long, value_enum, default_value_t = ArxivDepthArg::FullText)]
    depth: ArxivDepthArg,
    /// Directory for the full-text Markdown file; defaults to a new directory under the system
    /// temporary directory.
    #[arg(long, value_name = "DIR")]
    content_dir: Option<PathBuf>,
    /// `content` prints the full text (or the abstract) to stdout and writes no file.
    #[arg(long, value_enum, default_value_t = ContentOutputFormat::Json)]
    format: ContentOutputFormat,
    #[command(flatten)]
    common: PlatformCommonArgs,
}

#[derive(Debug, Subcommand)]
pub(super) enum SsrnCommand {
    /// Search SSRN papers by topic; each result carries its metadata and, when available, its
    /// abstract.
    Search(SsrnSearchArgs),
    /// Fetch the metadata of one SSRN paper, with its abstract when available.
    Fetch(SsrnFetchArgs),
}

#[derive(Debug, Args)]
pub(super) struct SsrnFetchArgs {
    /// An `ssrn:<id>` ref, an SSRN abstract page URL, an ssrn.com/abstract=<id> URL, or a
    /// 10.2139/ssrn.<id> DOI or its doi.org URL.
    reference: String,
    /// `metadata` returns the abstract too when Crossref has it; `abstract` requires it.
    #[arg(long, value_enum, default_value_t = SsrnDepthArg::Metadata)]
    depth: SsrnDepthArg,
    /// `content` prints the abstract to stdout.
    #[arg(long, value_enum, default_value_t = ContentOutputFormat::Json)]
    format: ContentOutputFormat,
    #[command(flatten)]
    common: PlatformCommonArgs,
}

#[derive(Debug, Args)]
struct PlatformCommonArgs {
    /// Whole-command deadline in seconds, including waits for the platform request window.
    #[arg(long, default_value_t = DEFAULT_TIMEOUT_SECONDS, value_parser = clap::value_parser!(u64).range(1..))]
    timeout: u64,
    #[command(flatten)]
    output: OutputArgs,
    /// Include every provider attempt, including skipped routes.
    #[arg(long)]
    verbose: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ArxivSortArg {
    Relevance,
    Submitted,
    Updated,
}

impl From<ArxivSortArg> for ArxivSort {
    fn from(value: ArxivSortArg) -> Self {
        match value {
            ArxivSortArg::Relevance => Self::Relevance,
            ArxivSortArg::Submitted => Self::Submitted,
            ArxivSortArg::Updated => Self::Updated,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ArxivDepthArg {
    #[value(name = "full_text")]
    FullText,
    Abstract,
}

impl From<ArxivDepthArg> for ContentDepth {
    fn from(value: ArxivDepthArg) -> Self {
        match value {
            ArxivDepthArg::FullText => Self::FullText,
            ArxivDepthArg::Abstract => Self::Abstract,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum SsrnDepthArg {
    Metadata,
    Abstract,
}

impl From<SsrnDepthArg> for ContentDepth {
    fn from(value: SsrnDepthArg) -> Self {
        match value {
            SsrnDepthArg::Metadata => Self::Metadata,
            SsrnDepthArg::Abstract => Self::Abstract,
        }
    }
}

fn parse_date(value: &str) -> Result<NaiveDate, String> {
    let shaped = value.len() == 10
        && value.bytes().enumerate().all(|(index, byte)| match index {
            4 | 7 => byte == b'-',
            _ => byte.is_ascii_digit(),
        });
    shaped
        .then(|| value.parse().ok())
        .flatten()
        .ok_or_else(|| format!("`{value}` is not a calendar date in YYYY-MM-DD form"))
}

impl PlatformCommand {
    /// The platform and the Platform Operation this command runs.
    pub(super) fn operation(&self) -> (Platform, &'static str) {
        let search = PlatformOperation::Search.as_str();
        let fetch = PlatformOperation::Fetch.as_str();
        match self {
            Self::Arxiv { command } => (
                Platform::Arxiv,
                match command {
                    ArxivCommand::Search(_) => search,
                    ArxivCommand::Fetch(_) => fetch,
                },
            ),
            Self::Ssrn { command } => (
                Platform::Ssrn,
                match command {
                    SsrnCommand::Search(_) => search,
                    SsrnCommand::Fetch(_) => fetch,
                },
            ),
            Self::Scholar { command } => (
                Platform::Scholar,
                match command {
                    ScholarCommand::Search(_) => search,
                    ScholarCommand::Fetch(_) => fetch,
                    ScholarCommand::CitedBy(_) => CITED_BY,
                },
            ),
            Self::Xiaohongshu { command } => (
                Platform::Xiaohongshu,
                match command {
                    XiaohongshuCommand::Search(_) => search,
                    XiaohongshuCommand::Fetch(_) => fetch,
                    XiaohongshuCommand::Comments(_) => COMMENTS,
                },
            ),
            Self::X { command } => (
                Platform::X,
                match command {
                    XCommand::Search(_) => search,
                    XCommand::Fetch(_) => fetch,
                    XCommand::Ask(_) => ASK,
                },
            ),
            Self::Hn { command } => (
                Platform::Hn,
                match command {
                    HnCommand::Search(_) => search,
                    HnCommand::Fetch(_) => fetch,
                    HnCommand::Stories(_) => STORIES,
                },
            ),
            Self::Youtube { command } => (
                Platform::Youtube,
                match command {
                    YoutubeCommand::Search(_) => search,
                    YoutubeCommand::Fetch(_) => fetch,
                    YoutubeCommand::Comments(_) => COMMENTS,
                    YoutubeCommand::Videos(_) => VIDEOS,
                    YoutubeCommand::Ask(_) => ASK,
                    YoutubeCommand::Transcribe(_) => TRANSCRIBE,
                },
            ),
            Self::Polymarket { command } => (
                Platform::Polymarket,
                match command {
                    PolymarketCommand::Search(_) => search,
                    PolymarketCommand::Fetch(_) => fetch,
                    PolymarketCommand::History(_) => HISTORY,
                },
            ),
            Self::Reddit { command } => (
                Platform::Reddit,
                match command {
                    RedditCommand::Search(_) => search,
                    RedditCommand::Fetch(_) => fetch,
                },
            ),
        }
    }
}

pub(super) fn run(command: PlatformCommand) -> Result<CommandOutput, AppError> {
    match command {
        PlatformCommand::Arxiv {
            command: ArxivCommand::Search(arguments),
        } => arxiv_search(arguments),
        PlatformCommand::Arxiv {
            command: ArxivCommand::Fetch(arguments),
        } => arxiv_fetch(arguments),
        PlatformCommand::Ssrn {
            command: SsrnCommand::Search(arguments),
        } => ssrn_search(arguments),
        PlatformCommand::Ssrn {
            command: SsrnCommand::Fetch(arguments),
        } => ssrn_fetch(arguments),
        PlatformCommand::Scholar { command } => scholar::run(command),
        PlatformCommand::Xiaohongshu { command } => xiaohongshu::run(command),
        PlatformCommand::X { command } => x::run(command),
        PlatformCommand::Hn { command } => hn::run(command),
        PlatformCommand::Youtube { command } => youtube::run(command),
        PlatformCommand::Polymarket { command } => polymarket::run(command),
        PlatformCommand::Reddit { command } => reddit::run(command),
    }
}

fn arxiv_search(arguments: ArxivSearchArgs) -> Result<CommandOutput, AppError> {
    let PlatformInput {
        request: input,
        format,
        common,
    } = arxiv_search_input(arguments)?;
    search(Platform::Arxiv, input, format, &common)
}

/// The page an arXiv search asks for, checked before any configuration is read.
fn arxiv_search_input(
    arguments: ArxivSearchArgs,
) -> Result<PlatformInput<PageInput<PlatformSearchRequest>, OutputFormat>, AppError> {
    let ArxivSearchArgs {
        query,
        categories,
        author,
        title,
        submitted_from,
        submitted_to,
        sort,
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
            options: PlatformSearchOptions::Arxiv(ArxivSearchOptions {
                categories,
                author,
                title,
                submitted_from,
                submitted_to,
                sort: sort.into(),
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

/// The page a paged command asks for: a first page, or the page a cursor names.
enum PageInput<R> {
    Request(R),
    Cursor(String),
}

/// A platform command's arguments, checked before any request: what the command asks its
/// route, how its output is rendered, and the options every platform command shares.
struct PlatformInput<R, F> {
    request: R,
    format: F,
    common: PlatformCommonArgs,
}

/// A [`PlatformInput`] whose content can be written to a directory as well as printed.
struct ContentInput<R> {
    request: R,
    format: ContentOutputFormat,
    content_dir: Option<PathBuf>,
    common: PlatformCommonArgs,
}

fn search(
    platform: Platform,
    input: PageInput<PlatformSearchRequest>,
    format: OutputFormat,
    common: &PlatformCommonArgs,
) -> Result<CommandOutput, AppError> {
    run_page(
        platform,
        |config, _| match input {
            PageInput::Request(request) => platform_chain::plan_search(config, request),
            PageInput::Cursor(cursor) => platform_chain::plan_cursor_search(config, &cursor),
        },
        platform_chain::run_search,
        format,
        common,
    )
}

fn scholar_cited_by(
    input: PageInput<ScholarCitedByRequest>,
    format: OutputFormat,
    common: &PlatformCommonArgs,
) -> Result<CommandOutput, AppError> {
    run_page(
        Platform::Scholar,
        |config, routes| match input {
            PageInput::Request(request) => {
                platform_operation::plan_cited_by(config, routes, request)
            }
            PageInput::Cursor(cursor) => {
                platform_operation::plan_cursor_cited_by(config, routes, &cursor)
            }
        },
        platform_operation::run_cited_by,
        format,
        common,
    )
}

/// Plans one page of a paged operation over the platform's configuration and runs it.
fn run_page<Plan, Page>(
    platform: Platform,
    plan: impl FnOnce(
        &PlatformRuntimeConfig,
        &PlatformRoutesRuntimeConfig,
    ) -> Result<Plan, PlatformPreflightError>,
    run: impl FnOnce(Plan, Client, RetryPolicy, Deadline) -> Page,
    format: OutputFormat,
    common: &PlatformCommonArgs,
) -> Result<CommandOutput, AppError>
where
    Page: Future<Output = Result<PlatformSearchPage, ProviderError>>,
{
    let dependencies = NetworkDependencies::load()?;
    let plan = plan(
        dependencies.platform(platform)?,
        &dependencies.config.platform_routes,
    )
    .map_err(preflight_error)?;
    let result = dependencies.runtime.block_on(run(
        plan,
        dependencies.client,
        dependencies.retry_policy,
        Deadline::new(Duration::from_secs(common.timeout)),
    ));
    let attempt_log = provider_attempt_log(dependencies.config.log_level, &result, |page| {
        &page.attempts
    });
    Ok(CommandOutput::PlatformSearch {
        result,
        presentation: Presentation {
            format,
            output: common.output.target(),
            verbose: common.verbose,
            attempt_log,
        },
    })
}

/// Plans the comments read over the request platform's configuration and runs it.
fn comments(
    request: PlatformCommentsRequest,
    format: OutputFormat,
    common: &PlatformCommonArgs,
) -> Result<CommandOutput, AppError> {
    let dependencies = NetworkDependencies::load()?;
    let plan = platform_operation::plan_comments(
        dependencies.platform(request.platform())?,
        &dependencies.config.platform_routes,
        request,
    )
    .map_err(preflight_error)?;
    let result = dependencies
        .runtime
        .block_on(platform_operation::run_comments(
            plan,
            dependencies.client,
            dependencies.retry_policy,
            Deadline::new(Duration::from_secs(common.timeout)),
        ));
    let attempt_log = provider_attempt_log(dependencies.config.log_level, &result, |page| {
        page.attempts()
    });
    Ok(CommandOutput::PlatformComments {
        result,
        presentation: Presentation {
            format,
            output: common.output.target(),
            verbose: common.verbose,
            attempt_log,
        },
    })
}

/// Plans the ask over the request platform's configuration and runs it.
fn ask(
    request: PlatformAskRequest,
    format: OutputFormat,
    common: &PlatformCommonArgs,
) -> Result<CommandOutput, AppError> {
    let platform = request.platform();
    let dependencies = NetworkDependencies::load()?;
    let plan = platform_operation::plan_ask(
        dependencies.platform(platform)?,
        &dependencies.config.platform_routes,
        request,
    )
    .map_err(preflight_error)?;
    let result = dependencies.runtime.block_on(platform_operation::run_ask(
        plan,
        dependencies.client,
        dependencies.retry_policy,
        Deadline::new(Duration::from_secs(common.timeout)),
    ));
    let attempt_log = provider_attempt_log(dependencies.config.log_level, &result, |answer| {
        answer.attempts()
    });
    Ok(CommandOutput::PlatformAsk {
        platform,
        result,
        presentation: Presentation {
            format,
            output: common.output.target(),
            verbose: common.verbose,
            attempt_log,
        },
    })
}

fn preflight_error(error: PlatformPreflightError) -> AppError {
    match error {
        PlatformPreflightError::Argument(message) => AppError::Argument(message),
        PlatformPreflightError::Config(message) => AppError::Config(ConfigError::Message(message)),
    }
}

fn arxiv_fetch(arguments: ArxivFetchArgs) -> Result<CommandOutput, AppError> {
    let ArxivFetchArgs {
        reference,
        depth,
        content_dir,
        format,
        common,
    } = arguments;
    let reference = PlatformRef::parse(Platform::Arxiv, &reference)
        .map_err(|error| AppError::Argument(error.to_string()))?;
    let request = PlatformFetchRequest::new(reference, depth.into());
    fetch(Platform::Arxiv, request, format, content_dir, &common)
}

fn ssrn_fetch(arguments: SsrnFetchArgs) -> Result<CommandOutput, AppError> {
    let PlatformInput {
        request,
        format,
        common,
    } = ssrn_fetch_input(arguments)?;
    fetch(Platform::Ssrn, request, format, None, &common)
}

/// Reads the paper ref before any request.
fn ssrn_fetch_input(
    arguments: SsrnFetchArgs,
) -> Result<PlatformInput<PlatformFetchRequest, ContentOutputFormat>, AppError> {
    let SsrnFetchArgs {
        reference,
        depth,
        format,
        common,
    } = arguments;
    let reference = PlatformRef::parse(Platform::Ssrn, &reference)
        .map_err(|error| AppError::Argument(error.to_string()))?;
    let request = PlatformFetchRequest::new(reference, depth.into());
    Ok(PlatformInput {
        request,
        format,
        common,
    })
}

fn fetch(
    platform: Platform,
    request: PlatformFetchRequest,
    format: ContentOutputFormat,
    content_dir: Option<PathBuf>,
    common: &PlatformCommonArgs,
) -> Result<CommandOutput, AppError> {
    let dependencies = NetworkDependencies::load()?;
    let plan = platform_fetch::plan_fetch(dependencies.platform(platform)?, request)
        .map_err(preflight_error)?;
    let web_fetch = dependencies.config.web_fetch;
    if plan.needs_web_fetch() && web_fetch.configured_provider_count() == 0 {
        return Err(AppError::Config(ConfigError::Message(
            "capabilities.web_fetch.order has no configured provider".into(),
        )));
    }
    let result = dependencies.runtime.block_on(platform_fetch::fetch(
        plan,
        web_fetch,
        dependencies.client,
        dependencies.retry_policy,
        Deadline::new(Duration::from_secs(common.timeout)),
    ));
    let result = result.and_then(|fetched| {
        let file_name = content_file_name(&fetched.item.reference);
        deliver(fetched, format, content_dir, &file_name)
    });
    let attempt_log = provider_attempt_log(dependencies.config.log_level, &result, |fetched| {
        &fetched.attempts
    });
    Ok(CommandOutput::PlatformFetch {
        result: Box::new(result),
        presentation: Presentation {
            format,
            output: common.output.target(),
            verbose: common.verbose,
            attempt_log,
        },
    })
}

/// Delivers a fetched full text. The content format prints it and writes no file; every other
/// format writes it to the Markdown file `file_name` in `content_dir`, or in the invocation's
/// temporary directory when none is given. A write failure is Runtime and never falls back to
/// inline output.
fn deliver(
    mut fetched: PlatformFetchResult,
    format: ContentOutputFormat,
    content_dir: Option<PathBuf>,
    file_name: &str,
) -> Result<PlatformFetchResult, ProviderError> {
    if format == ContentOutputFormat::Content {
        return Ok(fetched);
    }
    let Some(content) = fetched.content.as_mut() else {
        return Ok(fetched);
    };
    let directory = content_dir.unwrap_or_else(|| invocation_temp_dir("forager-platform"));
    let path = directory.join(file_name);
    match fs::create_dir_all(&directory).and_then(|()| fs::write(&path, &content.text)) {
        Ok(()) => {
            content.path = Some(path.display().to_string());
            Ok(fetched)
        }
        Err(error) => Err(ProviderError {
            kind: AttemptErrorKind::Runtime,
            message: format!("cannot write the full text to {}: {error}", path.display()),
            attempts: fetched.attempts,
            diagnostic: fetched.diagnostic,
        }),
    }
}

fn content_file_stem(reference: &PlatformRef) -> String {
    reference.to_string().replace([':', '/'], "-")
}

fn content_file_name(reference: &PlatformRef) -> String {
    format!("{}.md", content_file_stem(reference))
}

#[cfg(test)]
mod tests {
    use super::{content_file_name, deliver, parse_date};
    use crate::app::args::ContentOutputFormat;
    use crate::types::{
        AttemptErrorKind, ContentDepth, HnItemData, HnItemType, Platform, PlatformContent,
        PlatformFetchResult, PlatformItem, PlatformItemData, PlatformRef,
    };

    fn a_thread_with_its_full_text() -> PlatformFetchResult {
        let reference = PlatformRef::parse(Platform::Hn, "hn:8863").expect("valid ref");
        PlatformFetchResult {
            platform: Platform::Hn,
            provider: "hn_api",
            item: PlatformItem {
                url: reference.canonical_url(),
                reference,
                depth: ContentDepth::Thread,
                title: None,
                authors: Vec::new(),
                published: None,
                data: PlatformItemData::Hn(HnItemData {
                    item_type: HnItemType::Story,
                    link_url: None,
                    points: None,
                    comments: None,
                    text: None,
                    story_ref: None,
                    story_title: None,
                    parent_ref: None,
                    comments_delivered: None,
                    order: None,
                }),
            },
            content: Some(PlatformContent::new(
                "https://news.ycombinator.com/item?id=8863".to_owned(),
                "hn_api",
                "# A discussion".to_owned(),
            )),
            attempts: Vec::new(),
            diagnostic: None,
        }
    }

    #[test]
    fn the_content_format_writes_no_file_even_into_a_given_content_directory() {
        let fetched = a_thread_with_its_full_text();
        let content_dir = tempfile::tempdir().expect("content directory");

        let delivered = deliver(
            fetched,
            ContentOutputFormat::Content,
            Some(content_dir.path().to_path_buf()),
            "hn-8863.md",
        )
        .expect("delivered");

        assert_eq!(
            (
                delivered.content.and_then(|content| content.path),
                std::fs::read_dir(content_dir.path())
                    .expect("read content directory")
                    .count()
            ),
            (None, 0)
        );
    }

    #[test]
    fn a_full_text_that_cannot_be_written_is_a_runtime_failure() {
        let blocker = tempfile::NamedTempFile::new().expect("a file where the directory should be");
        let content_dir = blocker.path().join("threads");

        let error = deliver(
            a_thread_with_its_full_text(),
            ContentOutputFormat::Json,
            Some(content_dir.clone()),
            "hn-8863.md",
        )
        .expect_err("an unwritable content directory");

        let prefix = format!(
            "cannot write the full text to {}: ",
            content_dir.join("hn-8863.md").display()
        );
        assert_eq!(
            (error.kind, error.message.starts_with(&prefix)),
            (AttemptErrorKind::Runtime, true),
            "{}",
            error.message
        );
    }

    #[test]
    fn content_file_names_derive_from_the_versioned_ref() {
        let names = ["arxiv:2401.01234v2", "arxiv:math/0309136v1"].map(|input| {
            content_file_name(&PlatformRef::parse(Platform::Arxiv, input).expect("valid ref"))
        });

        assert_eq!(names, ["arxiv-2401.01234v2.md", "arxiv-math-0309136v1.md"]);
    }

    #[test]
    fn dates_must_be_zero_padded_calendar_days() {
        let parsed = [
            "2024-02-29",
            "2023-02-29",
            "2024-2-01",
            "20240201",
            "2024-02-01T00",
        ]
        .map(|value| parse_date(value).is_ok());

        assert_eq!(parsed, [true, false, false, false, false]);
    }
}

/// Parses `platform` followed by `arguments` with the command's own definition, for the tests of
/// each platform's argument rules.
#[cfg(test)]
fn parse_platform(arguments: &[&str]) -> Result<PlatformCommand, clap::Error> {
    #[derive(clap::Parser)]
    struct PlatformCli {
        #[command(subcommand)]
        command: PlatformCommand,
    }

    let arguments = std::iter::once("platform").chain(arguments.iter().copied());
    <PlatformCli as clap::Parser>::try_parse_from(arguments).map(|cli| cli.command)
}

/// The arXiv commands' argument rules, proven by parsing their definition in process.
#[cfg(test)]
mod arxiv_tests {
    use clap::error::ErrorKind;

    use super::{
        ArxivCommand, ArxivSearchArgs, PageInput, PlatformCommand, arxiv_search_input,
        parse_platform,
    };
    use crate::app::AppError;
    use crate::app::args::OutputFormat;

    const CURSOR: &str = "v1.arxiv_api.eyJxdWVyeSI6IngiLCJsaW1pdCI6Miwib3B0aW9ucyI6eyJwbGF0Zm9ybSI6ImFyeGl2In0sInBhZ2UiOiIyIn0";

    fn search_args(arguments: &[&str]) -> Result<ArxivSearchArgs, clap::Error> {
        let search = ["arxiv", "search"]
            .into_iter()
            .chain(arguments.iter().copied());
        match parse_platform(&search.collect::<Vec<_>>())? {
            PlatformCommand::Arxiv {
                command: ArxivCommand::Search(arguments),
            } => Ok(arguments),
            _ => unreachable!("the arguments name arxiv search"),
        }
    }

    #[test]
    fn option_values_outside_their_definition_are_refused_while_parsing() {
        for (arguments, message) in [
            (&["x", "--sort", "newest"][..], "invalid value"),
            (&["x", "--limit", "0"], "invalid value"),
            (&["x", "--limit", "101"], "invalid value"),
            (&["x", "--submitted-from", "2024-13-01"], "calendar date"),
        ] {
            let error = search_args(arguments).expect_err("the parser refuses the value");

            assert!(
                error.to_string().contains(message),
                "arguments: {arguments:?}\n{error}"
            );
        }
    }

    #[test]
    fn searches_the_options_cannot_express_are_argument_errors_before_any_request() {
        for (arguments, message) in [
            (
                &["x", "--category", "cs.AI OR all:x"][..],
                "invalid arXiv category",
            ),
            (&["x", "--author", " "], "--author must contain"),
            (&[r#""world model"#], "unmatched double quote"),
            (
                &[
                    "x",
                    "--submitted-from",
                    "2024-02-01",
                    "--submitted-to",
                    "2024-01-01",
                ],
                "--submitted-from must not be later than --submitted-to",
            ),
            (
                &["--submitted-from", "2024-01-01"],
                "needs a query or at least one of --category, --author, --title",
            ),
        ] {
            let arguments = search_args(arguments).expect("the parser accepts the values");

            let result = arxiv_search_input(arguments).map(|_| ());

            assert!(
                matches!(&result, Err(AppError::Argument(found)) if found.contains(message)),
                "expected an argument error about {message:?}, got {result:?}"
            );
        }
    }

    #[test]
    fn a_cursor_conflicts_with_an_explicit_query_option_or_limit() {
        for arguments in [
            &["x", "--cursor", CURSOR][..],
            &["--cursor", CURSOR, "--category", "cs.AI"],
            &["--cursor", CURSOR, "--sort", "relevance"],
            &["--cursor", CURSOR, "--limit", "10"],
        ] {
            let error = search_args(arguments).expect_err("the parser refuses the combination");

            assert_eq!(
                error.kind(),
                ErrorKind::ArgumentConflict,
                "arguments: {arguments:?}\n{error}"
            );
        }
    }

    #[test]
    fn a_cursor_takes_the_general_flags_and_names_the_page_to_run() {
        let arguments = search_args(&[
            "--cursor",
            CURSOR,
            "--timeout",
            "30",
            "--format",
            "markdown",
        ])
        .expect("the parser accepts general flags beside a cursor");

        let input = arxiv_search_input(arguments).expect("a page to run");
        let PageInput::Cursor(cursor) = input.request else {
            panic!("a search with --cursor runs the page its cursor names");
        };

        assert_eq!(
            (cursor.as_str(), input.format, input.common.timeout),
            (CURSOR, OutputFormat::Markdown, 30)
        );
    }
}

/// The SSRN fetch command's argument rules, proven by parsing its definition in process.
#[cfg(test)]
mod ssrn_tests {
    use clap::error::ErrorKind;

    use super::{PlatformCommand, SsrnCommand, SsrnFetchArgs, parse_platform, ssrn_fetch_input};
    use crate::app::AppError;
    use crate::types::ContentDepth;

    fn fetch_args(arguments: &[&str]) -> Result<SsrnFetchArgs, clap::Error> {
        let fetch = ["ssrn", "fetch"]
            .into_iter()
            .chain(arguments.iter().copied());
        match parse_platform(&fetch.collect::<Vec<_>>())? {
            PlatformCommand::Ssrn {
                command: SsrnCommand::Fetch(arguments),
            } => Ok(arguments),
            _ => unreachable!("the arguments name ssrn fetch"),
        }
    }

    #[test]
    fn a_fetch_reads_the_paper_at_metadata_depth_by_default() {
        let arguments = fetch_args(&["ssrn:2042750"]).expect("the parser accepts a ref");

        let request = ssrn_fetch_input(arguments).expect("a request").request;

        assert_eq!(
            (request.reference.to_string(), request.depth),
            ("ssrn:2042750".to_owned(), ContentDepth::Metadata)
        );
    }

    #[test]
    fn download_links_and_short_links_are_argument_errors_before_any_request() {
        for input in [
            "https://papers.ssrn.com/sol3/Delivery.cfm/SSRN_ID2881657_code1556771.pdf?abstractid=2042750&mirid=1",
            "https://papers.ssrn.com.evil.example/sol3/papers.cfm?abstract_id=2042750",
            "https://bit.ly/abc",
        ] {
            let arguments = fetch_args(&[input]).expect("the parser accepts any reference");

            let result = ssrn_fetch_input(arguments).map(|_| ());

            assert!(
                matches!(&result, Err(AppError::Argument(message)) if message.contains("unrecognized ssrn reference")),
                "{input}: {result:?}"
            );
        }
    }

    #[test]
    fn a_full_text_depth_is_refused_while_parsing() {
        let error = fetch_args(&["ssrn:2042750", "--depth", "full_text"])
            .map(|_| ())
            .expect_err("the parser refuses the depth");

        assert_eq!(
            (
                error.kind(),
                error
                    .to_string()
                    .contains("invalid value 'full_text' for '--depth <DEPTH>'")
            ),
            (ErrorKind::InvalidValue, true),
            "{error}"
        );
    }
}
