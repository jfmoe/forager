use serde::Serialize;

use super::{ProviderAttempt, Source};

pub(crate) const MIN_FETCH_CONTENT_CHARS: usize = 200;
pub(crate) const DENSITY_MAX_UNIQUE_LINES: usize = 3;
pub(crate) const DENSITY_MAX_CHARS: usize = 500;

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub versions: Vec<String>,
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
