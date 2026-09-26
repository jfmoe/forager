use serde::Serialize;
use thiserror::Error;

use super::ProviderAttempt;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// A terminal command error, including failures detected before provider execution.
pub enum ErrorKind {
    Auth,
    RateLimited,
    QuotaExhausted,
    Parameter,
    Config,
    Timeout,
    Network,
    Quality,
    Evidence,
    Runtime,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// The transport or content family used to map terminal errors to exit behavior.
///
/// Preflight configuration errors have no family; see [`ErrorKind::family`].
pub enum ErrorFamily {
    Transport,
    Content,
}

impl ErrorKind {
    pub(crate) fn is_retryable(self) -> bool {
        matches!(self, Self::Timeout | Self::Network)
    }

    pub(crate) fn rotates_credential(self) -> bool {
        matches!(self, Self::RateLimited | Self::QuotaExhausted)
    }

    #[must_use]
    pub fn family(self) -> Option<ErrorFamily> {
        match self {
            Self::Config => None,
            Self::Quality | Self::Evidence => Some(ErrorFamily::Content),
            _ => Some(ErrorFamily::Transport),
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auth => "auth",
            Self::RateLimited => "rate_limited",
            Self::QuotaExhausted => "quota_exhausted",
            Self::Parameter => "parameter",
            Self::Config => "config",
            Self::Timeout => "timeout",
            Self::Network => "network",
            Self::Quality => "quality",
            Self::Evidence => "evidence",
            Self::Runtime => "runtime",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// A provider-attempt failure category.
///
/// This domain excludes preflight configuration failures, which cannot result from an attempt.
pub enum AttemptErrorKind {
    Auth,
    RateLimited,
    QuotaExhausted,
    Parameter,
    Timeout,
    Network,
    Quality,
    Evidence,
    Runtime,
}

impl AttemptErrorKind {
    pub(crate) fn is_retryable(self) -> bool {
        ErrorKind::from(self).is_retryable()
    }

    pub(crate) fn rotates_credential(self) -> bool {
        ErrorKind::from(self).rotates_credential()
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        ErrorKind::from(self).as_str()
    }
}

impl From<AttemptErrorKind> for ErrorKind {
    fn from(value: AttemptErrorKind) -> Self {
        match value {
            AttemptErrorKind::Auth => Self::Auth,
            AttemptErrorKind::RateLimited => Self::RateLimited,
            AttemptErrorKind::QuotaExhausted => Self::QuotaExhausted,
            AttemptErrorKind::Parameter => Self::Parameter,
            AttemptErrorKind::Timeout => Self::Timeout,
            AttemptErrorKind::Network => Self::Network,
            AttemptErrorKind::Quality => Self::Quality,
            AttemptErrorKind::Evidence => Self::Evidence,
            AttemptErrorKind::Runtime => Self::Runtime,
        }
    }
}

#[derive(Debug, Error)]
#[error("{message}")]
/// A terminal provider failure with the attempts that led to it.
pub struct ProviderError {
    pub kind: AttemptErrorKind,
    pub message: String,
    pub attempts: Vec<ProviderAttempt>,
    pub verbose: bool,
    pub diagnostic: Option<String>,
    pub redirected_library_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::{AttemptErrorKind, ErrorFamily, ErrorKind};

    type AttemptErrorCase = (
        AttemptErrorKind,
        ErrorKind,
        &'static str,
        ErrorFamily,
        bool,
        bool,
    );

    const ATTEMPT_ERROR_CASES: [AttemptErrorCase; 9] = [
        (
            AttemptErrorKind::Auth,
            ErrorKind::Auth,
            "auth",
            ErrorFamily::Transport,
            false,
            false,
        ),
        (
            AttemptErrorKind::RateLimited,
            ErrorKind::RateLimited,
            "rate_limited",
            ErrorFamily::Transport,
            false,
            true,
        ),
        (
            AttemptErrorKind::QuotaExhausted,
            ErrorKind::QuotaExhausted,
            "quota_exhausted",
            ErrorFamily::Transport,
            false,
            true,
        ),
        (
            AttemptErrorKind::Parameter,
            ErrorKind::Parameter,
            "parameter",
            ErrorFamily::Transport,
            false,
            false,
        ),
        (
            AttemptErrorKind::Timeout,
            ErrorKind::Timeout,
            "timeout",
            ErrorFamily::Transport,
            true,
            false,
        ),
        (
            AttemptErrorKind::Network,
            ErrorKind::Network,
            "network",
            ErrorFamily::Transport,
            true,
            false,
        ),
        (
            AttemptErrorKind::Quality,
            ErrorKind::Quality,
            "quality",
            ErrorFamily::Content,
            false,
            false,
        ),
        (
            AttemptErrorKind::Evidence,
            ErrorKind::Evidence,
            "evidence",
            ErrorFamily::Content,
            false,
            false,
        ),
        (
            AttemptErrorKind::Runtime,
            ErrorKind::Runtime,
            "runtime",
            ErrorFamily::Transport,
            false,
            false,
        ),
    ];

    #[test]
    fn provider_attempt_errors_preserve_their_public_and_control_flow_semantics() {
        for (attempt, error, name, family, retryable, rotates) in ATTEMPT_ERROR_CASES {
            assert_eq!(
                (
                    ErrorKind::from(attempt),
                    attempt.as_str(),
                    serde_json::to_value(attempt).expect("serialize attempt error kind"),
                    error.family(),
                    attempt.is_retryable(),
                    attempt.rotates_credential(),
                ),
                (
                    error,
                    name,
                    Value::String(name.to_owned()),
                    Some(family),
                    retryable,
                    rotates,
                ),
                "attempt={attempt:?}"
            );
        }
        assert_eq!(
            (
                ErrorKind::Config.as_str(),
                serde_json::to_value(ErrorKind::Config).expect("serialize config error kind"),
                ErrorKind::Config.family(),
                ErrorKind::Config.is_retryable(),
                ErrorKind::Config.rotates_credential(),
            ),
            ("config", Value::String("config".into()), None, false, false)
        );
    }
}
