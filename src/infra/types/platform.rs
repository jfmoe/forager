//! Shared platform shapes: identities, references, search options, items, and result pages.
//!
//! Each platform's own shapes live in its own module and join the shared enums here as one
//! variant; see `docs/spec/forager/07-platforms.md`.

use std::fmt;

use serde::{Deserialize, Serialize, Serializer};
use thiserror::Error;

use super::ProviderAttempt;
use super::platform_arxiv::{ArxivItemData, ArxivRef, ArxivSearchOptions};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
/// A built-in external content source with its own identity space.
pub enum Platform {
    /// The arXiv preprint server.
    Arxiv,
}

impl Platform {
    /// Every built-in platform.
    pub const ALL: [Self; 1] = [Self::Arxiv];

    /// Returns the stable platform identifier used by commands and configuration.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Arxiv => "arxiv",
        }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// How much of a platform item a result carries; each platform defines what each depth means.
pub enum ContentDepth {
    /// A short excerpt.
    Snippet,
    /// Metadata and the author-written summary, never the full text.
    Abstract,
    /// The complete body.
    FullText,
    /// A post together with its conversation.
    Thread,
}

impl ContentDepth {
    /// Returns the stable depth identifier used by commands and output.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Snippet => "snippet",
            Self::Abstract => "abstract",
            Self::FullText => "full_text",
            Self::Thread => "thread",
        }
    }
}

#[derive(Debug, Error)]
#[error("unrecognized {platform} reference `{input}`; {hint}")]
/// An input that is not a recognizable reference or original URL for the platform.
pub struct PlatformRefError {
    pub(super) platform: Platform,
    pub(super) input: String,
    pub(super) hint: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// The typed identity of a platform entity.
///
/// The display form is the ref string, such as `arxiv:2401.01234v2`. Parsing the canonical URL
/// of a ref returns the same ref.
pub enum PlatformRef {
    /// An arXiv paper.
    Arxiv(ArxivRef),
}

impl PlatformRef {
    /// Parses a ref string or an original platform URL without any network access.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformRefError`] when the input is not recognizable for `platform`.
    pub fn parse(platform: Platform, input: &str) -> Result<Self, PlatformRefError> {
        match platform {
            Platform::Arxiv => ArxivRef::parse(input).map(Self::Arxiv),
        }
    }

    /// Returns the platform that owns this identity.
    #[must_use]
    pub const fn platform(&self) -> Platform {
        match self {
            Self::Arxiv(_) => Platform::Arxiv,
        }
    }

    /// Returns the platform-owned entity kind.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Arxiv(_) => "paper",
        }
    }

    /// Returns the canonical URL; it carries a version only when the ref does.
    #[must_use]
    pub fn canonical_url(&self) -> String {
        match self {
            Self::Arxiv(reference) => reference.canonical_url(),
        }
    }
}

impl fmt::Display for PlatformRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Arxiv(reference) => write!(formatter, "arxiv:{reference}"),
        }
    }
}

impl Serialize for PlatformRef {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "platform", rename_all = "snake_case")]
/// The typed search options of one platform.
pub enum PlatformSearchOptions {
    /// arXiv options.
    Arxiv(ArxivSearchOptions),
}

impl PlatformSearchOptions {
    /// Returns the default options of `platform`.
    #[must_use]
    pub fn defaults(platform: Platform) -> Self {
        match platform {
            Platform::Arxiv => Self::Arxiv(ArxivSearchOptions::default()),
        }
    }

    /// Returns the platform these options belong to.
    #[must_use]
    pub const fn platform(&self) -> Platform {
        match self {
            Self::Arxiv(_) => Platform::Arxiv,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// A platform search request; a page cursor encodes it completely.
pub(crate) struct PlatformSearchRequest {
    pub(crate) query: String,
    pub(crate) limit: u16,
    pub(crate) options: PlatformSearchOptions,
    /// The route-owned position of the requested page; absent for the first page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) page: Option<String>,
}

impl PlatformSearchRequest {
    /// Checks the cross-field rules that argument parsing cannot express.
    pub(crate) fn validate(&self) -> Result<(), String> {
        match &self.options {
            PlatformSearchOptions::Arxiv(options) => options.validate(&self.query, self.limit),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
/// One platform entity in a result.
pub struct PlatformItem {
    #[serde(rename = "ref")]
    pub reference: PlatformRef,
    pub url: String,
    pub depth: ContentDepth,
    pub title: String,
    pub authors: Vec<String>,
    pub published: Option<String>,
    #[serde(flatten)]
    pub data: PlatformItemData,
}

#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
/// The platform-owned fields of an item.
pub enum PlatformItemData {
    /// arXiv metadata.
    Arxiv(ArxivItemData),
}

#[derive(Clone, Debug, Serialize)]
/// One page of platform search results.
pub struct PlatformSearchPage {
    pub platform: Platform,
    /// The route that produced the page.
    pub provider: &'static str,
    pub items: Vec<PlatformItem>,
    /// An opaque cursor for the next page, or `null` on the last page.
    pub next_cursor: Option<String>,
    #[serde(rename = "provider_attempts", skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<ProviderAttempt>,
    #[serde(skip)]
    pub diagnostic: Option<String>,
}

/// One route's search result before cursor encoding.
pub(crate) struct PlatformSearchOutcome {
    pub(crate) items: Vec<PlatformItem>,
    /// The route-owned position of the next page, when one exists.
    pub(crate) next_page: Option<String>,
    pub(crate) attempts: Vec<ProviderAttempt>,
    pub(crate) diagnostic: Option<String>,
}

#[derive(Clone, Debug)]
/// A platform fetch request.
pub(crate) struct PlatformFetchRequest {
    pub(crate) reference: PlatformRef,
    pub(crate) depth: ContentDepth,
}

/// One route's fetch result before the full-text stage.
pub(crate) struct PlatformFetchOutcome {
    /// The metadata item; its ref carries the version the platform returned.
    pub(crate) item: PlatformItem,
    /// The full-text URLs to read in order; empty unless the request asks for the full text.
    pub(crate) content_urls: Vec<String>,
    pub(crate) attempts: Vec<ProviderAttempt>,
    pub(crate) diagnostic: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
/// One fetched platform item: its metadata and, at full-text depth, a reference to its body.
pub struct PlatformFetchResult {
    pub platform: Platform,
    /// The route that returned the metadata.
    pub provider: &'static str,
    #[serde(flatten)]
    pub item: PlatformItem,
    #[serde(flatten)]
    pub content: Option<PlatformContent>,
    #[serde(rename = "provider_attempts", skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<ProviderAttempt>,
    #[serde(skip)]
    pub diagnostic: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
/// The full text of a platform item; the body itself never serializes.
pub struct PlatformContent {
    /// The URL the body was read from.
    #[serde(rename = "content_url")]
    pub url: String,
    /// The Web Fetch provider that returned the body.
    #[serde(rename = "content_provider")]
    pub provider: &'static str,
    /// The local Markdown file that holds the body, once written.
    #[serde(rename = "content_path", skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The body length in characters.
    #[serde(rename = "content_len")]
    pub len: usize,
    #[serde(skip)]
    pub text: String,
}

impl PlatformContent {
    /// Wraps a body read from `url`; the path stays empty until the body is written.
    #[must_use]
    pub fn new(url: String, provider: &'static str, text: String) -> Self {
        Self {
            url,
            provider,
            path: None,
            len: text.chars().count(),
            text,
        }
    }
}
