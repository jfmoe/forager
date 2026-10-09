//! The `forager gemini` command group: Gemini Deep Research in the user's own logged-in Chrome,
//! and delivery of the Delegated Research Report it produces.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Args, Subcommand};

use super::args::{DocsOutputFormat, OutputArgs, OutputFormat};
use super::dispatch::{
    AppError, CommandOutput, NetworkDependencies, invocation_temp_dir, provider_attempt_log,
};
use crate::providers;
use crate::types::{
    AttemptErrorKind, Deadline, GeminiConversationId, GeminiReport, GeminiReportFiles,
    GeminiResearchFailure, GeminiResearchResult, GeminiResearchState, ProviderError,
};

const DEFAULT_START_TIMEOUT_SECONDS: u64 = 240;
const DEFAULT_RESULT_TIMEOUT_SECONDS: u64 = 120;

#[derive(Debug, Subcommand)]
pub(super) enum GeminiCommand {
    /// Gemini Deep Research conversations.
    Research {
        #[command(subcommand)]
        command: GeminiResearchCommand,
    },
}

#[derive(Debug, Subcommand)]
pub(super) enum GeminiResearchCommand {
    /// Start a Deep Research in a foreground Chrome window: select Deep Research, send the
    /// question once, and confirm the plan Gemini proposes. It spends the account's Deep
    /// Research quota and is never retried.
    Start(GeminiStartArgs),
    /// Read where a Deep Research conversation stands, without changing it; a completed report
    /// and its sources are written to local files.
    Result(GeminiResultArgs),
}

#[derive(Debug, Args)]
pub(super) struct GeminiStartArgs {
    /// The research question, sent to Gemini as written.
    query: String,
    /// Whole-command deadline in seconds, including waits for the request window.
    #[arg(long, default_value_t = DEFAULT_START_TIMEOUT_SECONDS, value_parser = clap::value_parser!(u64).range(1..))]
    timeout: u64,
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    format: OutputFormat,
}

#[derive(Debug, Args)]
pub(super) struct GeminiResultArgs {
    /// A `https://gemini.google.com/app/<id>` conversation URL, or its hexadecimal `<id>`.
    conversation: String,
    /// Directory for the report and sources files; defaults to a new directory under the
    /// system temporary directory.
    #[arg(long, value_name = "DIR")]
    report_dir: Option<PathBuf>,
    /// Whole-command deadline in seconds, including waits for the request window.
    #[arg(long, default_value_t = DEFAULT_RESULT_TIMEOUT_SECONDS, value_parser = clap::value_parser!(u64).range(1..))]
    timeout: u64,
    /// `content` prints a completed report and its sources to stdout and writes no file.
    #[arg(long, value_enum, default_value_t = DocsOutputFormat::Json)]
    format: DocsOutputFormat,
    #[command(flatten)]
    output: OutputArgs,
}

pub(super) fn run(command: GeminiCommand) -> Result<CommandOutput, AppError> {
    match command {
        GeminiCommand::Research {
            command: GeminiResearchCommand::Start(arguments),
        } => start(arguments),
        GeminiCommand::Research {
            command: GeminiResearchCommand::Result(arguments),
        } => result(arguments),
    }
}

fn start(arguments: GeminiStartArgs) -> Result<CommandOutput, AppError> {
    let GeminiStartArgs {
        query,
        timeout,
        format,
    } = arguments;
    if query.trim().is_empty() {
        return Err(AppError::Argument(
            "the Gemini Deep Research question must not be empty".into(),
        ));
    }
    let dependencies = NetworkDependencies::load()?;
    let provider = providers::build_gemini_browser(
        dependencies.config.gemini_browser,
        dependencies.retry_policy,
        Deadline::new(Duration::from_secs(timeout)),
    );
    let result = dependencies.runtime.block_on(provider.start(&query));
    let attempts = match &result {
        Ok(started) => &started.attempts,
        Err(failure) => &failure.error.attempts,
    };
    let attempt_log = crate::attempt_log::render(dependencies.config.log_level, attempts);
    Ok(CommandOutput::GeminiResearchStart {
        result: Box::new(result),
        format,
        attempt_log,
    })
}

fn result(arguments: GeminiResultArgs) -> Result<CommandOutput, AppError> {
    let GeminiResultArgs {
        conversation,
        report_dir,
        timeout,
        format,
        output,
    } = arguments;
    let conversation = GeminiConversationId::parse(&conversation).map_err(AppError::Argument)?;
    let dependencies = NetworkDependencies::load()?;
    let provider = providers::build_gemini_browser(
        dependencies.config.gemini_browser,
        dependencies.retry_policy,
        Deadline::new(Duration::from_secs(timeout)),
    );
    let result = dependencies
        .runtime
        .block_on(provider.result(&conversation))
        .and_then(|found| {
            if format == DocsOutputFormat::Content {
                return Ok(found);
            }
            let directory = report_dir.unwrap_or_else(|| invocation_temp_dir("forager-gemini"));
            deliver(found, &directory)
        });
    let attempt_log = provider_attempt_log(dependencies.config.log_level, &result, |found| {
        &found.attempts
    });
    Ok(CommandOutput::GeminiResearch {
        result: Box::new(result.map_err(|error| GeminiResearchFailure {
            error,
            conversation_url: Some(conversation.url()),
        })),
        format,
        output: output.target(),
        attempt_log,
    })
}

/// Writes a completed report and its sources next to each other. A write failure is Runtime
/// and never falls back to inline output.
fn deliver(
    mut found: GeminiResearchResult,
    directory: &Path,
) -> Result<GeminiResearchResult, ProviderError> {
    let GeminiResearchState::Completed(report) = &mut found.state else {
        return Ok(found);
    };
    let stem = format!("gemini-{}", found.conversation);
    match write_report(report, directory, &stem) {
        Ok(files) => {
            report.files = Some(files);
            Ok(found)
        }
        Err(message) => Err(ProviderError {
            kind: AttemptErrorKind::Runtime,
            message,
            attempts: found.attempts,
            verbose: false,
            diagnostic: found.diagnostic,
            redirected_library_id: None,
        }),
    }
}

fn write_report(
    report: &GeminiReport,
    directory: &Path,
    stem: &str,
) -> Result<GeminiReportFiles, String> {
    let report_path = directory.join(format!("{stem}.md"));
    let sources_path = directory.join(format!("{stem}.sources.json"));
    let failed = |path: &Path, error: std::io::Error| {
        format!(
            "cannot write the Gemini report to {}: {error}",
            path.display()
        )
    };
    fs::create_dir_all(directory).map_err(|error| failed(directory, error))?;
    fs::write(&report_path, report.markdown()).map_err(|error| failed(&report_path, error))?;
    let sources = serde_json::to_vec_pretty(&report.sources)
        .map_err(|error| failed(&sources_path, std::io::Error::other(error)))?;
    fs::write(&sources_path, sources).map_err(|error| failed(&sources_path, error))?;
    Ok(GeminiReportFiles {
        report_path: report_path.display().to_string(),
        sources_path: sources_path.display().to_string(),
    })
}
