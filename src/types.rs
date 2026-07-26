use std::time::{Duration, Instant};

use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
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

    pub fn family(self) -> Option<ErrorFamily> {
        match self {
            Self::Config => None,
            Self::Quality | Self::Evidence => Some(ErrorFamily::Content),
            _ => Some(ErrorFamily::Transport),
        }
    }

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

#[derive(Clone, Debug, Serialize)]
pub struct ProviderAttempt {
    pub provider: &'static str,
    pub seam: &'static str,
    pub error_kind: Option<AttemptErrorKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    pub duration_ms: u64,
    pub credential_index: usize,
    pub retry_count: usize,
    pub rotation_count: usize,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Source {
    pub title: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub highlights: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub enum ExaInput {
    Search { query: String },
    Similar { url: String },
}

impl ExaInput {
    pub fn value(&self) -> &str {
        match self {
            Self::Search { query } => query,
            Self::Similar { url } => url,
        }
    }

    pub fn operation(&self) -> &'static str {
        match self {
            Self::Search { .. } => "search",
            Self::Similar { .. } => "similar",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ExaOutcome {
    pub provider: &'static str,
    #[serde(flatten)]
    pub input: ExaInput,
    pub results: Vec<Source>,
    #[serde(rename = "provider_attempts", skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<ProviderAttempt>,
    #[serde(skip)]
    pub diagnostic: Option<String>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Deadline {
    started: Instant,
    duration: Duration,
}

impl Deadline {
    pub(crate) fn new(duration: Duration) -> Self {
        Self {
            started: Instant::now(),
            duration,
        }
    }

    pub(crate) fn remaining(self) -> Option<Duration> {
        self.duration.checked_sub(self.started.elapsed())
    }
}
