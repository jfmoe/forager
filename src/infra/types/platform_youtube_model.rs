//! YouTube's model operations: Ask and Transcribe, which a model answers after it watched and
//! listened to one video. Everything a model writes is relayed content, never the video's own
//! words.

use serde::Serialize;

use super::attempt::ProviderAttempt;
use super::platform::{Platform, PlatformFetchResult, PlatformRef, TranscriptInfo};
use super::platform_youtube::YoutubeRef;

/// The attempt-target name of the transcribe operation.
pub(crate) const TRANSCRIBE: &str = "transcribe";

/// A request to answer a question from one video's picture and sound.
#[derive(Clone, Debug)]
pub(crate) struct YoutubeAskRequest {
    /// The video; parsing guarantees a video ref.
    pub(crate) video: YoutubeRef,
    pub(crate) question: String,
}

impl YoutubeAskRequest {
    /// Checks the rules that argument parsing cannot express.
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.question.trim().is_empty() {
            return Err("youtube ask needs a question".into());
        }
        Ok(())
    }
}

/// A request to transcribe one video's speech with a model.
#[derive(Clone, Debug)]
pub(crate) struct YoutubeTranscribeRequest {
    /// The video; parsing guarantees a video ref.
    pub(crate) video: YoutubeRef,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
/// The tokens one model request consumed, as the model service reported them; a count the
/// service did not report is `null`.
pub struct ModelUsage {
    /// The prompt, the video included.
    pub prompt_tokens: Option<u64>,
    /// The answer the model wrote.
    pub output_tokens: Option<u64>,
    /// The model's reasoning before it answered.
    pub thoughts_tokens: Option<u64>,
    /// The part of the prompt the service served from its cache.
    pub cached_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
/// The video a model answered about, as the metadata check read it.
pub struct AskedVideo {
    #[serde(rename = "ref")]
    pub reference: PlatformRef,
    pub url: String,
    pub title: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
/// A model's answer about one video. The whole answer is relayed content: the model's account
/// of what it saw and heard, not the video's own words.
pub struct YoutubeAskResult {
    pub platform: Platform,
    /// The route that answered.
    pub provider: &'static str,
    pub video: AskedVideo,
    pub question: String,
    pub answer: String,
    /// The model version the service reported.
    pub model: Option<String>,
    pub usage: Option<ModelUsage>,
    #[serde(rename = "provider_attempts", skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<ProviderAttempt>,
    #[serde(skip)]
    pub diagnostic: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
/// A transcript a model generated, delivered like a platform full text: the item and its
/// document, with the model and the tokens it used.
pub struct PlatformTranscribeResult {
    #[serde(flatten)]
    pub fetched: PlatformFetchResult,
    /// The model version the service reported.
    pub model: Option<String>,
    pub usage: Option<ModelUsage>,
}

/// One route's answer before the chain attaches the route and the planning attempts.
pub(crate) struct ModelAnswerOutcome {
    pub(crate) answer: String,
    pub(crate) model: Option<String>,
    pub(crate) usage: Option<ModelUsage>,
    pub(crate) attempts: Vec<ProviderAttempt>,
    pub(crate) diagnostic: Option<String>,
}

/// One route's rendered transcript before the chain attaches the route and the planning
/// attempts.
pub(crate) struct ModelTranscriptOutcome {
    pub(crate) markdown: String,
    pub(crate) transcript: TranscriptInfo,
    pub(crate) model: Option<String>,
    pub(crate) usage: Option<ModelUsage>,
    pub(crate) attempts: Vec<ProviderAttempt>,
    pub(crate) diagnostic: Option<String>,
}
