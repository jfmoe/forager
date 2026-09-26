use serde::Serialize;

use super::{AnysearchResult, Capability, EvidenceLocator, LibraryCandidate, ProviderAttempt};

#[derive(Clone, Debug, Serialize)]
/// A normalized source returned by a search provider.
pub struct Source {
    #[serde(skip_serializing_if = "String::is_empty")]
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
    #[serde(skip)]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub favicon: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(transparent)]
struct CandidateUrl(String);

impl CandidateUrl {
    fn parse(value: String) -> Option<Self> {
        reqwest::Url::parse(&value)
            .ok()
            .filter(|url| matches!(url.scheme(), "http" | "https") && url.host().is_some())
            .map(|_| Self(value))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct EmptyCandidateData {}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct Context7CandidateData {
    library_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_snippets: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    trust_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    benchmark_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stars: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    versions: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct ExaCandidateData {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    highlights: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    published_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    favicon: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct AnysearchCandidateData {
    #[serde(skip_serializing_if = "Option::is_none")]
    evidence_type: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
enum SearchCandidateProviderData {
    Context7(Context7CandidateData),
    Exa(ExaCandidateData),
    Anysearch(AnysearchCandidateData),
    Empty(EmptyCandidateData),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
/// A non-primary search result that callers can evaluate before fetching its content.
pub struct SearchCandidate {
    provider: &'static str,
    capability: Capability,
    title: Option<String>,
    url: Option<CandidateUrl>,
    summary: Option<String>,
    provider_data: SearchCandidateProviderData,
}

impl SearchCandidate {
    pub(crate) fn from_source(
        source: Source,
        provider: &'static str,
        capability: Capability,
    ) -> Option<Self> {
        let Source {
            title,
            url,
            text,
            highlights,
            ..
        } = source;
        Some(Self {
            provider,
            capability,
            title: (!title.trim().is_empty()).then_some(title),
            url: Some(CandidateUrl::parse(url)?),
            summary: text
                .or_else(|| highlights.into_iter().next())
                .filter(|summary| !summary.trim().is_empty()),
            provider_data: SearchCandidateProviderData::Empty(EmptyCandidateData {}),
        })
    }

    pub(crate) fn from_exa_source(source: Source) -> Option<Self> {
        let Source {
            title,
            url,
            published_date,
            author,
            highlights,
            id,
            image,
            favicon,
            ..
        } = source;
        Some(Self {
            provider: "exa",
            capability: Capability::DocsSearch,
            title: (!title.trim().is_empty()).then_some(title),
            url: Some(CandidateUrl::parse(url)?),
            summary: highlights
                .first()
                .filter(|summary| !summary.trim().is_empty())
                .cloned(),
            provider_data: SearchCandidateProviderData::Exa(ExaCandidateData {
                id,
                highlights,
                published_date,
                author,
                image,
                favicon,
            }),
        })
    }

    pub(crate) fn from_context7_library(library: LibraryCandidate) -> Option<Self> {
        if library.id.trim().is_empty() {
            return None;
        }
        Some(Self {
            provider: "context7",
            capability: Capability::DocsSearch,
            title: (!library.title.trim().is_empty()).then_some(library.title),
            url: None,
            summary: (!library.description.trim().is_empty()).then_some(library.description),
            provider_data: SearchCandidateProviderData::Context7(Context7CandidateData {
                library_id: library.id,
                total_snippets: library.total_snippets,
                trust_score: library.trust_score,
                benchmark_score: library.benchmark_score,
                stars: library.stars,
                versions: library.versions,
            }),
        })
    }

    pub(crate) fn from_vertical_result(result: AnysearchResult) -> Self {
        let url = CandidateUrl::parse(result.url);
        Self {
            provider: "anysearch",
            capability: Capability::VerticalSearch,
            title: (!result.title.trim().is_empty()).then_some(result.title),
            url,
            summary: (!result.description.trim().is_empty()).then_some(result.description),
            provider_data: SearchCandidateProviderData::Anysearch(AnysearchCandidateData {
                evidence_type: result.evidence_type,
            }),
        }
    }

    pub(crate) fn from_web_fetch(
        provider: &'static str,
        url: String,
        summary: String,
    ) -> Option<Self> {
        Some(Self {
            provider,
            capability: Capability::WebFetch,
            title: None,
            url: Some(CandidateUrl::parse(url)?),
            summary: Some(summary),
            provider_data: SearchCandidateProviderData::Empty(EmptyCandidateData {}),
        })
    }

    /// Returns the provider that discovered this candidate.
    #[must_use]
    pub fn provider(&self) -> &'static str {
        self.provider
    }

    /// Returns the provider-native title when one was supplied.
    #[must_use]
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Returns the candidate's HTTP(S) URL when it has one.
    #[must_use]
    pub fn url(&self) -> Option<&str> {
        self.url.as_ref().map(|url| url.0.as_str())
    }

    /// Returns the provider-native summary when one was supplied.
    #[must_use]
    pub fn summary(&self) -> Option<&str> {
        self.summary.as_deref()
    }

    pub(crate) fn capability(&self) -> Capability {
        self.capability
    }

    pub(crate) fn into_evidence_source(self) -> Option<(EvidenceLocator, Source)> {
        let locator = match (self.url, self.provider_data) {
            (Some(url), _) => EvidenceLocator::Url(url.0),
            (None, SearchCandidateProviderData::Context7(data)) => {
                EvidenceLocator::Context7Library(data.library_id)
            }
            (None, _) => return None,
        };
        let source = Source {
            title: self.title.unwrap_or_default(),
            url: locator.url().unwrap_or_default().to_owned(),
            published_date: None,
            author: None,
            text: self.summary,
            highlights: Vec::new(),
            id: None,
            image: None,
            favicon: None,
        };
        Some((locator, source))
    }
}

#[derive(Clone, Debug, Serialize)]
/// The normalized result of a search, including enrichment and capability coverage.
pub struct SearchOutcome {
    #[serde(skip)]
    pub provider: &'static str,
    #[serde(skip)]
    pub query: String,
    #[serde(skip)]
    pub model: String,
    pub answer: String,
    pub sources: Vec<Source>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_sources: Vec<SearchCandidate>,
    #[serde(skip)]
    pub capabilities: Vec<Capability>,
    pub capability_gaps: Vec<CapabilityGap>,
    #[serde(skip)]
    pub attempts: Vec<ProviderAttempt>,
    #[serde(skip)]
    pub diagnostic: Option<String>,
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
pub(crate) struct DocumentationSearchOutcome {
    pub(crate) candidate_sources: Vec<SearchCandidate>,
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
