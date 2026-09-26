//! The `forager platform <id> <op>` command group: one static argument tree per platform.

use std::time::Duration;

use chrono::NaiveDate;
use clap::{Args, Subcommand, ValueEnum};

use super::args::{OutputArgs, OutputFormat};
use super::dispatch::{AppError, CommandOutput, NetworkDependencies, provider_attempt_log};
use crate::config::ConfigError;
use crate::platform_chain::{self, PlatformPreflightError, PlatformSearchPlan};
use crate::types::{
    ArxivSearchOptions, ArxivSort, Deadline, Platform, PlatformSearchOptions, PlatformSearchRequest,
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
    #[command(flatten)]
    common: PlatformCommonArgs,
}

#[derive(Debug, Args)]
struct PlatformCommonArgs {
    /// Whole-command deadline in seconds, including waits for the platform request window.
    #[arg(long, default_value_t = DEFAULT_TIMEOUT_SECONDS, value_parser = clap::value_parser!(u64).range(1..))]
    timeout: u64,
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    format: OutputFormat,
    #[command(flatten)]
    output: OutputArgs,
    /// Include every route attempt, including skipped routes.
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
    search(Platform::Arxiv, input, &common)
}

enum SearchInput {
    Request(PlatformSearchRequest),
    Cursor(String),
}

fn search(
    platform: Platform,
    input: SearchInput,
    common: &PlatformCommonArgs,
) -> Result<CommandOutput, AppError> {
    let dependencies = NetworkDependencies::load()?;
    let config = dependencies.config.platforms.get(platform);
    let plan: PlatformSearchPlan = match input {
        SearchInput::Request(request) => platform_chain::plan_search(config, request),
        SearchInput::Cursor(cursor) => platform_chain::plan_cursor_search(config, &cursor),
    }
    .map_err(|error| match error {
        PlatformPreflightError::Argument(message) => AppError::Argument(message),
        PlatformPreflightError::Config(message) => AppError::Config(ConfigError::Message(message)),
    })?;
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
        format: common.format,
        output: common.output.target(),
        attempt_log,
    })
}

#[cfg(test)]
mod tests {
    use super::parse_date;

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
