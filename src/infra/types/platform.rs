//! Shared platform shapes: identities, references, search options, items, and result pages.
//!
//! Each platform's own shapes live in its own module and join the shared enums here as one
//! variant; see `docs/spec/forager/07-platforms.md`.

use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize, Serializer};
use thiserror::Error;

use super::ProviderAttempt;
use super::platform_arxiv::{ArxivItemData, ArxivRef, ArxivSearchOptions};
use super::platform_scholar::{ScholarItemData, ScholarRef, ScholarSearchOptions};
use super::platform_ssrn::{SsrnItemData, SsrnRef};
use super::platform_ssrn_search::SsrnSearchOptions;
use super::platform_xiaohongshu::{
    AccessToken, XiaohongshuItemData, XiaohongshuNoteData, XiaohongshuRef, XiaohongshuSearchOptions,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
/// A built-in external content source with its own identity space.
pub enum Platform {
    /// The arXiv preprint server.
    Arxiv,
    /// The Social Science Research Network (SSRN).
    Ssrn,
    /// Google Scholar, an index of papers across publishers.
    Scholar,
    /// Xiaohongshu (小红书), a social platform of user notes.
    Xiaohongshu,
}

impl Platform {
    /// Every built-in platform.
    pub const ALL: [Self; 4] = [Self::Arxiv, Self::Ssrn, Self::Scholar, Self::Xiaohongshu];

    /// Returns the stable platform identifier used by commands and configuration.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Arxiv => "arxiv",
            Self::Ssrn => "ssrn",
            Self::Scholar => "scholar",
            Self::Xiaohongshu => "xiaohongshu",
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
    /// Bibliographic information only, without an abstract.
    Metadata,
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
            Self::Metadata => "metadata",
            Self::Snippet => "snippet",
            Self::Abstract => "abstract",
            Self::FullText => "full_text",
            Self::Thread => "thread",
        }
    }
}

#[derive(Debug, Error)]
/// An input that is not a recognizable reference or original URL for the platform.
pub struct PlatformRefError {
    pub(super) platform: Platform,
    /// The input to echo, or `None` for a platform whose URLs may carry an access token.
    pub(super) input: Option<String>,
    pub(super) hint: &'static str,
}

impl fmt::Display for PlatformRefError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "unrecognized {} reference", self.platform)?;
        if let Some(input) = &self.input {
            write!(formatter, " `{input}`")?;
        }
        write!(formatter, "; {}", self.hint)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// The typed identity of a platform entity.
///
/// The display form is the ref string, such as `arxiv:2401.01234v2`. Parsing the canonical URL
/// of a ref returns the same ref.
pub enum PlatformRef {
    /// An arXiv paper.
    Arxiv(ArxivRef),
    /// An SSRN paper.
    Ssrn(SsrnRef),
    /// A Google Scholar paper cluster.
    Scholar(ScholarRef),
    /// A Xiaohongshu note.
    Xiaohongshu(XiaohongshuRef),
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
            Platform::Ssrn => SsrnRef::parse(input).map(Self::Ssrn),
            Platform::Scholar => ScholarRef::parse(input).map(Self::Scholar),
            Platform::Xiaohongshu => XiaohongshuRef::parse(input).map(Self::Xiaohongshu),
        }
    }

    /// Returns the platform that owns this identity.
    #[must_use]
    pub const fn platform(&self) -> Platform {
        match self {
            Self::Arxiv(_) => Platform::Arxiv,
            Self::Ssrn(_) => Platform::Ssrn,
            Self::Scholar(_) => Platform::Scholar,
            Self::Xiaohongshu(_) => Platform::Xiaohongshu,
        }
    }

    /// Returns the platform-owned entity kind.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Arxiv(_) | Self::Ssrn(_) | Self::Scholar(_) => "paper",
            Self::Xiaohongshu(_) => "note",
        }
    }

    /// Returns the canonical URL; it carries a version only when the ref does.
    #[must_use]
    pub fn canonical_url(&self) -> String {
        match self {
            Self::Arxiv(reference) => reference.canonical_url(),
            Self::Ssrn(reference) => reference.canonical_url(),
            Self::Scholar(reference) => reference.canonical_url(),
            Self::Xiaohongshu(reference) => reference.canonical_url(),
        }
    }
}

impl fmt::Display for PlatformRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Arxiv(reference) => write!(formatter, "arxiv:{reference}"),
            Self::Ssrn(reference) => write!(formatter, "ssrn:{reference}"),
            Self::Scholar(reference) => write!(formatter, "scholar:{reference}"),
            Self::Xiaohongshu(reference) => write!(formatter, "xiaohongshu:{reference}"),
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
    /// SSRN options.
    Ssrn(SsrnSearchOptions),
    /// Google Scholar options.
    Scholar(ScholarSearchOptions),
    /// Xiaohongshu options.
    Xiaohongshu(XiaohongshuSearchOptions),
}

impl PlatformSearchOptions {
    /// Returns the default options of `platform`.
    #[must_use]
    pub fn defaults(platform: Platform) -> Self {
        match platform {
            Platform::Arxiv => Self::Arxiv(ArxivSearchOptions::default()),
            Platform::Ssrn => Self::Ssrn(SsrnSearchOptions::default()),
            Platform::Scholar => Self::Scholar(ScholarSearchOptions::default()),
            Platform::Xiaohongshu => Self::Xiaohongshu(XiaohongshuSearchOptions::default()),
        }
    }

    /// Returns the platform these options belong to.
    #[must_use]
    pub const fn platform(&self) -> Platform {
        match self {
            Self::Arxiv(_) => Platform::Arxiv,
            Self::Ssrn(_) => Platform::Ssrn,
            Self::Scholar(_) => Platform::Scholar,
            Self::Xiaohongshu(_) => Platform::Xiaohongshu,
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
            PlatformSearchOptions::Ssrn(options) => options.validate(&self.query, self.limit),
            PlatformSearchOptions::Scholar(options) => options.validate(&self.query, self.limit),
            PlatformSearchOptions::Xiaohongshu(_) => {
                XiaohongshuSearchOptions::validate(&self.query, self.limit)
            }
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
    /// SSRN metadata.
    Ssrn(SsrnItemData),
    /// Google Scholar metadata.
    Scholar(ScholarItemData),
    /// Xiaohongshu note-card fields of a search result.
    Xiaohongshu(XiaohongshuItemData),
    /// Xiaohongshu fields of a fetched note.
    XiaohongshuNote(XiaohongshuNoteData),
}

#[derive(Clone, Debug, Serialize)]
/// One page of platform search results.
pub struct PlatformSearchPage {
    pub platform: Platform,
    /// The route that produced the page.
    pub provider: &'static str,
    pub items: Vec<PlatformItem>,
    /// An opaque cursor for the next page, or `null` when the route issues no cursor that
    /// another command can continue from. `null` alone does not prove the results are
    /// exhausted: Xiaohongshu search never issues a cursor.
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
    /// The access token that opens the entity, for a platform that needs one; it never reaches
    /// the ref, the canonical URL, or the journal.
    pub(crate) access: Option<AccessToken>,
}

/// One route's fetch result before the full-text stage.
pub(crate) struct PlatformFetchOutcome {
    /// The metadata item; its ref carries the version the platform returned.
    pub(crate) item: PlatformItem,
    /// The full-text source; `Urls` is empty unless the request asks for the full text.
    pub(crate) content_source: FullTextSource,
    pub(crate) attempts: Vec<ProviderAttempt>,
    pub(crate) diagnostic: Option<String>,
}

#[derive(Clone, Debug)]
/// Where the full text of an item lives: URLs to read in order, one local file the route
/// produced and verified in the same command, or the Markdown body the route itself read and
/// verified in the same attempt.
pub(crate) enum FullTextSource {
    /// URLs to read in order; empty unless the request asks for the full text.
    Urls(Vec<String>),
    /// One verified local file.
    LocalFile(LocalFile),
    /// The platform-native Markdown body; it needs no Web Fetch provider.
    Native(String),
}

#[derive(Clone, Debug)]
/// A local file one stage of a command hands to a later stage, with its media type.
pub(crate) struct LocalFile {
    pub(crate) path: PathBuf,
    pub(crate) media_type: LocalMediaType,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// The media type of a [`LocalFile`].
pub(crate) enum LocalMediaType {
    /// A PDF document.
    Pdf,
}

impl LocalMediaType {
    /// Returns the MIME type string.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Pdf => "application/pdf",
        }
    }

    /// Returns the conventional file extension.
    pub(crate) const fn extension(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
        }
    }
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
    /// The URL the body was read from; for a body converted from a local file or read natively
    /// by the route, the item's canonical URL, which names the source but may not open
    /// anonymously.
    #[serde(rename = "content_url")]
    pub url: String,
    /// The provider that produced the Markdown: the Web Fetch provider, or the route itself for
    /// a native body.
    #[serde(rename = "content_provider")]
    pub provider: &'static str,
    /// The local Markdown file that holds the body, once written.
    #[serde(rename = "content_path", skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The body length in characters.
    #[serde(rename = "content_len")]
    pub len: usize,
    /// The original PDF kept next to the Markdown, when the caller asked to keep it.
    #[serde(rename = "pdf_path", skip_serializing_if = "Option::is_none")]
    pub pdf_path: Option<String>,
    /// The kept PDF size in bytes.
    #[serde(rename = "pdf_bytes", skip_serializing_if = "Option::is_none")]
    pub pdf_bytes: Option<u64>,
    #[serde(skip)]
    pub text: String,
    /// The local file the body was converted from; delivery keeps or removes it.
    #[serde(skip)]
    pub(crate) source_file: Option<LocalFile>,
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
            pdf_path: None,
            pdf_bytes: None,
            text,
            source_file: None,
        }
    }
}
