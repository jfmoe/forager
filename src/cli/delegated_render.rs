//! Binary-side rendering that every delegated research command shares: the rendered result
//! before `--output` handling, and the failure that names the conversation it concerns.

use std::fmt::Write as _;

use forager::app::{OutputTarget, ProviderError};
use serde_json::Value;

use crate::{
    RenderedOutput, apply_tee, encode_failure_payload, failure_payload, postflight_exit_code,
};

/// A rendered result before `--output` handling.
pub(crate) struct Rendered {
    pub(crate) stdout: String,
    pub(crate) is_json: bool,
    pub(crate) exit_code: u8,
    pub(crate) diagnostic: Option<String>,
}

impl Rendered {
    pub(crate) fn success(stdout: String, is_json: bool, diagnostic: Option<String>) -> Self {
        Self {
            stdout,
            is_json,
            exit_code: 0,
            diagnostic,
        }
    }

    /// A failed command of `agent`, such as `Gemini`: the stable failure payload plus a
    /// nullable `conversation_url`, or a Markdown failure that links the conversation.
    pub(crate) fn failure(
        agent: &str,
        error: ProviderError,
        conversation_url: Option<String>,
        json: bool,
    ) -> Result<Self, String> {
        let stdout = if json {
            let mut payload = failure_payload(&error)?;
            payload.insert(
                "conversation_url".into(),
                conversation_url.map_or(Value::Null, Value::String),
            );
            encode_failure_payload(&error, &payload)?
        } else {
            let mut markdown = format!(
                "# {agent} Deep Research failed\n\n**{}**: {}",
                error.kind.as_str(),
                error.message
            );
            if let Some(url) = conversation_url {
                let _ = write!(markdown, "\n\nConversation: <{url}>");
            }
            markdown
        };
        Ok(Self {
            stdout,
            is_json: json,
            exit_code: postflight_exit_code(error.kind),
            diagnostic: error.diagnostic,
        })
    }

    pub(crate) fn emit(self, output: Option<OutputTarget>) -> Result<RenderedOutput, String> {
        apply_tee(
            self.stdout,
            self.exit_code,
            self.is_json,
            output,
            self.diagnostic,
        )
    }
}
