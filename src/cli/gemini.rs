//! The `forager gemini` command group: Gemini Deep Research in the user's own logged-in Chrome,
//! and delivery of the Delegated Research Report it produces.

use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, Subcommand};

use super::args::{DocsOutputFormat, OutputArgs, OutputFormat};
use super::delegated_report::ReportFiling;
use super::dispatch::{AppError, CommandOutput, NetworkDependencies};
use crate::config::LogLevel;
use crate::providers::{self, GeminiBrowser};
use crate::types::{
    Deadline, GeminiConversationId, GeminiResearchFailure, GeminiResearchResult,
    GeminiResearchState, ProviderAttempt, ProviderError,
};

const DEFAULT_START_TIMEOUT_SECONDS: u64 = 240;
const DEFAULT_RESULT_TIMEOUT_SECONDS: u64 = 120;
const REPORT_FILING: ReportFiling = ReportFiling {
    agent: "Gemini",
    prefix: "gemini",
};

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
    let session = Session::load(timeout)?;
    let result = session.runtime.block_on(session.provider.start(&query));
    let attempt_log = session.attempt_log(&result, |started| &started.attempts);
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
    let session = Session::load(timeout)?;
    let result = session
        .runtime
        .block_on(session.provider.result(&conversation))
        .and_then(|found| {
            if format == DocsOutputFormat::Content {
                return Ok(found);
            }
            deliver(found, report_dir)
        })
        .map_err(|error| GeminiResearchFailure {
            error,
            conversation: Some(conversation),
        });
    let attempt_log = session.attempt_log(&result, |found| &found.attempts);
    Ok(CommandOutput::GeminiResearch {
        result: Box::new(result),
        format,
        output: output.target(),
        attempt_log,
    })
}

/// The `gemini_browser` provider of one command and what running it needs.
struct Session {
    provider: GeminiBrowser,
    runtime: tokio::runtime::Runtime,
    log_level: LogLevel,
}

impl Session {
    fn load(timeout: u64) -> Result<Self, AppError> {
        let dependencies = NetworkDependencies::load()?;
        Ok(Self {
            provider: providers::build_gemini_browser(
                dependencies.config.gemini_browser,
                dependencies.retry_policy,
                Deadline::new(Duration::from_secs(timeout)),
            ),
            runtime: dependencies.runtime,
            log_level: dependencies.config.log_level,
        })
    }

    /// The terminal projection of the attempts behind `result`, as `log.level` selects.
    fn attempt_log<'a, T>(
        &self,
        result: &'a Result<T, GeminiResearchFailure>,
        success_attempts: impl FnOnce(&'a T) -> &'a [ProviderAttempt],
    ) -> Option<String> {
        let attempts = match result {
            Ok(value) => success_attempts(value),
            Err(failure) => &failure.error.attempts,
        };
        crate::attempt_log::render(self.log_level, attempts)
    }
}

/// Writes a completed report and its sources, unless the research is unfinished.
fn deliver(
    mut found: GeminiResearchResult,
    report_dir: Option<PathBuf>,
) -> Result<GeminiResearchResult, ProviderError> {
    let GeminiResearchState::Completed(report) = &found.state else {
        return Ok(found);
    };
    let files = REPORT_FILING.write(
        found.conversation.as_str(),
        report,
        report_dir,
        &mut found.attempts,
        &mut found.diagnostic,
    )?;
    found.report_files = Some(files);
    Ok(found)
}
