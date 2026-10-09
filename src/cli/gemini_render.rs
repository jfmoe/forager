//! Binary-side rendering of `forager gemini research` results.

use std::fmt::Write as _;

use forager::app::{DocsOutputFormat, OutputFormat, OutputTarget};
use forager::types::{
    GeminiPlan, GeminiProgress, GeminiReport, GeminiReportFiles, GeminiResearchFailure,
    GeminiResearchResult, GeminiResearchStarted, GeminiResearchState,
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
        Err(failure) => render_failure(failure, format != DocsOutputFormat::Markdown)?,
    };
    apply_tee(stdout, exit_code, is_json, output, diagnostic)
}

pub(crate) fn render_start(
    result: Result<GeminiResearchStarted, GeminiResearchFailure>,
    format: OutputFormat,
) -> Result<RenderedOutput, String> {
    let (stdout, is_json, exit_code, diagnostic) = match result {
        Ok(started) => {
            let stdout = match format {
                OutputFormat::Json => {
                    serde_json::to_string(&started).map_err(|error| error.to_string())?
                }
                OutputFormat::Markdown => started_markdown(&started),
            };
            (stdout, format == OutputFormat::Json, 0, started.diagnostic)
        }
        Err(failure) => render_failure(failure, format == OutputFormat::Json)?,
    };
    apply_tee(stdout, exit_code, is_json, None, diagnostic)
}

/// Returns the rendered failure, whether it is JSON, its exit code, and its diagnostic.
fn render_failure(
    failure: GeminiResearchFailure,
    json: bool,
) -> Result<(String, bool, u8, Option<String>), String> {
    let error = &failure.error;
    let stdout = if json {
        format_failure(&failure)?
    } else {
        let mut markdown = format!(
            "# Gemini Deep Research failed\n\n**{}**: {}",
            error.kind.as_str(),
            error.message
        );
        if let Some(url) = &failure.conversation_url {
            let _ = write!(markdown, "\n\nConversation: <{url}>");
        }
        markdown
    };
    Ok((
        stdout,
        json,
        postflight_exit_code(error.kind),
        failure.error.diagnostic,
    ))
}

fn started_markdown(started: &GeminiResearchStarted) -> String {
    let url = started.conversation.url();
    let mut markdown = format!(
        "# Gemini Deep Research started\n\nConversation: <{url}>\n\n{}",
        plan_markdown(&started.plan)
    );
    let _ = write!(
        markdown,
        "\n\nRead where it stands with `forager gemini research result {url}`."
    );
    markdown
}

fn plan_markdown(plan: &GeminiPlan) -> String {
    let mut markdown = format!("## Plan: {}\n", plan.title);
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
        (DocsOutputFormat::Markdown, _) => Ok((format_markdown(found), false)),
    }
}

fn format_markdown(found: &GeminiResearchResult) -> String {
    let url = found.conversation.url();
    match &found.state {
        GeminiResearchState::AwaitingConfirmation(plan) => awaiting_markdown(&url, plan),
        GeminiResearchState::Running(progress) => running_markdown(&url, progress),
        GeminiResearchState::Completed(report) => {
            completed_markdown(&url, report, found.report_files.as_ref())
        }
    }
}

fn awaiting_markdown(url: &str, plan: &GeminiPlan) -> String {
    let mut markdown = format!(
        "# Gemini Deep Research: awaiting confirmation\n\nConversation: <{url}>\n\n{}",
        plan_markdown(plan)
    );
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

fn completed_markdown(
    url: &str,
    report: &GeminiReport,
    files: Option<&GeminiReportFiles>,
) -> String {
    let mut markdown = format!(
        "# {}\n\nGemini Deep Research report of <{url}>: {} characters, {} sources",
        report.title,
        report.body.chars().count(),
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
