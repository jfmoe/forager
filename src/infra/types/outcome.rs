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
    /// Every attempt of the command; the binary lists them only under `--verbose`.
    #[serde(skip)]
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
    /// Every attempt of the command; the binary lists them only under `--verbose`.
    #[serde(skip)]
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
    /// Every attempt of the command; the binary lists them only under `--verbose`.
    #[serde(skip)]
    pub attempts: Vec<ProviderAttempt>,
    #[serde(skip)]
    pub diagnostic: Option<String>,
}
