//! ChatGPT Deep Research: the conversation a research runs in, ChatGPT's own research plan and
//! progress, and the Delegated Research Report it delivers. None of these is Research Evidence.

use std::fmt;

use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};

use super::attempt::ProviderAttempt;
use super::delegated_report::{DelegatedReport, ReportFiles};
use super::error::ProviderError;

/// The attempt-target name of starting a ChatGPT Deep Research.
pub(crate) const CHATGPT_RESEARCH_START: &str = "chatgpt_research_start";
/// The attempt-target name of reading a ChatGPT Deep Research conversation.
pub(crate) const CHATGPT_RESEARCH_RESULT: &str = "chatgpt_research_result";

const APP_HOST: &str = "chatgpt.com";
const UUID_GROUPS: [usize; 5] = [8, 4, 4, 4, 12];

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
/// The id of a ChatGPT conversation: the lowercase UUID in its `/c/<uuid>` URL.
pub struct ChatgptConversationId(String);

impl ChatgptConversationId {
    /// Accepts `https://chatgpt.com/c/<uuid>`, `https://chatgpt.com/g/<project>/c/<uuid>`, or a
    /// bare `<uuid>`, in any letter case.
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
                "`{input}` is not a ChatGPT conversation; pass https://{APP_HOST}/c/<uuid>, https://{APP_HOST}/g/<project>/c/<uuid>, or the <uuid>"
            )
        })
    }

    /// Returns the conversation of a ChatGPT page URL, or `None` for any other URL.
    pub(crate) fn from_url(url: &str) -> Option<Self> {
        let url = reqwest::Url::parse(url).ok()?;
        if url.scheme() != "https" || url.host_str() != Some(APP_HOST) {
            return None;
        }
        let segments = url.path_segments()?.collect::<Vec<_>>();
        let segments = segments.strip_suffix(&[""]).unwrap_or(&segments);
        match segments {
            ["c", id] => Self::from_id(id),
            ["g", project, "c", id] if !project.is_empty() => Self::from_id(id),
            _ => None,
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        let groups = id.split('-').collect::<Vec<_>>();
        let is_uuid = groups.len() == UUID_GROUPS.len()
            && groups.iter().zip(UUID_GROUPS).all(|(group, length)| {
                group.len() == length && group.bytes().all(|byte| byte.is_ascii_hexdigit())
            });
        is_uuid.then(|| Self(id.to_ascii_lowercase()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The conversation's page in the ChatGPT web app.
    #[must_use]
    pub fn url(&self) -> String {
        format!("https://{APP_HOST}/c/{}", self.0)
    }
}

impl fmt::Display for ChatgptConversationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
/// The research plan ChatGPT proposes before it starts. It is unrelated to the Research Plan
/// Schema v1 of `forager research`.
pub struct ChatgptPlan {
    pub title: String,
    pub steps: Vec<ChatgptPlanStep>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ChatgptPlanStep {
    /// The step's position in the plan, from 1.
    pub index: usize,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
/// How far a running research has come: the steps of the current plan that ChatGPT marks
/// completed.
pub struct ChatgptProgress {
    pub steps_completed: usize,
    pub steps_total: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Where a ChatGPT Deep Research conversation stands.
pub enum ChatgptResearchState {
    /// ChatGPT proposed a plan and waits for the user to start the research.
    AwaitingConfirmation(ChatgptPlan),
    Running {
        plan_title: String,
        progress: ChatgptProgress,
    },
    /// The report, with ChatGPT's citation markers replaced by `[N]`.
    Completed(DelegatedReport),
}

#[derive(Clone, Debug)]
/// The state of one Deep Research conversation, as `chatgpt_browser` read it.
pub struct ChatgptResearchResult {
    /// The provider that read the conversation.
    pub route: &'static str,
    pub conversation: ChatgptConversationId,
    pub state: ChatgptResearchState,
    /// Where the completed report was written; `None` before delivery or when it was printed.
    pub report_files: Option<ReportFiles>,
    pub attempts: Vec<ProviderAttempt>,
    pub diagnostic: Option<String>,
}

impl Serialize for ChatgptResearchResult {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("route", self.route)?;
        map.serialize_entry("conversation_id", &self.conversation)?;
        map.serialize_entry("conversation_url", &self.conversation.url())?;
        match &self.state {
            ChatgptResearchState::AwaitingConfirmation(plan) => {
                map.serialize_entry("status", "awaiting_confirmation")?;
                map.serialize_entry("plan", plan)?;
            }
            ChatgptResearchState::Running {
                plan_title,
                progress,
            } => {
                map.serialize_entry("status", "running")?;
                map.serialize_entry("plan_title", plan_title)?;
                map.serialize_entry("progress", progress)?;
            }
            ChatgptResearchState::Completed(report) => {
                map.serialize_entry("status", "completed")?;
                map.serialize_entry("title", &report.title)?;
                if let Some(files) = &self.report_files {
                    map.serialize_entry("report_path", &files.report_path)?;
                    map.serialize_entry("sources_path", &files.sources_path)?;
                }
                map.serialize_entry("content_len", &report.content_len())?;
                map.serialize_entry("source_count", &report.sources.len())?;
            }
        }
        map.end()
    }
}

#[derive(Debug)]
/// A failed ChatGPT Deep Research command, with the conversation it concerns when forager
/// knows a checked conversation id, so the user can open it.
pub struct ChatgptResearchFailure {
    pub error: ProviderError,
    pub conversation: Option<ChatgptConversationId>,
}
