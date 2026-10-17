//! Gemini API fixtures: configuration that pairs `gemini_api` with `youtube_api`, and
//! `generateContent` responses in the shape the gateway returned on 2026-10-17.

use serde_json::{Value, json};

use super::Response;
use super::youtube;

/// The fixture Gemini key; tests assert it travels only in the request header.
pub(crate) const KEY: &str = "gemini-fixture-key-0123456789";
pub(crate) const MODEL: &str = "gemini-flash-latest";
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
    generate(text, "STOP")
}

/// A `generateContent` response with one candidate that stopped for `finish_reason`.
pub(crate) fn generate(text: &str, finish_reason: &str) -> Response {
    Response::json(
        200,
        &json!({
            "candidates": [{
                "content": {"role": "model", "parts": [
                    {"text": "Looking at the video first.", "thought": true},
                    {"text": text}
                ]},
                "finishReason": finish_reason,
                "index": 0
            }],
            "usageMetadata": {
                "promptTokenCount": 1747,
                "candidatesTokenCount": 494,
                "totalTokenCount": 2736,
                "thoughtsTokenCount": 495
            },
            "modelVersion": MODEL_VERSION,
            "responseId": "fixture-response"
        })
        .to_string(),
    )
}

/// The usage `generate` reports, as forager prints it.
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
    Response::json(
        status,
        &json!({"error": {"code": status, "message": message, "status": reason}}).to_string(),
    )
}
