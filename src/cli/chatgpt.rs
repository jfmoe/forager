//! The `forager chatgpt` command group: ChatGPT Deep Research in the user's own logged-in
//! Chrome, and delivery of the Delegated Research Report it produces.

use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, Subcommand};

use super::args::{DocsOutputFormat, OutputArgs, OutputFormat};
use super::delegated_report::ReportFiling;
use super::dispatch::{AppError, CommandOutput, NetworkDependencies};
use crate::config::LogLevel;
use crate::providers::{self, ChatgptBrowser};
use crate::types::{
    ChatgptConversationId, ChatgptResearchFailure, ChatgptResearchResult, ChatgptResearchState,
    Deadline, ProviderAttempt, ProviderError,
};

const DEFAULT_START_TIMEOUT_SECONDS: u64 = 240;
const DEFAULT_RESULT_TIMEOUT_SECONDS: u64 = 120;
const REPORT_FILING: ReportFiling = ReportFiling {
    agent: "ChatGPT",
    prefix: "chatgpt",
};

#[derive(Debug, Subcommand)]
pub(super) enum ChatgptCommand {
    /// ChatGPT Deep Research conversations.
    Research {
        #[command(subcommand)]
        command: ChatgptResearchCommand,
    },
}

#[derive(Debug, Subcommand)]
pub(super) enum ChatgptResearchCommand {
    /// Start a Deep Research in a foreground Chrome window: switch the composer to chat mode,
    /// select Deep Research, send the question once, and keep the page open through the plan's
    /// countdown until the research starts. It spends the account's Deep Research quota and is
    /// never retried.
    Start(ChatgptStartArgs),
    /// Read where a Deep Research conversation stands, without changing it; a completed report
    /// and its sources are written to local files.
    Result(ChatgptResultArgs),
}

#[derive(Debug, Args)]
pub(super) struct ChatgptStartArgs {
    /// The research question, sent to ChatGPT as written.
    query: String,
    /// Whole-command deadline in seconds, including waits for the request window.
    #[arg(long, default_value_t = DEFAULT_START_TIMEOUT_SECONDS, value_parser = clap::value_parser!(u64).range(1..))]
    timeout: u64,
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    format: OutputFormat,
}

#[derive(Debug, Args)]
pub(super) struct ChatgptResultArgs {
    /// A `https://chatgpt.com/c/<uuid>` or `https://chatgpt.com/g/<project>/c/<uuid>`
    /// conversation URL, or its `<uuid>`.
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

pub(super) fn run(command: ChatgptCommand) -> Result<CommandOutput, AppError> {
    match command {
        ChatgptCommand::Research {
            command: ChatgptResearchCommand::Start(arguments),
        } => start(arguments),
        ChatgptCommand::Research {
            command: ChatgptResearchCommand::Result(arguments),
        } => result(arguments),
    }
}

fn start(arguments: ChatgptStartArgs) -> Result<CommandOutput, AppError> {
    let ChatgptStartArgs {
        query,
        timeout,
        format,
    } = arguments;
    if query.trim().is_empty() {
        return Err(AppError::Argument(
            "the ChatGPT Deep Research question must not be empty".into(),
        ));
    }
    let session = Session::load(timeout)?;
    let result = session.runtime.block_on(session.provider.start(&query));
    let attempt_log = session.attempt_log(&result, |started| &started.attempts);
    Ok(CommandOutput::ChatgptResearchStart {
        result: Box::new(result),
        format,
        attempt_log,
    })
}

fn result(arguments: ChatgptResultArgs) -> Result<CommandOutput, AppError> {
    let ChatgptResultArgs {
        conversation,
        report_dir,
        timeout,
        format,
        output,
    } = arguments;
    let conversation = ChatgptConversationId::parse(&conversation).map_err(AppError::Argument)?;
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
        .map_err(|error| ChatgptResearchFailure {
            error,
            conversation: Some(conversation),
        });
    let attempt_log = session.attempt_log(&result, |found| &found.attempts);
    Ok(CommandOutput::ChatgptResearch {
        result: Box::new(result),
        format,
        output: output.target(),
        attempt_log,
    })
}

/// The `chatgpt_browser` provider of one command and what running it needs.
struct Session {
    provider: ChatgptBrowser,
    runtime: tokio::runtime::Runtime,
    log_level: LogLevel,
}

impl Session {
    fn load(timeout: u64) -> Result<Self, AppError> {
        let dependencies = NetworkDependencies::load()?;
        Ok(Self {
            provider: providers::build_chatgpt_browser(
                dependencies.config.chatgpt_browser,
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
        result: &'a Result<T, ChatgptResearchFailure>,
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
    mut found: ChatgptResearchResult,
    report_dir: Option<PathBuf>,
) -> Result<ChatgptResearchResult, ProviderError> {
    let ChatgptResearchState::Completed(report) = &found.state else {
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
