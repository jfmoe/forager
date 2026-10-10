//! Binary-side rendering of `forager chatgpt research` results.

use std::fmt::Write as _;

use forager::app::{DocsOutputFormat, OutputTarget};
use forager::types::{
    ChatgptConversationId, ChatgptPlan, ChatgptProgress, ChatgptResearchFailure,
    ChatgptResearchResult, ChatgptResearchState, DelegatedReport, ReportFiles,
};

use crate::RenderedOutput;
use crate::delegated_render::Rendered;

pub(crate) fn render(
    result: Result<ChatgptResearchResult, ChatgptResearchFailure>,
    format: DocsOutputFormat,
    output: Option<OutputTarget>,
) -> Result<RenderedOutput, String> {
    let rendered = match result {
        Ok(found) => {
            let (stdout, is_json) = format_result(&found, format)?;
            Rendered::success(stdout, is_json, found.diagnostic)
        }
        Err(failure) => {
            let url = failure
                .conversation
                .as_ref()
                .map(ChatgptConversationId::url);
            Rendered::failure(
                "ChatGPT",
                failure.error,
                url,
                format != DocsOutputFormat::Markdown,
            )?
        }
    };
    rendered.emit(output)
}

/// Returns the rendered result and whether it is JSON. Only a completed report has content to
/// print; `content` renders every other state as JSON.
fn format_result(
    found: &ChatgptResearchResult,
    format: DocsOutputFormat,
) -> Result<(String, bool), String> {
    let json = || serde_json::to_string(found).map_err(|error| error.to_string());
    match (format, &found.state) {
        (DocsOutputFormat::Content, ChatgptResearchState::Completed(report)) => {
            Ok((report.markdown(), false))
        }
        (DocsOutputFormat::Json | DocsOutputFormat::Content, _) => Ok((json()?, true)),
        (DocsOutputFormat::Markdown, _) => Ok((format_markdown(found), false)),
    }
}

fn format_markdown(found: &ChatgptResearchResult) -> String {
    let url = found.conversation.url();
    match &found.state {
        ChatgptResearchState::AwaitingConfirmation(plan) => awaiting_markdown(&url, plan),
        ChatgptResearchState::Running {
            plan_title,
            progress,
        } => running_markdown(&url, plan_title, progress),
        ChatgptResearchState::Completed(report) => {
            completed_markdown(&url, report, found.report_files.as_ref())
        }
    }
}

fn awaiting_markdown(url: &str, plan: &ChatgptPlan) -> String {
    let mut markdown = format!(
        "# ChatGPT Deep Research: awaiting confirmation\n\nConversation: <{url}>\n\n## Plan: {}\n",
        plan.title
    );
    for step in &plan.steps {
        let _ = write!(markdown, "\n{}. {}", step.index, step.text);
    }
    markdown.push_str(
        "\n\nStart the research on the conversation page, or wait for its countdown, then read the result again.",
    );
    markdown
}

fn running_markdown(url: &str, plan_title: &str, progress: &ChatgptProgress) -> String {
    format!(
        "# ChatGPT Deep Research: running\n\nConversation: <{url}>\n\nPlan: {plan_title}; steps completed: {} of {}",
        progress.steps_completed, progress.steps_total
    )
}

fn completed_markdown(url: &str, report: &DelegatedReport, files: Option<&ReportFiles>) -> String {
    let mut markdown = format!(
        "# {}\n\nChatGPT Deep Research report of <{url}>: {} characters, {} sources",
        report.title,
        report.content_len(),
        report.sources.len()
    );
    if let Some(files) = files {
        let _ = write!(
            markdown,
            "\n\nReport: `{}`\n\nSources: `{}`",
            files.report_path, files.sources_path
        );
    }
    markdown
}
