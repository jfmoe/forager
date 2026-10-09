//! Binary-side rendering of `forager gemini research` results.

use std::fmt::Write as _;

use forager::app::{DocsOutputFormat, OutputTarget};
use forager::types::{
    GeminiPlan, GeminiProgress, GeminiReport, GeminiResearchFailure, GeminiResearchResult,
    GeminiResearchState,
};
use serde_json::Value;

use crate::{RenderedOutput, apply_tee, format_failure_json, postflight_exit_code};

pub(crate) fn render(
    result: Result<GeminiResearchResult, GeminiResearchFailure>,
    format: DocsOutputFormat,
    output: Option<OutputTarget>,
) -> Result<RenderedOutput, String> {
    let (stdout, is_json, exit_code, diagnostic) = match result {
        Ok(found) => {
            let (stdout, is_json) = format_result(&found, format)?;
            (stdout, is_json, 0, found.diagnostic)
        }
        Err(failure) => {
            let error = &failure.error;
            let (stdout, is_json) = match format {
                DocsOutputFormat::Json | DocsOutputFormat::Content => {
                    (format_failure(&failure)?, true)
                }
                DocsOutputFormat::Markdown => {
                    let mut markdown = format!(
                        "# Gemini Deep Research failed\n\n**{}**: {}",
                        error.kind.as_str(),
                        error.message
                    );
                    if let Some(url) = &failure.conversation_url {
                        let _ = write!(markdown, "\n\nConversation: <{url}>");
                    }
                    (markdown, false)
                }
            };
            (
                stdout,
                is_json,
                postflight_exit_code(error.kind),
                failure.error.diagnostic,
            )
        }
    };
    apply_tee(stdout, exit_code, is_json, output, diagnostic)
}

/// Returns the rendered result and whether it is JSON. Only a completed report has content to
/// print; `content` renders every other state as JSON.
fn format_result(
    found: &GeminiResearchResult,
    format: DocsOutputFormat,
) -> Result<(String, bool), String> {
    let json = || serde_json::to_string(found).map_err(|error| error.to_string());
    match (format, &found.state) {
        (DocsOutputFormat::Content, GeminiResearchState::Completed(report)) => {
            Ok((report.markdown(), false))
        }
        (DocsOutputFormat::Json | DocsOutputFormat::Content, _) => Ok((json()?, true)),
        (DocsOutputFormat::Markdown, state) => {
            Ok((format_markdown(&found.conversation.url(), state), false))
        }
    }
}

fn format_markdown(url: &str, state: &GeminiResearchState) -> String {
    match state {
        GeminiResearchState::AwaitingConfirmation(plan) => awaiting_markdown(url, plan),
        GeminiResearchState::Running(progress) => running_markdown(url, progress),
        GeminiResearchState::Completed(report) => completed_markdown(url, report),
    }
}

fn awaiting_markdown(url: &str, plan: &GeminiPlan) -> String {
    let mut markdown = format!(
        "# Gemini Deep Research: awaiting confirmation\n\nConversation: <{url}>\n\n## Plan: {}\n",
        plan.title
    );
    for step in &plan.steps {
        let _ = write!(
            markdown,
            "\n{}. **{}** — {}",
            step.index, step.label, step.description
        );
    }
    if let Some(eta) = &plan.eta_text {
        let _ = write!(markdown, "\n\n{eta}");
    }
    markdown
        .push_str("\n\nStart the research on the conversation page, then read the result again.");
    markdown
}

fn running_markdown(url: &str, progress: &GeminiProgress) -> String {
    let mut markdown = format!(
        "# Gemini Deep Research: running\n\nConversation: <{url}>\n\nSources visited: {}; thoughts: {}",
        progress.sources_visited, progress.thoughts
    );
    if let Some(thought) = &progress.latest_thought {
        let _ = write!(markdown, "; latest thought: {thought}");
    }
    markdown
}

fn completed_markdown(url: &str, report: &GeminiReport) -> String {
    let mut markdown = format!(
        "# {}\n\nGemini Deep Research report of <{url}>: {} characters, {} sources",
        report.title,
        report.body.chars().count(),
        report.sources.len()
    );
    if let Some(files) = &report.files {
        let _ = write!(
            markdown,
            "\n\nReport: `{}`\n\nSources: `{}`",
            files.report_path, files.sources_path
        );
    }
    markdown
}

/// The stable failure payload plus the conversation it concerns.
fn format_failure(failure: &GeminiResearchFailure) -> Result<String, String> {
    let mut payload: Value = serde_json::from_str(&format_failure_json(&failure.error)?)
        .map_err(|error| error.to_string())?;
    payload
        .as_object_mut()
        .ok_or_else(|| "failure payload is not a JSON object".to_owned())?
        .insert(
            "conversation_url".into(),
            failure
                .conversation_url
                .clone()
                .map_or(Value::Null, Value::String),
        );
    serde_json::to_string(&payload).map_err(|error| error.to_string())
}
