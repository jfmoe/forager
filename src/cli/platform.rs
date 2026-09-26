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
use crate::net::combine_diagnostics;
use crate::platform_chain::{self, PlatformPreflightError, PlatformSearchPlan};
use crate::platform_fetch;
use crate::types::{
    ArxivSearchOptions, ArxivSort, AttemptErrorKind, ContentDepth, Deadline, LocalFile, Platform,
    PlatformFetchRequest, PlatformFetchResult, PlatformRef, PlatformSearchOptions,
    PlatformSearchRequest, ProviderError, SsrnSearchOptions,
};

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

#[derive(Debug, Subcommand)]
pub(super) enum SsrnCommand {
    /// Search SSRN papers by topic; each result carries its metadata and, when available, its
    /// abstract.
    Search(SsrnSearchArgs),
    /// Fetch the metadata of one SSRN paper, with its abstract when available.
    Fetch(SsrnFetchArgs),
}

#[derive(Debug, Args)]
pub(super) struct SsrnSearchArgs {
    /// Topic keywords, ranked by relevance.
    #[arg(conflicts_with = "cursor")]
    query: Option<String>,
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
pub(super) struct SsrnFetchArgs {
    /// An `ssrn:<id>` ref, an SSRN abstract page URL, an ssrn.com/abstract=<id> URL, or a
    /// 10.2139/ssrn.<id> DOI or its doi.org URL.
    reference: String,
    /// `metadata` returns the abstract too when a route has it; `abstract` requires it;
    /// `full_text` downloads the paper and delivers it as Markdown (needs the browser route).
    #[arg(long, value_enum, default_value_t = SsrnDepthArg::Metadata)]
    depth: SsrnDepthArg,
    /// Directory for the full-text Markdown file; defaults to a new directory under the system
    /// temporary directory.
    #[arg(long, value_name = "DIR")]
    content_dir: Option<PathBuf>,
    /// Keep the downloaded PDF next to the Markdown and report its path and size.
    #[arg(long)]
    keep_pdf: bool,
    /// `content` prints the abstract or the full text to stdout.
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

#[derive(Clone, Copy, Debug, ValueEnum)]
enum SsrnDepthArg {
    Metadata,
    Abstract,
    #[value(name = "full_text")]
    FullText,
}

impl From<SsrnDepthArg> for ContentDepth {
    fn from(value: SsrnDepthArg) -> Self {
        match value {
            SsrnDepthArg::Metadata => Self::Metadata,
            SsrnDepthArg::Abstract => Self::Abstract,
            SsrnDepthArg::FullText => Self::FullText,
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

fn ssrn_search(arguments: SsrnSearchArgs) -> Result<CommandOutput, AppError> {
    let SsrnSearchArgs {
        query,
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
            options: PlatformSearchOptions::Ssrn(SsrnSearchOptions::default()),
            page: None,
        };
        request.validate().map_err(AppError::Argument)?;
        SearchInput::Request(request)
    };
    search(Platform::Ssrn, input, format, &common)
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
    fetch(
        Platform::Arxiv,
        request,
        format,
        content_dir,
        false,
        &common,
    )
}

fn ssrn_fetch(arguments: SsrnFetchArgs) -> Result<CommandOutput, AppError> {
    let SsrnFetchArgs {
        reference,
        depth,
        content_dir,
        keep_pdf,
        format,
        common,
    } = arguments;
    let reference = PlatformRef::parse(Platform::Ssrn, &reference)
        .map_err(|error| AppError::Argument(error.to_string()))?;
    let request = PlatformFetchRequest {
        reference,
        depth: depth.into(),
    };
    fetch(
        Platform::Ssrn,
        request,
        format,
        content_dir,
        keep_pdf,
        &common,
    )
}

fn fetch(
    platform: Platform,
    request: PlatformFetchRequest,
    format: DocsOutputFormat,
    content_dir: Option<PathBuf>,
    keep_pdf: bool,
    common: &PlatformCommonArgs,
) -> Result<CommandOutput, AppError> {
    let dependencies = NetworkDependencies::load()?;
    let full_text = request.depth == ContentDepth::FullText;
    let reference = request.reference.clone();
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
        Ok(fetched) => {
            let has_content = fetched.content.is_some();
            let has_source = fetched
                .content
                .as_ref()
                .is_some_and(|content| content.source_file.is_some());
            if !has_content || (format == DocsOutputFormat::Content && !has_source) {
                Ok(fetched)
            } else {
                let directory =
                    content_dir.unwrap_or_else(|| invocation_temp_dir("forager-platform"));
                deliver(
                    fetched,
                    &directory,
                    keep_pdf,
                    format != DocsOutputFormat::Content,
                    common.verbose,
                )
            }
        }
        Err(failure) => {
            let mut error = failure.error;
            // The conversion failed after the route produced a file: keep the file so the
            // user can still read the paper, and say where it is.
            if let Some(source) = failure.source_file {
                let directory =
                    content_dir.unwrap_or_else(|| invocation_temp_dir("forager-platform"));
                error.message = format!(
                    "{}; {}",
                    error.message,
                    keep_source_file(&source, &directory, &reference)
                );
            }
            Err(error)
        }
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

/// Delivers a fetched full text: writes the Markdown file unless the format is `content`, then
/// keeps the local source file when asked and removes it otherwise. A write failure is Runtime
/// and never falls back to inline output.
fn deliver(
    mut fetched: PlatformFetchResult,
    directory: &Path,
    keep_pdf: bool,
    write_markdown: bool,
    verbose: bool,
) -> Result<PlatformFetchResult, ProviderError> {
    if fetched.content.is_none() {
        return Ok(fetched);
    }
    if write_markdown {
        let path = directory.join(content_file_name(&fetched.item.reference));
        let text = &fetched.content.as_ref().expect("content checked").text;
        if let Err(error) = fs::create_dir_all(directory).and_then(|()| fs::write(&path, text)) {
            let mut message = format!("cannot write the full text to {}: {error}", path.display());
            // The conversion succeeded but the write failed; keep the source file so the
            // paper is not lost.
            if let Some(source) = fetched
                .content
                .as_mut()
                .and_then(|content| content.source_file.take())
            {
                message = format!(
                    "{message}; {}",
                    keep_source_file(&source, directory, &fetched.item.reference)
                );
            }
            return Err(delivery_failure(fetched, message, verbose));
        }
        fetched.content.as_mut().expect("content checked").path = Some(path.display().to_string());
    }
    let source = fetched
        .content
        .as_mut()
        .and_then(|content| content.source_file.take());
    if let Some(source) = source {
        if keep_pdf {
            match keep_source_in(&source, directory, &fetched.item.reference) {
                Ok((target, bytes)) => {
                    let content = fetched.content.as_mut().expect("content checked");
                    content.pdf_path = Some(target.display().to_string());
                    content.pdf_bytes = Some(bytes);
                }
                Err(error) => {
                    let target = directory.join(source_file_name(&fetched.item.reference, &source));
                    return Err(delivery_failure(
                        fetched,
                        format!(
                            "cannot keep the downloaded file as {}: {error}",
                            target.display()
                        ),
                        verbose,
                    ));
                }
            }
        } else if let Err(error) = fs::remove_file(&source.path) {
            // A leftover download is not a failure; say where it is.
            fetched.diagnostic = combine_diagnostics(
                [
                    fetched.diagnostic.take(),
                    Some(format!(
                        "cannot delete the downloaded file {}: {error}",
                        source.path.display()
                    )),
                ]
                .into_iter()
                .flatten(),
            );
        }
    }
    Ok(fetched)
}

/// Moves the downloaded source file next to the Markdown and says where it ended up: the kept
/// path, or the original path when the move fails.
fn keep_source_file(source: &LocalFile, directory: &Path, reference: &PlatformRef) -> String {
    match keep_source_in(source, directory, reference) {
        Ok((target, _)) => format!("the downloaded file is kept at {}", target.display()),
        Err(error) => format!(
            "the downloaded file remains at {} (cannot move it: {error})",
            source.path.display()
        ),
    }
}

/// Creates the content directory when needed and moves the source file into it under its
/// ref-derived name. Returns the target path and the byte count.
fn keep_source_in(
    source: &LocalFile,
    directory: &Path,
    reference: &PlatformRef,
) -> std::io::Result<(PathBuf, u64)> {
    let target = directory.join(source_file_name(reference, source));
    fs::create_dir_all(directory)?;
    let bytes = move_file(&source.path, &target)?;
    Ok((target, bytes))
}

/// Moves `source` to `target`, across filesystems by copy-then-delete. Returns the byte count.
fn move_file(source: &Path, target: &Path) -> std::io::Result<u64> {
    let bytes = fs::metadata(source)?.len();
    if fs::rename(source, target).is_ok() {
        return Ok(bytes);
    }
    fs::copy(source, target)?;
    fs::remove_file(source)?;
    Ok(bytes)
}

fn delivery_failure(fetched: PlatformFetchResult, message: String, verbose: bool) -> ProviderError {
    ProviderError {
        kind: AttemptErrorKind::Runtime,
        message,
        attempts: fetched.attempts,
        verbose,
        diagnostic: fetched.diagnostic,
        redirected_library_id: None,
    }
}

fn content_file_stem(reference: &PlatformRef) -> String {
    reference.to_string().replace([':', '/'], "-")
}

fn content_file_name(reference: &PlatformRef) -> String {
    format!("{}.md", content_file_stem(reference))
}

fn source_file_name(reference: &PlatformRef, source: &LocalFile) -> String {
    format!(
        "{}.{}",
        content_file_stem(reference),
        source.media_type.extension()
    )
}

#[cfg(test)]
mod tests {
    use super::{content_file_name, parse_date};
    use crate::types::{Platform, PlatformRef};

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
