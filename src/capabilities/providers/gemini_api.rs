//! The `gemini_api` route: Gemini's native `generateContent` protocol, at the official API or a
//! gateway that speaks it, with the user's own keys.
//!
//! The video travels as a `fileData` part holding its canonical URL; only the native protocol
//! passes such a URL through, and no part of the video is clipped. The key travels only in the
//! `x-goog-api-key` header. Everything the model writes is relayed content. The route never
//! reads the platform itself: core checks the video through the platform's fetch chain first
//! and hands the item it read to the route.

use std::time::Duration;

use reqwest::{Client, RequestBuilder};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::catalog::ProviderId;
use crate::config::ModelRouteRuntimeConfig;
use crate::credentials::CredentialPool;
use crate::net::{
    AttemptFailure, RetryPolicy, combine_diagnostics, read_complete_protocol, send_provider_request,
};
use crate::providers::execution::{
    ExecutionOutcome, ExecutionSettings, execute_pinned, execute_v2,
};
use crate::providers::shared::acquire_window;
use crate::rate_limit::RateLimiter;
use crate::redact::Secret;
use crate::types::{
    ASK, AttemptErrorKind, AttemptTarget, Deadline, ModelAnswerOutcome, ModelTranscriptOutcome,
    ModelUsage, Platform, PlatformItem, ProviderError, TRANSCRIBE,
};

#[path = "gemini_api_transcript.rs"]
mod transcript;

const ROUTE: ProviderId = ProviderId::GeminiApi;
const PLATFORM: Platform = Platform::Youtube;
const KEY_HEADER: &str = "x-goog-api-key";
const VIDEO_MIME_TYPE: &str = "video/*";
const ASK_INSTRUCTION: &str = "Answer the question below from this video alone: from what it shows and what it says. Mark every moment you rely on as mm:ss, or h:mm:ss from the first hour on. If you cannot watch the video, say so instead of answering.";

pub(crate) struct GeminiApi {
    config: ModelRouteRuntimeConfig,
    client: Client,
    credentials: CredentialPool,
    limiter: RateLimiter,
    retry_policy: RetryPolicy,
    deadline: Deadline,
}

impl GeminiApi {
    pub(crate) fn new(
        config: ModelRouteRuntimeConfig,
        client: Client,
        credentials: CredentialPool,
        limiter: RateLimiter,
        retry_policy: RetryPolicy,
        deadline: Deadline,
    ) -> Self {
        Self {
            config,
            client,
            credentials,
            limiter,
            retry_policy,
            deadline,
        }
    }

    /// Answers a question about the video `video` names. An empty answer is a runtime failure.
    pub(crate) async fn ask(
        &self,
        video: &PlatformItem,
        question: &str,
    ) -> Result<ModelAnswerOutcome, ProviderError> {
        let body = video_request(
            &video.url,
            &format!("{ASK_INSTRUCTION}\n\nQuestion: {question}"),
            None,
        );
        let outcome = self
            .generate(ASK, &body, |response| {
                let text = response.text()?;
                if text.trim().is_empty() {
                    return Err(refusal(
                        AttemptErrorKind::Runtime,
                        "Gemini returned an empty answer".into(),
                    ));
                }
                Ok(text)
            })
            .await?;
        let (answer, model, usage) = outcome.value;
        Ok(ModelAnswerOutcome {
            answer,
            model,
            usage,
            attempts: outcome.attempts,
            diagnostic: outcome.diagnostic,
        })
    }

    /// Transcribes the speech of the video `video` names and renders it as a transcript
    /// document; the item supplies the header fields, the description, and the chapters.
    pub(crate) async fn transcribe(
        &self,
        video: &PlatformItem,
    ) -> Result<ModelTranscriptOutcome, ProviderError> {
        let body = video_request(
            &video.url,
            transcript::INSTRUCTION,
            Some(transcript::generation_config()),
        );
        let outcome = self
            .generate(TRANSCRIBE, &body, |response| {
                transcript::render(&response.text()?, video).map_err(|message| AttemptFailure {
                    kind: AttemptErrorKind::Quality,
                    status: Some(200),
                    message,
                })
            })
            .await?;
        let (rendered, model, usage) = outcome.value;
        Ok(ModelTranscriptOutcome {
            markdown: rendered.markdown,
            transcript: rendered.info,
            model,
            usage,
            attempts: outcome.attempts,
            diagnostic: combine_diagnostics(
                [outcome.diagnostic, rendered.diagnostic]
                    .into_iter()
                    .flatten(),
            ),
        })
    }

    /// Lists the models with every key in pool order; each check sends only its own key, so the
    /// persistent credential cursor does not move and no model quota is spent.
    pub(crate) async fn accounts(&self) -> Vec<Result<(), ProviderError>> {
        let url = format!("{}/models", self.base_url());
        let mut accounts = Vec::with_capacity(self.credentials.len());
        for index in 0..self.credentials.len() {
            let url = &url;
            let result = execute_pinned(
                &self.credentials,
                index,
                self.settings(AttemptTarget::operation("account")),
                |credential, deadline| async move {
                    let request = self.client.get(url);
                    let (status, text) = self.send(request, &credential, deadline).await?;
                    model_list(&text).map(|()| (Some(status), ()))
                },
            )
            .await;
            accounts.push(result.map(drop));
        }
        accounts
    }

    /// Sends one `generateContent` request with credential claim, rotation, and retry; `decode`
    /// turns the model's text into the operation's value inside the attempt, so its refusal is
    /// that attempt's failure. The value comes with the model version and the token usage.
    async fn generate<T>(
        &self,
        operation: &'static str,
        body: &Value,
        decode: impl Fn(&GenerateResponse) -> Result<T, AttemptFailure>,
    ) -> Result<ExecutionOutcome<(T, Option<String>, Option<ModelUsage>)>, ProviderError> {
        let url = &format!(
            "{}/models/{}:generateContent",
            self.base_url(),
            self.config.model
        );
        let decode = &decode;
        execute_v2(
            &self.credentials,
            self.settings(AttemptTarget::platform(PLATFORM.as_str(), operation)),
            |credential, deadline| async move {
                let request = self.client.post(url).json(body);
                let (status, text) = self.send(request, &credential, deadline).await?;
                let response =
                    serde_json::from_str::<GenerateResponse>(&text).map_err(|error| {
                        refusal(
                            AttemptErrorKind::Runtime,
                            format!("invalid Gemini response: {error}"),
                        )
                    })?;
                let value = decode(&response).map_err(|failure| AttemptFailure {
                    status: Some(status),
                    ..failure
                })?;
                Ok((
                    Some(status),
                    (
                        value,
                        response.model_version,
                        response.usage_metadata.map(ModelUsage::from),
                    ),
                ))
            },
        )
        .await
    }

    /// Sends one request with `credential` under the route's access policy and reads the whole
    /// body; an error status fails by the shared status mapping.
    async fn send(
        &self,
        request: RequestBuilder,
        credential: &Secret,
        deadline: Deadline,
    ) -> Result<(u16, String), AttemptFailure> {
        let _permit = acquire_window(&self.limiter, deadline).await?;
        let request = request.header(KEY_HEADER, credential.expose());
        let response = send_provider_request(request, &self.credentials).await?;
        let body = read_complete_protocol(response, &self.credentials, failure_message).await?;
        Ok((body.status, body.text))
    }

    fn base_url(&self) -> &str {
        self.config.url.trim_end_matches('/')
    }

    fn settings(&self, target: AttemptTarget) -> ExecutionSettings {
        ExecutionSettings {
            provider: ROUTE.name(),
            target,
            retry_policy: self.retry_policy,
            deadline: self.deadline,
            attempt_timeout: Duration::from_secs(self.config.timeout_seconds),
            verbose: false,
            timeout_message: "Gemini API request timed out",
            model: Some(self.config.model.clone()),
            transport: Some("http"),
            endpoint_host: None,
            breaker_event: None,
        }
    }
}

/// Builds a request whose user turn is the video, then the instruction; `generation_config`
/// shapes the output when present.
fn video_request(video_url: &str, instruction: &str, generation_config: Option<Value>) -> Value {
    let mut body = json!({
        "contents": [{
            "role": "user",
            "parts": [
                {"fileData": {"mimeType": VIDEO_MIME_TYPE, "fileUri": video_url}},
                {"text": instruction},
            ],
        }],
    });
    if let Some(config) = generation_config {
        body["generationConfig"] = config;
    }
    body
}

fn refusal(kind: AttemptErrorKind, message: String) -> AttemptFailure {
    AttemptFailure {
        kind,
        status: Some(200),
        message,
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GenerateResponse {
    #[serde(default)]
    candidates: Vec<Candidate>,
    prompt_feedback: Option<PromptFeedback>,
    usage_metadata: Option<UsageMetadata>,
    model_version: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Candidate {
    content: Option<Content>,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Content {
    #[serde(default)]
    parts: Vec<Part>,
}

#[derive(Debug, Deserialize)]
struct Part {
    text: Option<String>,
    /// Marks a summary of the model's reasoning, which is no part of the answer.
    #[serde(default)]
    thought: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PromptFeedback {
    block_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UsageMetadata {
    #[serde(rename = "promptTokenCount")]
    prompt: Option<u64>,
    #[serde(rename = "candidatesTokenCount")]
    candidates: Option<u64>,
    #[serde(rename = "thoughtsTokenCount")]
    thoughts: Option<u64>,
    #[serde(rename = "cachedContentTokenCount")]
    cached_content: Option<u64>,
    #[serde(rename = "totalTokenCount")]
    total: Option<u64>,
}

impl From<UsageMetadata> for ModelUsage {
    fn from(usage: UsageMetadata) -> Self {
        Self {
            prompt_tokens: usage.prompt,
            output_tokens: usage.candidates,
            thoughts_tokens: usage.thoughts,
            cached_tokens: usage.cached_content,
            total_tokens: usage.total,
        }
    }
}

impl GenerateResponse {
    /// Returns the text of the first candidate, without reasoning summaries. A blocked prompt, a
    /// withheld answer, and an answer cut off at the token limit fail as Quality: the model
    /// delivered no complete answer.
    fn text(&self) -> Result<String, AttemptFailure> {
        if let Some(reason) = self
            .prompt_feedback
            .as_ref()
            .and_then(|feedback| feedback.block_reason.as_deref())
        {
            return Err(refusal(
                AttemptErrorKind::Quality,
                format!("Gemini blocked the request ({reason})"),
            ));
        }
        let Some(candidate) = self.candidates.first() else {
            return Err(refusal(
                AttemptErrorKind::Runtime,
                "Gemini returned no candidate".into(),
            ));
        };
        match candidate.finish_reason.as_deref() {
            None | Some("STOP" | "FINISH_REASON_UNSPECIFIED") => {}
            Some("MAX_TOKENS") => {
                return Err(refusal(
                    AttemptErrorKind::Quality,
                    "Gemini stopped at its output token limit, so the answer is incomplete".into(),
                ));
            }
            Some(
                reason @ ("SAFETY" | "RECITATION" | "BLOCKLIST" | "PROHIBITED_CONTENT" | "SPII"
                | "IMAGE_SAFETY"),
            ) => {
                return Err(refusal(
                    AttemptErrorKind::Quality,
                    format!("Gemini withheld the answer ({reason})"),
                ));
            }
            Some(reason) => {
                return Err(refusal(
                    AttemptErrorKind::Runtime,
                    format!("Gemini stopped without finishing the answer ({reason})"),
                ));
            }
        }
        Ok(candidate
            .content
            .iter()
            .flat_map(|content| &content.parts)
            .filter(|part| !part.thought)
            .filter_map(|part| part.text.as_deref())
            .collect())
    }
}

/// Accepts a model list: the answer of an endpoint that knows the key.
fn model_list(body: &str) -> Result<(), AttemptFailure> {
    serde_json::from_str::<Value>(body)
        .ok()
        .filter(|list| list["models"].is_array())
        .map(drop)
        .ok_or_else(|| {
            refusal(
                AttemptErrorKind::Runtime,
                "the Gemini model list is not a JSON object with a `models` array".into(),
            )
        })
}

#[derive(Deserialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Deserialize)]
struct ErrorBody {
    #[serde(default)]
    message: String,
    status: Option<String>,
}

fn failure_message(body: &str, status: u16) -> String {
    match serde_json::from_str::<ErrorEnvelope>(body) {
        Ok(ErrorEnvelope { error }) if !error.message.is_empty() => {
            let reason = error
                .status
                .map(|reason| format!(" ({reason})"))
                .unwrap_or_default();
            format!(
                "Gemini API returned HTTP {status}{reason}: {}",
                error.message
            )
        }
        _ => format!("Gemini API returned HTTP {status}"),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{GenerateResponse, failure_message};
    use crate::types::AttemptErrorKind;

    fn response(value: &serde_json::Value) -> GenerateResponse {
        serde_json::from_value(value.clone()).expect("generate response")
    }

    fn answer(finish_reason: &str) -> Result<String, AttemptErrorKind> {
        response(&json!({
            "candidates": [{
                "content": {"role": "model", "parts": [
                    {"text": "planning the answer", "thought": true},
                    {"text": "Two elephants "},
                    {"text": "at 00:05."}
                ]},
                "finishReason": finish_reason
            }]
        }))
        .text()
        .map_err(|failure| failure.kind)
    }

    #[test]
    fn the_answer_joins_the_text_parts_and_leaves_out_reasoning() {
        assert_eq!(answer("STOP"), Ok("Two elephants at 00:05.".to_owned()));
    }

    #[test]
    fn a_truncated_withheld_or_blocked_answer_is_quality_and_an_unknown_stop_is_runtime() {
        let blocked = response(&json!({"promptFeedback": {"blockReason": "SAFETY"}}))
            .text()
            .map_err(|failure| failure.kind);
        let empty = response(&json!({"candidates": []}))
            .text()
            .map_err(|failure| failure.kind);

        assert_eq!(
            [
                answer("MAX_TOKENS"),
                answer("SAFETY"),
                answer("RECITATION"),
                blocked,
                answer("MALFORMED_FUNCTION_CALL"),
                empty,
            ],
            [
                Err(AttemptErrorKind::Quality),
                Err(AttemptErrorKind::Quality),
                Err(AttemptErrorKind::Quality),
                Err(AttemptErrorKind::Quality),
                Err(AttemptErrorKind::Runtime),
                Err(AttemptErrorKind::Runtime),
            ]
        );
    }

    #[test]
    fn failures_name_the_status_reason_and_message_gemini_gives() {
        let body = r#"{"error":{"code":403,"message":"The caller does not have permission","status":"PERMISSION_DENIED"}}"#;

        assert_eq!(
            [failure_message(body, 403), failure_message("<html>", 502)],
            [
                "Gemini API returned HTTP 403 (PERMISSION_DENIED): The caller does not have permission",
                "Gemini API returned HTTP 502",
            ]
        );
    }
}
