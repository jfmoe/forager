//! Gemini Deep Research: the conversation a research runs in, Gemini's own research plan and
//! progress, and the Delegated Research Report it delivers. None of these is Research Evidence.

use std::fmt;
use std::fmt::Write as _;

use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};

use super::attempt::ProviderAttempt;
use super::error::ProviderError;

/// The attempt-target name of starting a Gemini Deep Research.
pub(crate) const GEMINI_RESEARCH_START: &str = "gemini_research_start";
/// The attempt-target name of reading a Gemini Deep Research conversation.
pub(crate) const GEMINI_RESEARCH_RESULT: &str = "gemini_research_result";

const APP_HOST: &str = "gemini.google.com";
const APP_PATH: &str = "/app/";
// Live conversation ids are 16 hex digits; the bounds only reject inputs that cannot be one.
const MIN_ID_LEN: usize = 8;
const MAX_ID_LEN: usize = 64;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
/// The id of a Gemini web conversation: the hexadecimal last segment of its `/app/<id>` URL.
pub struct GeminiConversationId(String);

impl GeminiConversationId {
    /// Accepts `https://gemini.google.com/app/<id>` or a bare `<id>`.
    ///
    /// # Errors
    ///
    /// Returns a message for any other input.
    pub(crate) fn parse(input: &str) -> Result<Self, String> {
        let input = input.trim();
        let id = if input.contains('/') {
            Self::from_url(input)
        } else {
            Self::from_id(input)
        };
        id.ok_or_else(|| {
            format!(
                "`{input}` is not a Gemini conversation; pass https://{APP_HOST}{APP_PATH}<id> or the hexadecimal <id>"
            )
        })
    }

    /// Returns the conversation of a Gemini page URL, or `None` for any other URL.
    pub(crate) fn from_url(url: &str) -> Option<Self> {
        let url = reqwest::Url::parse(url).ok()?;
        if url.scheme() != "https" || url.host_str() != Some(APP_HOST) {
            return None;
        }
        Self::from_id(url.path().strip_prefix(APP_PATH)?.trim_end_matches('/'))
    }

    fn from_id(id: &str) -> Option<Self> {
        ((MIN_ID_LEN..=MAX_ID_LEN).contains(&id.len())
            && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| Self(id.to_ascii_lowercase()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The conversation's page in the Gemini web app.
    #[must_use]
    pub fn url(&self) -> String {
        format!("https://{APP_HOST}{APP_PATH}{}", self.0)
    }
}

impl fmt::Display for GeminiConversationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
/// The research plan Gemini proposes before it starts. It is unrelated to the Research Plan
/// Schema v1 of `forager research`.
pub struct GeminiPlan {
    pub title: String,
    pub steps: Vec<GeminiPlanStep>,
    /// Gemini's own estimate, such as `Ready in a few mins`.
    pub eta_text: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GeminiPlanStep {
    pub index: u64,
    pub label: String,
    pub description: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
/// How far a running research has come, from the progress the page itself shows.
pub struct GeminiProgress {
    pub sources_visited: usize,
    pub thoughts: usize,
    /// The heading of the newest thought, or `None` before the first one.
    pub latest_thought: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
/// A source the report cites as `[cite: <id>]`.
pub struct GeminiSource {
    pub id: u64,
    pub title: String,
    pub url: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// A completed Delegated Research Report: Gemini's Markdown body, unchanged, and the sources it
/// declares, ordered by citation number.
pub struct GeminiReport {
    pub title: String,
    pub body: String,
    pub sources: Vec<GeminiSource>,
    /// Where the report was written; `None` when it was printed instead.
    pub files: Option<GeminiReportFiles>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeminiReportFiles {
    pub report_path: String,
    pub sources_path: String,
}

impl GeminiReport {
    /// The body followed by a `## Sources` list that resolves every `[cite: N]` without Gemini.
    #[must_use]
    pub fn markdown(&self) -> String {
        let mut markdown = self.body.trim_end().to_owned();
        markdown.push_str("\n\n## Sources\n");
        for source in &self.sources {
            let _ = write!(
                markdown,
                "\n- [{}] {} <{}>",
                source.id, source.title, source.url
            );
        }
        markdown.push('\n');
        markdown
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Where a Deep Research conversation stands.
pub enum GeminiResearchState {
    /// Gemini proposed a plan and waits for the user to start the research.
    AwaitingConfirmation(GeminiPlan),
    Running(GeminiProgress),
    Completed(GeminiReport),
}

#[derive(Clone, Debug)]
/// The state of one Deep Research conversation, as `gemini_browser` read it.
pub struct GeminiResearchResult {
    /// The provider that read the conversation.
    pub route: &'static str,
    pub conversation: GeminiConversationId,
    pub state: GeminiResearchState,
    pub attempts: Vec<ProviderAttempt>,
    pub diagnostic: Option<String>,
}

impl Serialize for GeminiResearchResult {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("route", self.route)?;
        map.serialize_entry("conversation_id", &self.conversation)?;
        map.serialize_entry("conversation_url", &self.conversation.url())?;
        match &self.state {
            GeminiResearchState::AwaitingConfirmation(plan) => {
                map.serialize_entry("status", "awaiting_confirmation")?;
                map.serialize_entry("plan", plan)?;
            }
            GeminiResearchState::Running(progress) => {
                map.serialize_entry("status", "running")?;
                map.serialize_entry("progress", progress)?;
            }
            GeminiResearchState::Completed(report) => {
                map.serialize_entry("status", "completed")?;
                map.serialize_entry("title", &report.title)?;
                if let Some(files) = &report.files {
                    map.serialize_entry("report_path", &files.report_path)?;
                    map.serialize_entry("sources_path", &files.sources_path)?;
                }
                map.serialize_entry("content_len", &report.body.chars().count())?;
                map.serialize_entry("source_count", &report.sources.len())?;
            }
        }
        map.end()
    }
}

#[derive(Debug)]
/// A failed Gemini Deep Research command, with the conversation it concerns when forager knows
/// a checked conversation id, so the user can open it.
pub struct GeminiResearchFailure {
    pub error: ProviderError,
    pub conversation_url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::GeminiConversationId;

    #[test]
    fn a_conversation_is_its_url_or_its_hexadecimal_id() {
        let parsed = [
            "https://gemini.google.com/app/373c21e79b55c71b",
            "https://gemini.google.com/app/373C21E79B55C71B/?hl=zh",
            "373c21e79b55c71b",
            "http://gemini.google.com/app/373c21e79b55c71b",
            "https://gemini.google.com/app",
            "https://example.com/app/373c21e79b55c71b",
            "373c21e79b55c71z",
            "c_373c21e79b55c71b",
            "",
        ]
        .map(|input| GeminiConversationId::parse(input).ok().map(|id| id.0));

        assert_eq!(
            parsed,
            [
                Some("373c21e79b55c71b".to_owned()),
                Some("373c21e79b55c71b".to_owned()),
                Some("373c21e79b55c71b".to_owned()),
                None,
                None,
                None,
                None,
                None,
                None,
            ]
        );
    }
}
