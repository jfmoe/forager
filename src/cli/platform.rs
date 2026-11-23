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
    AppError, CommandOutput, NetworkDependencies, invocation_temp_dir, provider_attempt_log,
};
use crate::config::{ConfigError, PlatformRoutesRuntimeConfig, PlatformRuntimeConfig};
use crate::net::RetryPolicy;
use crate::platform_chain::{self, PlatformPreflightError};
use crate::platform_fetch;
use crate::platform_operation;
use crate::types::{
    ArxivSearchOptions, ArxivSort, AttemptErrorKind, ContentDepth, Deadline, Platform,
    PlatformAskRequest, PlatformCommentsRequest, PlatformFetchRequest, PlatformFetchResult,
    PlatformRef, PlatformSearchOptions, PlatformSearchPage, PlatformSearchRequest, ProviderError,
    ScholarCitedByRequest,
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
    /// Retrieve papers from arXiv.
    Arxiv {
        #[command(subcommand)]
        command: ArxivCommand,
    },
    /// Retrieve SSRN papers.
    Ssrn {
        #[command(subcommand)]
        command: SsrnCommand,
    },
    /// Find papers across publishers through Google Scholar; needs a SerpApi key.
    Scholar {
        #[command(subcommand)]
        command: ScholarCommand,
    },
    /// Search Xiaohongshu (小红书) notes in a headless browser logged into your account; disabled
    /// until you enable its browser route.
    Xiaohongshu {
        #[command(subcommand)]
        command: XiaohongshuCommand,
    },
    /// Search X (formerly Twitter) posts through xAI's `x_search` tool; uses the xAI key of main
    /// search. Post text is relayed by the model and may differ from the original.
    X {
        #[command(subcommand)]
        command: XCommand,
    },
    /// Search Hacker News, read whole discussions, and list the front page; needs no key.
    Hn {
        #[command(subcommand)]
        command: HnCommand,
    },
    /// Search YouTube videos and read their metadata, captions, comments, and playlists; needs
    /// your own YouTube Data API key. Ask and transcribe need your own Gemini key.
    Youtube {
        #[command(subcommand)]
        command: YoutubeCommand,
    },
    /// Search Polymarket prediction markets and read each outcome's market-implied probability
    /// from the live order book; needs no key.
    Polymarket {
        #[command(subcommand)]
        command: PolymarketCommand,
    },
    /// Search Reddit posts and read their discussions in a headless browser logged into your
    /// account; disabled until you enable its browser route.
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
    let (input, format, common) = arxiv_search_input(arguments)?;
    search(Platform::Arxiv, input, format, &common)
}

/// The page an arXiv search asks for, checked before any configuration is read.
fn arxiv_search_input(
    arguments: ArxivSearchArgs,
) -> Result<
    (
        PageInput<PlatformSearchRequest>,
        OutputFormat,
        PlatformCommonArgs,
    ),
    AppError,
> {
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
    Ok((input, format, common))
}

/// The page a paged command asks for: a first page, or the page a cursor names.
enum PageInput<R> {
    Request(R),
    Cursor(String),
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
    run: impl FnOnce(Plan, Client, RetryPolicy, Deadline, bool) -> Page,
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
        common.verbose,
    ));
    let attempt_log = provider_attempt_log(dependencies.config.log_level, &result, |page| {
        &page.attempts
    });
    Ok(CommandOutput::PlatformSearch {
        result,
        format,
        output: common.output.target(),
        attempt_log,
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
            common.verbose,
        ));
    let attempt_log = provider_attempt_log(dependencies.config.log_level, &result, |page| {
        page.attempts()
    });
    Ok(CommandOutput::PlatformComments {
        result,
        format,
        output: common.output.target(),
        attempt_log,
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
        common.verbose,
    ));
    let attempt_log = provider_attempt_log(dependencies.config.log_level, &result, |answer| {
        answer.attempts()
    });
    Ok(CommandOutput::PlatformAsk {
        platform,
        result,
        format,
        output: common.output.target(),
        attempt_log,
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
    let SsrnFetchArgs {
        reference,
        depth,
        format,
        common,
    } = arguments;
    let reference = PlatformRef::parse(Platform::Ssrn, &reference)
        .map_err(|error| AppError::Argument(error.to_string()))?;
    let request = PlatformFetchRequest::new(reference, depth.into());
    fetch(Platform::Ssrn, request, format, None, &common)
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
        common.verbose,
    ));
    let result = result.and_then(|fetched| {
        let file_name = content_file_name(&fetched.item.reference);
        deliver(fetched, format, content_dir, &file_name, common.verbose)
    });
    let attempt_log = provider_attempt_log(dependencies.config.log_level, &result, |fetched| {
        &fetched.attempts
    });
    Ok(CommandOutput::PlatformFetch {
        result: Box::new(result),
        format,
        output: common.output.target(),
        attempt_log,
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
    verbose: bool,
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
            verbose,
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
        ContentDepth, HnItemData, HnItemType, Platform, PlatformContent, PlatformFetchResult,
        PlatformItem, PlatformItemData, PlatformRef,
    };

    #[test]
    fn the_content_format_writes_no_file_even_into_a_given_content_directory() {
        let reference = PlatformRef::parse(Platform::Hn, "hn:8863").expect("valid ref");
        let fetched = PlatformFetchResult {
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
        };
        let content_dir = tempfile::tempdir().expect("content directory");

        let delivered = deliver(
            fetched,
            ContentOutputFormat::Content,
            Some(content_dir.path().to_path_buf()),
            "hn-8863.md",
            false,
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

        let (input, format, common) = arxiv_search_input(arguments).expect("a page to run");
        let PageInput::Cursor(cursor) = input else {
            panic!("a search with --cursor runs the page its cursor names");
        };

        assert_eq!(
            (cursor.as_str(), format, common.timeout),
            (CURSOR, OutputFormat::Markdown, 30)
        );
    }
}
