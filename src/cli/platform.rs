//! The `forager platform <id> <op>` command group: one static argument tree per platform.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::NaiveDate;
use clap::{Args, Subcommand, ValueEnum};

use super::args::{DocsOutputFormat, OutputArgs, OutputFormat};
use super::dispatch::{
    AppError, CommandOutput, NetworkDependencies, invocation_temp_dir, provider_attempt_log,
};
use crate::config::ConfigError;
use crate::platform_chain::{self, PlatformPreflightError, PlatformSearchPlan};
use crate::platform_fetch;
use crate::types::{
    ArxivSearchOptions, ArxivSort, AttemptErrorKind, ContentDepth, Deadline, Platform,
    PlatformFetchRequest, PlatformFetchResult, PlatformRef, PlatformSearchOptions,
    PlatformSearchRequest, ProviderError,
};

const DEFAULT_TIMEOUT_SECONDS: u64 = 120;

#[derive(Debug, Subcommand)]
pub(super) enum PlatformCommand {
    /// Retrieve papers from arXiv.
    Arxiv {
        #[command(subcommand)]
        command: ArxivCommand,
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
    /// Plain keywords that must all match; arXiv query syntax in them is literal text.
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
    #[arg(long, value_enum, default_value_t = DocsOutputFormat::Json)]
    format: DocsOutputFormat,
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
    }
}

fn arxiv_search(arguments: ArxivSearchArgs) -> Result<CommandOutput, AppError> {
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
        SearchInput::Cursor(cursor)
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
        SearchInput::Request(request)
    };
    search(Platform::Arxiv, input, format, &common)
}

enum SearchInput {
    Request(PlatformSearchRequest),
    Cursor(String),
}

fn search(
    platform: Platform,
    input: SearchInput,
    format: OutputFormat,
    common: &PlatformCommonArgs,
) -> Result<CommandOutput, AppError> {
    let dependencies = NetworkDependencies::load()?;
    let config = dependencies.config.platforms.get(platform);
    let plan: PlatformSearchPlan = match input {
        SearchInput::Request(request) => platform_chain::plan_search(config, request),
        SearchInput::Cursor(cursor) => platform_chain::plan_cursor_search(config, &cursor),
    }
    .map_err(preflight_error)?;
    let result = dependencies.runtime.block_on(platform_chain::search(
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
    let request = PlatformFetchRequest {
        reference,
        depth: depth.into(),
    };
    fetch(Platform::Arxiv, request, format, content_dir, &common)
}

fn fetch(
    platform: Platform,
    request: PlatformFetchRequest,
    format: DocsOutputFormat,
    content_dir: Option<PathBuf>,
    common: &PlatformCommonArgs,
) -> Result<CommandOutput, AppError> {
    let dependencies = NetworkDependencies::load()?;
    let full_text = request.depth == ContentDepth::FullText;
    let plan = platform_fetch::plan_fetch(dependencies.config.platforms.get(platform), request)
        .map_err(preflight_error)?;
    let web_fetch = dependencies.config.web_fetch;
    if full_text && web_fetch.configured_provider_count() == 0 {
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
    let result = match result {
        Ok(fetched) if format != DocsOutputFormat::Content => {
            let directory = content_dir.unwrap_or_else(|| invocation_temp_dir("forager-platform"));
            write_content(fetched, &directory, common.verbose)
        }
        result => result,
    };
    let attempt_log = provider_attempt_log(dependencies.config.log_level, &result, |fetched| {
        &fetched.attempts
    });
    Ok(CommandOutput::PlatformFetch {
        result,
        format,
        output: common.output.target(),
        attempt_log,
    })
}

/// Writes the full text, when the result has one, to a Markdown file named after the versioned
/// ref. A write failure never falls back to inline output.
fn write_content(
    mut fetched: PlatformFetchResult,
    directory: &Path,
    verbose: bool,
) -> Result<PlatformFetchResult, ProviderError> {
    let Some(content) = fetched.content.as_mut() else {
        return Ok(fetched);
    };
    let path = directory.join(content_file_name(&fetched.item.reference));
    match fs::create_dir_all(directory).and_then(|()| fs::write(&path, &content.text)) {
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
            redirected_library_id: None,
        }),
    }
}

fn content_file_name(reference: &PlatformRef) -> String {
    format!("{}.md", reference.to_string().replace([':', '/'], "-"))
}

#[cfg(test)]
mod tests {
    use super::{content_file_name, parse_date};
    use crate::types::{Platform, PlatformRef};

    #[test]
    fn content_file_names_derive_from_the_versioned_ref() {
        let names = ["arxiv:2401.01234v2", "arxiv:math.GT/0309136v1"].map(|input| {
            content_file_name(&PlatformRef::parse(Platform::Arxiv, input).expect("valid ref"))
        });

        assert_eq!(
            names,
            ["arxiv-2401.01234v2.md", "arxiv-math.GT-0309136v1.md"]
        );
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
