use serde::Serialize;

use super::AttemptErrorKind;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// Whether a provider attempt ran and how it completed.
pub enum AttemptDisposition {
    /// The provider operation completed successfully.
    Succeeded,
    /// The provider operation ran and failed.
    Failed,
    /// The provider operation was intentionally not run.
    Skipped,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(untagged)]
/// The execution boundary associated with a provider attempt.
pub enum AttemptTarget {
    /// A canonical research capability seam.
    Seam { seam: &'static str },
    /// A provider-specific operation outside the capability vocabulary.
    Operation { operation: &'static str },
}

impl AttemptTarget {
    /// Constructs a canonical capability-seam target.
    #[must_use]
    pub const fn seam(seam: &'static str) -> Self {
        Self::Seam { seam }
    }

    /// Constructs a provider-specific operation target.
    #[must_use]
    pub const fn operation(operation: &'static str) -> Self {
        Self::Operation { operation }
    }

    pub(crate) const fn seam_name(self) -> Option<&'static str> {
        match self {
            Self::Seam { seam } => Some(seam),
            Self::Operation { .. } => None,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
/// Diagnostic metadata for one logical provider attempt.
///
/// Only failed attempts have an `error_kind`; failure messages must already be redacted before
/// construction.
pub struct ProviderAttempt {
    pub provider: &'static str,
    #[serde(flatten)]
    pub target: AttemptTarget,
    pub disposition: AttemptDisposition,
    pub error_kind: Option<AttemptErrorKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    pub duration_ms: u64,
    /// Credential used by this attempt; always 0 for a provider that requires no credentials.
    pub credential_index: usize,
    pub retry_count: usize,
    /// Credential rotations before this attempt; always 0 for a provider that requires no
    /// credentials.
    pub rotation_count: usize,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint_host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub breaker_event: Option<&'static str>,
}
