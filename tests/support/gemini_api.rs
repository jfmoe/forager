//! Gemini API fixtures: configuration that pairs `gemini_api` with `youtube_api`, and
//! `generateContent` and error responses derived from the stored samples.

use serde_json::{Value, json};

use super::Response;
use super::gemini_api_samples as samples;
use super::youtube;

/// The fixture Gemini key; tests assert it travels only in the request header.
pub(crate) const KEY: &str = "gemini-fixture-key-0123456789";
pub(crate) const MODEL: &str = "gemini-flash-latest";
/// The model version of the stored `generateContent` answer.
pub(crate) const MODEL_VERSION: &str = "gemini-3.8-flash-n";

/// Points `youtube_api` at `api_url` and `gemini_api` at `gemini_url`, one attempt per request;
/// the default order runs the precheck on `youtube_api` and the model on `gemini_api`.
pub(crate) fn config(api_url: &str, gemini_url: &str) -> String {
    format!(
        "{}\n[providers.gemini_api]\nurl = \"{gemini_url}/v1beta\"\nkeys = [\"{KEY}\"]\nmodel = \"{MODEL}\"\n",
        youtube::config(api_url)
    )
}

/// A finished `generateContent` response whose answer is `text`, after a reasoning summary.
pub(crate) fn answer(text: &str) -> Response {
    Response::json(200, &samples::generated(text, "STOP").to_string())
}

/// The stored transcription answer: a finished `generateContent` response whose answer text is
/// the model's transcript of the stored video.
pub(crate) fn transcription() -> Response {
    Response::json(
        200,
        &samples::stored("generate-content-transcript.json").to_string(),
    )
}

/// The usage the stored `generateContent` answer reports, as forager prints it.
pub(crate) fn usage() -> Value {
    json!({
        "prompt_tokens": 1747,
        "output_tokens": 494,
        "thoughts_tokens": 495,
        "cached_tokens": null,
        "total_tokens": 2736
    })
}

/// A Gemini error body.
pub(crate) fn error(status: u16, reason: &str, message: &str) -> Response {
    Response::json(status, &samples::error(status, reason, message).to_string())
}
