//! Zero-IO shared shapes: capabilities, platforms, research plans, errors, attempts, and outcomes.

mod attempt;
mod capability;
mod deadline;
mod error;
mod gemini_research;
mod outcome;
mod platform;
mod platform_arxiv;
mod platform_scholar;
mod platform_ssrn;
mod platform_ssrn_search;
mod platform_xiaohongshu;
mod platform_xiaohongshu_comments;
mod research;
mod search;

pub use attempt::{AttemptDisposition, AttemptTarget, ProviderAttempt};
pub use capability::{Capability, CapabilitySet, FallbackPolicy, PlanCapability};
pub(crate) use deadline::{Deadline, MIN_USEFUL_SLICE_SECONDS};
pub use error::{AttemptErrorKind, ErrorFamily, ErrorKind, ProviderError};
pub(crate) use gemini_research::{GEMINI_RESEARCH_RESULT, GEMINI_RESEARCH_START};
pub use gemini_research::{
    GeminiConversationId, GeminiPlan, GeminiPlanStep, GeminiProgress, GeminiReport,
    GeminiReportFiles, GeminiResearchFailure, GeminiResearchResult, GeminiResearchState,
    GeminiSource,
};
pub use outcome::{
    AnysearchDomain, AnysearchDomainsOutcome, AnysearchOutcome, AnysearchResult,
    AnysearchSearchOutcome, Context7DocsOutcome, Context7LibraryOutcome, Context7Outcome, ExaInput,
    ExaOutcome, FetchOutcome, JournalOutcome, LibraryCandidate, MapOutcome, SchemaValidation,
};
pub(crate) use outcome::{DENSITY_MAX_CHARS, DENSITY_MAX_UNIQUE_LINES, MIN_FETCH_CONTENT_CHARS};
pub use platform::{
    ContentDepth, Platform, PlatformContent, PlatformFetchResult, PlatformItem, PlatformItemData,
    PlatformRef, PlatformRefError, PlatformSearchOptions, PlatformSearchPage,
};
pub(crate) use platform::{
    FullTextSource, LocalFile, LocalMediaType, PlatformFetchOutcome, PlatformFetchRequest,
    PlatformSearchOutcome, PlatformSearchRequest,
};
pub use platform_arxiv::{ArxivItemData, ArxivRef, ArxivSearchOptions, ArxivSort};
pub(crate) use platform_scholar::{
    CITED_BY, SCHOLAR_MAX_LIMIT, ScholarCitedByRequest, ScholarCitedBySort,
};
pub use platform_scholar::{
    ScholarCluster, ScholarItemData, ScholarRef, ScholarResource, ScholarResult,
    ScholarSearchOptions, ScholarVersion,
};
pub use platform_ssrn::{SsrnItemData, SsrnRef};
pub use platform_ssrn_search::{
    SsrnDatePreset, SsrnDateRange, SsrnSearchMode, SsrnSearchOptions, SsrnSearchScope, SsrnSort,
    SsrnSortOrder, SsrnWorkType,
};
pub(crate) use platform_xiaohongshu::AccessToken;
pub use platform_xiaohongshu::{
    XiaohongshuImage, XiaohongshuItemData, XiaohongshuNoteData, XiaohongshuNoteType,
    XiaohongshuPublishTime, XiaohongshuRef, XiaohongshuSearchOptions, XiaohongshuSort,
    XiaohongshuVideo,
};
pub(crate) use platform_xiaohongshu_comments::{
    COMMENTS, XiaohongshuCommentsOutcome, XiaohongshuCommentsRequest,
};
pub use platform_xiaohongshu_comments::{
    XiaohongshuComment, XiaohongshuCommentsPage, XiaohongshuReply,
};
pub(crate) use research::DocumentationEvidence;
pub use research::{
    ClaimRisk, EvidenceItem, EvidenceLocator, EvidenceStrength, RecencyRequirement, ResearchGap,
    ResearchGapCheck, ResearchIntentSignals, ResearchPlan, ResearchSubquestion,
    UnconsumedCandidates,
};
pub use search::{CapabilityGap, SearchCandidate, SearchOutcome, Source};
pub(crate) use search::{
    DocumentationSearchOutcome, SupplementalSearchOutcome, VerticalSearchOutcome,
};
