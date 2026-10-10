//! The `forager chatgpt` command group: ChatGPT Deep Research in the user's own logged-in
//! Chrome, and delivery of the Delegated Research Report it produces.

use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, Subcommand};

use super::args::{DocsOutputFormat, OutputArgs};
use super::delegated_report::ReportFiling;
use super::dispatch::{AppError, CommandOutput, NetworkDependencies};
use crate::providers;
use crate::types::{
    ChatgptConversationId, ChatgptResearchFailure, ChatgptResearchResult, ChatgptResearchState,
    Deadline, ProviderError,
};

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
    /// Read where a Deep Research conversation stands, without changing it; a completed report
    /// and its sources are written to local files.
    Result(ChatgptResultArgs),
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
            command: ChatgptResearchCommand::Result(arguments),
        } => result(arguments),
    }
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
    let dependencies = NetworkDependencies::load()?;
    let provider = providers::build_chatgpt_browser(
        dependencies.config.chatgpt_browser,
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
            deliver(found, report_dir)
        })
        .map_err(|error| ChatgptResearchFailure {
            error,
            conversation: Some(conversation),
        });
    let attempts = match &result {
        Ok(found) => &found.attempts,
        Err(failure) => &failure.error.attempts,
    };
    let attempt_log = crate::attempt_log::render(dependencies.config.log_level, attempts);
    Ok(CommandOutput::ChatgptResearch {
        result: Box::new(result),
        format,
        output: output.target(),
        attempt_log,
    })
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
