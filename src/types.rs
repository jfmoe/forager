use std::collections::HashSet;
use std::str::FromStr;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

pub(crate) const MIN_FETCH_CONTENT_CHARS: usize = 200;
pub(crate) const DENSITY_MAX_UNIQUE_LINES: usize = 3;
pub(crate) const DENSITY_MAX_CHARS: usize = 500;
pub(crate) const MIN_USEFUL_SLICE_SECONDS: u64 = 5;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    DocsSearch,
    WebSearch,
    WebFetch,
    VerticalSearch,
}

impl Capability {
    const VOCABULARY: [Self; 4] = [
        Self::DocsSearch,
        Self::WebSearch,
        Self::WebFetch,
        Self::VerticalSearch,
    ];

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::DocsSearch => "docs_search",
            Self::WebSearch => "web_search",
            Self::WebFetch => "web_fetch",
            Self::VerticalSearch => "vertical_search",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilitySet(Vec<Capability>);

impl CapabilitySet {
    pub(crate) fn from_capabilities(capabilities: impl IntoIterator<Item = Capability>) -> Self {
        let selected = capabilities.into_iter().collect::<HashSet<_>>();
        Self(
            Capability::VOCABULARY
                .into_iter()
                .filter(|capability| selected.contains(capability))
                .collect(),
        )
    }

    pub(crate) fn default_supplemental_web_search() -> Self {
        Self(vec![Capability::WebSearch])
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = Capability> + '_ {
        self.0.iter().copied()
    }
}

impl FromStr for CapabilitySet {
    type Err = String;

    fn from_str(declaration: &str) -> Result<Self, Self::Err> {
        if declaration.trim().is_empty() {
            return Err(
                "capability declaration must not be empty; use `none` for an empty set".into(),
            );
        }
        let values = declaration
            .split(',')
            .map(|value| value.trim().to_ascii_lowercase())
            .collect::<Vec<_>>();
        if values.iter().any(String::is_empty) {
            return Err(
                "capability declaration contains an empty CSV value; use `none` for an empty set"
                    .into(),
            );
        }
        if values.iter().any(|value| value == "none") {
            return if values.len() == 1 {
                Ok(Self(Vec::new()))
            } else {
                Err("`none` must be used alone".into())
            };
        }
        if let Some(unknown) = values.iter().find(|value| {
            !Capability::VOCABULARY
                .iter()
                .any(|capability| capability.as_str() == value.as_str())
        }) {
            return Err(format!(
                "unknown capability `{unknown}`; expected docs_search, web_search, web_fetch, vertical_search, or none"
            ));
        }
        let selected = values.into_iter().collect::<HashSet<_>>();
        Ok(Self::from_capabilities(
            Capability::VOCABULARY
                .into_iter()
                .filter(|capability| selected.contains(capability.as_str())),
        ))
    }
}

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint_host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub breaker_event: Option<&'static str>,
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
pub struct SearchOutcome {
    pub provider: &'static str,
    pub query: String,
    pub model: String,
    pub answer: String,
    pub sources: Vec<Source>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_sources: Vec<Source>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub validation_results: Vec<ValidationResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vertical_results: Vec<AnysearchResult>,
    pub capabilities: Vec<Capability>,
    pub capability_gaps: Vec<CapabilityGap>,
    #[serde(skip)]
    pub attempts: Vec<ProviderAttempt>,
    #[serde(skip)]
    pub diagnostic: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ValidationResult {
    pub url: String,
    pub provider: &'static str,
    pub status: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct CapabilityGap {
    pub capability: Capability,
    pub reason: &'static str,
    pub providers_skipped: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct SupplementalSearchOutcome {
    pub(crate) sources: Vec<Source>,
    pub(crate) attempts: Vec<ProviderAttempt>,
    pub(crate) diagnostic: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct VerticalSearchOutcome {
    pub(crate) results: Vec<AnysearchResult>,
    pub(crate) sources: Vec<Source>,
    pub(crate) attempts: Vec<ProviderAttempt>,
    pub(crate) diagnostic: Option<String>,
}

#[derive(Clone, Debug)]
pub struct JournalOutcome {
    pub status: &'static str,
    pub reference: Option<String>,
    pub warning: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LibraryCandidate {
    pub id: String,
    pub title: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trust_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub benchmark_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_snippets: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stars: Option<u64>,
    pub provider: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct Context7LibraryOutcome {
    pub provider: &'static str,
    pub query: String,
    pub results: Vec<LibraryCandidate>,
    pub total: usize,
    #[serde(rename = "provider_attempts", skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<ProviderAttempt>,
    #[serde(skip)]
    pub diagnostic: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Context7DocsOutcome {
    pub provider: &'static str,
    pub library_id: String,
    pub query: String,
    pub content: String,
    pub code_snippets: Vec<serde_json::Value>,
    pub info_snippets: Vec<serde_json::Value>,
    pub results: Vec<serde_json::Value>,
    pub total: usize,
    #[serde(rename = "provider_attempts", skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<ProviderAttempt>,
    #[serde(skip)]
    pub diagnostic: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Context7Outcome {
    Library(Context7LibraryOutcome),
    Docs(Context7DocsOutcome),
}

#[derive(Clone, Debug, Serialize)]
/// A child domain and its parameter contract returned by Domain Discovery.
pub struct AnysearchDomain {
    pub domain: String,
    pub sub_domain: String,
    pub description: String,
    pub parameter_schema: serde_json::Value,
}

#[derive(Clone, Debug, Serialize)]
/// The normalized result of an AnySearch Domain Discovery operation.
pub struct AnysearchDomainsOutcome {
    pub provider: &'static str,
    pub operation: &'static str,
    pub experimental: bool,
    pub domain: String,
    pub results: Vec<AnysearchDomain>,
    pub total: usize,
    #[serde(rename = "provider_attempts", skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<ProviderAttempt>,
    #[serde(skip)]
    pub diagnostic: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
/// A normalized URL-backed or structured AnySearch result.
pub struct AnysearchResult {
    pub title: String,
    pub url: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence_type: Option<&'static str>,
}

#[derive(Clone, Debug, Serialize)]
/// The relationship between request parameters and a Verified Domain Contract.
pub struct SchemaValidation {
    pub status: &'static str,
    pub errors: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_fingerprint: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
/// The normalized result of Vertical Discovery or an explicit Vertical Search Request.
pub struct AnysearchSearchOutcome {
    pub provider: &'static str,
    pub operation: &'static str,
    pub experimental: bool,
    pub query: String,
    pub max_results: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain_status: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sub_domain_param_keys: Vec<String>,
    pub schema_validation: SchemaValidation,
    pub results: Vec<AnysearchResult>,
    pub total: usize,
    #[serde(rename = "provider_attempts", skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<ProviderAttempt>,
    #[serde(skip)]
    pub diagnostic: Option<String>,
}

#[derive(Clone, Debug)]
/// A terminal AnySearch Acceptance Surface result.
pub enum AnysearchOutcome {
    Domains(AnysearchDomainsOutcome),
    Search(AnysearchSearchOutcome),
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

#[derive(Clone, Debug, Serialize)]
pub struct FetchOutcome {
    pub provider: &'static str,
    pub url: String,
    pub content: String,
    #[serde(rename = "provider_attempts", skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<ProviderAttempt>,
    #[serde(skip)]
    pub diagnostic: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MapOutcome {
    pub provider: &'static str,
    pub url: String,
    pub base_url: String,
    pub results: Vec<String>,
    pub response_time: f64,
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
