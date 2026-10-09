//! Binary-side rendering of `forager gemini research` results.

use std::fmt::Write as _;

use forager::app::{DocsOutputFormat, OutputFormat, OutputTarget};
use forager::types::{
    GeminiConversationId, GeminiPlan, GeminiProgress, GeminiReport, GeminiReportFiles,
    GeminiResearchFailure, GeminiResearchResult, GeminiResearchStarted, GeminiResearchState,
};
use serde_json::Value;

use crate::{
    RenderedOutput, apply_tee, encode_failure_payload, failure_payload, postflight_exit_code,
};

/// A rendered result before `--output` handling.
struct Rendered {
    stdout: String,
    is_json: bool,
    exit_code: u8,
    diagnostic: Option<String>,
}

impl Rendered {
    fn success(stdout: String, is_json: bool, diagnostic: Option<String>) -> Self {
        Self {
            stdout,
            is_json,
            exit_code: 0,
            diagnostic,
        }
    }

    fn emit(self, output: Option<OutputTarget>) -> Result<RenderedOutput, String> {
        apply_tee(
            self.stdout,
            self.exit_code,
            self.is_json,
            output,
            self.diagnostic,
        )
    }
}

pub(crate) fn render(
    result: Result<GeminiResearchResult, GeminiResearchFailure>,
    format: DocsOutputFormat,
    output: Option<OutputTarget>,
) -> Result<RenderedOutput, String> {
    let rendered = match result {
        Ok(found) => {
            let (stdout, is_json) = format_result(&found, format)?;
            Rendered::success(stdout, is_json, found.diagnostic)
        }
        Err(failure) => render_failure(failure, format != DocsOutputFormat::Markdown)?,
    };
    rendered.emit(output)
}

pub(crate) fn render_start(
    result: Result<GeminiResearchStarted, GeminiResearchFailure>,
    format: OutputFormat,
) -> Result<RenderedOutput, String> {
    let rendered = match result {
        Ok(started) => {
            let stdout = match format {
                OutputFormat::Json => {
                    serde_json::to_string(&started).map_err(|error| error.to_string())?
                }
                OutputFormat::Markdown => started_markdown(&started),
            };
            Rendered::success(stdout, format == OutputFormat::Json, started.diagnostic)
        }
        Err(failure) => render_failure(failure, format == OutputFormat::Json)?,
    };
    rendered.emit(None)
}

fn render_failure(failure: GeminiResearchFailure, json: bool) -> Result<Rendered, String> {
    let GeminiResearchFailure {
        error,
        conversation,
    } = failure;
    let url = conversation.as_ref().map(GeminiConversationId::url);
    let stdout = if json {
        let mut payload = failure_payload(&error)?;
        payload.insert(
            "conversation_url".into(),
            url.map_or(Value::Null, Value::String),
        );
        encode_failure_payload(&error, &payload)?
    } else {
        let mut markdown = format!(
            "# Gemini Deep Research failed\n\n**{}**: {}",
            error.kind.as_str(),
            error.message
        );
        if let Some(url) = url {
            let _ = write!(markdown, "\n\nConversation: <{url}>");
        }
        markdown
    };
    Ok(Rendered {
        stdout,
        is_json: json,
        exit_code: postflight_exit_code(error.kind),
        diagnostic: error.diagnostic,
    })
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
