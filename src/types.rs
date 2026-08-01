use std::collections::HashSet;
use std::str::FromStr;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub(crate) const MIN_FETCH_CONTENT_CHARS: usize = 200;
pub(crate) const DENSITY_MAX_UNIQUE_LINES: usize = 3;
pub(crate) const DENSITY_MAX_CHARS: usize = 500;
pub(crate) const MIN_USEFUL_SLICE_SECONDS: u64 = 5;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// A research capability exposed by an execution seam.
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
/// A deduplicated capability set kept in the canonical [`Capability`] order.
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

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// A capability that a Schema v1 research plan may authorize.
pub enum PlanCapability {
    /// Documentation Search Capability.
    DocsSearch,
    /// Supplemental Web Search Capability.
    WebSearch,
    /// Vertical Search Capability.
    VerticalSearch,
}

impl PlanCapability {
    pub(crate) fn as_capability(self) -> Capability {
        match self {
            Self::DocsSearch => Capability::DocsSearch,
            Self::WebSearch => Capability::WebSearch,
            Self::VerticalSearch => Capability::VerticalSearch,
        }
    }
}

impl<'de> Deserialize<'de> for PlanCapability {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "docs_search" => Ok(Self::DocsSearch),
            "web_search" => Ok(Self::WebSearch),
            "vertical_search" => Ok(Self::VerticalSearch),
            "web_fetch" => Err(serde::de::Error::custom(
                "`web_fetch` is a research engine invariant and is executed automatically",
            )),
            _ => Err(serde::de::Error::custom(format!(
                "unknown plan capability `{value}`; expected docs_search, web_search, or vertical_search"
            ))),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// How current the requested research evidence must be.
pub enum RecencyRequirement {
    /// No recency requirement.
    None,
    /// Recent evidence is preferred.
    Recent,
    /// Current evidence is preferred.
    Current,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// The requested evidence or cross-validation strength.
pub enum EvidenceStrength {
    /// Normal evidence strength.
    Normal,
    /// High evidence strength.
    High,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// The consequence of making a claim from insufficient evidence.
pub enum ClaimRisk {
    /// Ordinary claim risk.
    Medium,
    /// High claim risk.
    High,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// Typed Schema v1 signals that adjust evidence strength without selecting seams.
pub struct ResearchIntentSignals {
    /// Required evidence recency.
    pub recency_requirement: RecencyRequirement,
    /// Whether the question concerns documentation or APIs.
    pub docs_api_intent: bool,
    /// Required source authority.
    pub source_authority_need: EvidenceStrength,
    /// Risk of making an unsupported claim.
    pub claim_risk: ClaimRisk,
    /// Required cross-validation strength.
    pub cross_validation_need: EvidenceStrength,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// One independently answerable part of a Schema v1 research plan.
pub struct ResearchSubquestion {
    /// Unique, non-empty identifier within the plan.
    pub id: String,
    /// Question used at the declared capability seams.
    pub question: String,
    /// Non-empty reason the subquestion is required.
    pub reason: String,
    /// Complete, authoritative capability set for this subquestion.
    pub required_capabilities: Vec<PlanCapability>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// A strictly parsed caller or classifier research plan using Schema v1.
pub struct ResearchPlan {
    /// Schema version, which must equal one.
    pub plan_version: u8,
    /// Evidence-policy signals that cannot alter capability selection.
    pub intent_signals: ResearchIntentSignals,
    /// Non-empty list of subquestions with unique identifiers.
    pub decomposition: Vec<ResearchSubquestion>,
}

impl ResearchPlan {
    pub(crate) fn parse_json(input: &str) -> Result<Self, String> {
        let mut plan: Self = serde_json::from_str(input)
            .map_err(|error| format!("invalid research plan: {error}"))?;
        if plan.plan_version != 1 {
            return Err(format!(
                "unsupported research plan version {}; expected version 1",
                plan.plan_version
            ));
        }
        if plan.decomposition.is_empty() {
            return Err("research plan decomposition must not be empty".into());
        }
        let mut ids = HashSet::new();
        for subquestion in &mut plan.decomposition {
            subquestion.id = subquestion.id.trim().to_owned();
            if subquestion.id.is_empty() {
                return Err("research plan decomposition id must not be empty".into());
            }
            if !ids.insert(subquestion.id.clone()) {
                return Err(format!(
                    "research plan decomposition id `{}` is duplicated",
                    subquestion.id
                ));
            }
            if subquestion.reason.trim().is_empty() {
                return Err(format!(
                    "research plan decomposition `{}` reason must not be empty",
                    subquestion.id
                ));
            }
            let mut capabilities = HashSet::new();
            subquestion
                .required_capabilities
                .retain(|capability| capabilities.insert(*capability));
        }
        Ok(plan)
    }

    pub(crate) fn capabilities(&self) -> CapabilitySet {
        CapabilitySet::from_capabilities(
            self.decomposition
                .iter()
                .flat_map(|subquestion| subquestion.required_capabilities.iter())
                .copied()
                .map(PlanCapability::as_capability)
                .chain(std::iter::once(Capability::WebFetch)),
        )
    }
}

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

#[derive(Clone, Debug, Serialize)]
/// Diagnostic metadata for one logical provider attempt.
///
/// A successful attempt has no `error_kind`; failure messages must already be redacted before
/// construction.
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
/// A normalized source returned by a search provider.
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
/// The normalized result of a search, including enrichment and capability coverage.
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
/// A citation derived only from fetched or read evidence.
pub struct Citation {
    /// Redacted evidence URL.
    pub url: String,
    /// Human-readable evidence title.
    pub title: String,
    /// Provider that returned the evidence content.
    pub provider: &'static str,
}

#[derive(Clone, Debug, Serialize)]
/// Fetched or read content that may support research claims.
pub struct EvidenceItem {
    /// Invocation-local stable identifier.
    pub id: String,
    /// Redacted evidence URL.
    pub url: String,
    /// Human-readable evidence title.
    pub title: String,
    /// Provider that returned the content.
    pub provider: &'static str,
    /// Whether the content came from Web Fetch or documentation retrieval.
    pub source_type: &'static str,
    /// Plan subquestion associated with the evidence.
    pub subquestion_id: String,
    /// Full fetched or read content.
    pub content: String,
    /// Character count of `content`.
    pub content_len: usize,
    /// Whether non-empty content passed the evidence boundary.
    pub verified: bool,
}

#[derive(Clone, Debug, Serialize)]
/// An evidence gap that remains after research execution.
pub struct ResearchGap {
    /// Related subquestion, or empty for a plan-wide gap.
    pub subquestion_id: String,
    /// Stable explanation of the missing evidence.
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Redacted URL associated with the gap.
    pub url: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
/// Terminal evidence coverage and stop reason.
pub struct ResearchGapCheck {
    /// `closed` or `degraded`.
    pub status: &'static str,
    /// Remaining evidence gaps.
    pub gaps: Vec<ResearchGap>,
    /// Stable reason execution stopped.
    pub stop_reason: &'static str,
}

#[derive(Clone, Debug, Serialize)]
/// Successful evidence-backed research output.
pub struct ResearchOutcome {
    /// Original caller query.
    pub query: String,
    /// Applied quick, standard, or deep budget.
    pub budget: &'static str,
    /// Origin of the authoritative plan.
    pub plan_source: &'static str,
    /// Normalized Schema v1 plan.
    pub research_plan: ResearchPlan,
    /// Final seam set, including the Web Fetch engine invariant.
    pub capabilities: Vec<Capability>,
    /// Evidence-only synthesized answer.
    pub final_answer: String,
    /// Alias of `final_answer` for content output.
    pub content: String,
    /// Citations derived from `evidence_items`.
    pub citations: Vec<Citation>,
    /// Full fetched or read evidence.
    pub evidence_items: Vec<EvidenceItem>,
    /// Advisory provider availability gaps.
    pub capability_gaps: Vec<CapabilityGap>,
    /// Evidence coverage state.
    pub gap_check: ResearchGapCheck,
    /// Whether a same-seam provider fallback occurred.
    pub fallback_used: bool,
    /// Whether non-fatal gaps remain.
    pub degraded: bool,
    /// Directory containing evidence artifacts.
    pub evidence_dir: String,
    #[serde(skip)]
    /// Full provider attempt chain for verbose output and journaling.
    pub attempts: Vec<ProviderAttempt>,
    #[serde(skip)]
    /// Non-fatal diagnostic text for standard error.
    pub diagnostic: Option<String>,
}

#[derive(Clone, Debug, Error)]
#[error("{message}")]
/// A research terminal that could not produce sufficient evidence.
pub struct ResearchError {
    /// Terminal error attribution.
    pub kind: AttemptErrorKind,
    /// Redacted human-readable failure reason.
    pub message: String,
    /// Full provider attempt chain.
    pub attempts: Vec<ProviderAttempt>,
    /// Evidence obtained before the terminal threshold was evaluated.
    pub evidence_items: Vec<EvidenceItem>,
    /// Advisory provider availability gaps.
    pub capability_gaps: Vec<CapabilityGap>,
    /// Non-fatal diagnostic text for standard error.
    pub diagnostic: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
/// The outcome of validating whether a source URL is usable as evidence.
pub struct ValidationResult {
    pub url: String,
    pub provider: &'static str,
    pub status: &'static str,
}

#[derive(Clone, Debug, Serialize)]
/// An advisory record that a requested capability could not be executed.
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
/// The non-fatal result of attempting to persist a journal entry.
pub struct JournalOutcome {
    pub status: &'static str,
    pub reference: Option<String>,
    pub warning: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
/// A documentation library candidate returned by Context7 resolution.
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
/// The normalized result of a Context7 library-resolution request.
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
/// The normalized result of a Context7 documentation request.
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
/// A Context7 result whose variant matches the requested library or documentation operation.
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
/// The normalized result of an `AnySearch` Domain Discovery operation.
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
/// A normalized URL-backed or structured `AnySearch` result.
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
/// A terminal `AnySearch` Acceptance Surface result.
pub enum AnysearchOutcome {
    Domains(AnysearchDomainsOutcome),
    Search(AnysearchSearchOutcome),
}

#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
/// The operation-specific input echoed in an Exa result.
pub enum ExaInput {
    Search { query: String },
    Similar { url: String },
}

impl ExaInput {
    #[must_use]
    pub fn value(&self) -> &str {
        match self {
            Self::Search { query } => query,
            Self::Similar { url } => url,
        }
    }

    #[must_use]
    pub fn operation(&self) -> &'static str {
        match self {
            Self::Search { .. } => "search",
            Self::Similar { .. } => "similar",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
/// The normalized result of an Exa search or similar-page request.
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
/// The normalized content returned by a Web Fetch provider chain.
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
/// The normalized URL collection returned by a site-map request.
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

#[cfg(test)]
mod research_plan_tests {
    use serde_json::{Value, json};

    use super::{AttemptErrorKind, ErrorKind, ResearchPlan};

    #[test]
    fn provider_attempt_error_domain_excludes_preflight_config() {
        let kinds = [
            AttemptErrorKind::Auth,
            AttemptErrorKind::RateLimited,
            AttemptErrorKind::QuotaExhausted,
            AttemptErrorKind::Parameter,
            AttemptErrorKind::Timeout,
            AttemptErrorKind::Network,
            AttemptErrorKind::Quality,
            AttemptErrorKind::Evidence,
            AttemptErrorKind::Runtime,
        ];

        assert_eq!(kinds.len(), 9);
        assert!(
            kinds
                .into_iter()
                .map(ErrorKind::from)
                .all(|kind| kind != ErrorKind::Config)
        );
    }

    #[test]
    fn strict_parser_accepts_schema_v1_and_normalizes_duplicate_capabilities() {
        let plan = ResearchPlan::parse_json(
            &valid_plan(&json!([
                "vertical_search",
                "docs_search",
                "vertical_search"
            ]))
            .to_string(),
        )
        .expect("valid plan");

        assert_eq!(
            serde_json::to_value(&plan.decomposition[0].required_capabilities)
                .expect("serialize capabilities"),
            json!(["vertical_search", "docs_search"])
        );
    }

    #[test]
    fn strict_parser_rejects_each_schema_boundary() {
        let valid = valid_plan(&json!(["web_search"]));
        let mut invalid = Vec::new();

        let mut version = valid.clone();
        version["plan_version"] = json!(2);
        invalid.push(version);
        let mut capability = valid.clone();
        capability["decomposition"][0]["required_capabilities"] = json!(["unknown"]);
        invalid.push(capability);
        let mut web_fetch = valid.clone();
        web_fetch["decomposition"][0]["required_capabilities"] = json!(["web_fetch"]);
        invalid.push(web_fetch);
        let mut duplicate_id = valid.clone();
        duplicate_id["decomposition"]
            .as_array_mut()
            .expect("decomposition")
            .push(valid["decomposition"][0].clone());
        invalid.push(duplicate_id);
        let mut empty_id = valid.clone();
        empty_id["decomposition"][0]["id"] = json!(" ");
        invalid.push(empty_id);
        let mut empty_reason = valid.clone();
        empty_reason["decomposition"][0]["reason"] = json!("");
        invalid.push(empty_reason);
        let mut unknown_field = valid.clone();
        unknown_field["steps"] = json!([]);
        invalid.push(unknown_field);
        let mut missing_field = valid.clone();
        missing_field
            .as_object_mut()
            .expect("plan")
            .remove("intent_signals");
        invalid.push(missing_field);
        let mut empty_decomposition = valid;
        empty_decomposition["decomposition"] = json!([]);
        invalid.push(empty_decomposition);

        assert!(
            invalid
                .iter()
                .all(|plan| ResearchPlan::parse_json(&plan.to_string()).is_err())
        );
    }

    #[test]
    fn web_fetch_error_explains_the_engine_invariant() {
        let plan = valid_plan(&json!(["web_fetch"]));

        let error = ResearchPlan::parse_json(&plan.to_string()).expect_err("invalid plan");

        assert!(error.contains("research engine invariant"));
    }

    fn valid_plan(capabilities: &Value) -> Value {
        json!({
            "plan_version": 1,
            "intent_signals": {
                "recency_requirement": "none",
                "docs_api_intent": false,
                "source_authority_need": "normal",
                "claim_risk": "medium",
                "cross_validation_need": "normal"
            },
            "decomposition": [{
                "id": "sq1",
                "question": "What evidence is available?",
                "reason": "Gather relevant evidence",
                "required_capabilities": capabilities
            }]
        })
    }
}
