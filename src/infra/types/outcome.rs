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
