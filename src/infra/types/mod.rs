//! Zero-IO shared shapes: capabilities, platforms, research plans, errors, attempts, and outcomes.

mod attempt;
mod capability;
mod chatgpt_research;
mod deadline;
mod delegated_report;
mod error;
mod gemini_research;
mod outcome;
mod platform;
mod platform_arxiv;
mod platform_hn;
mod platform_polymarket;
mod platform_polymarket_history;
mod platform_polymarket_price;
mod platform_reddit;
mod platform_scholar;
mod platform_ssrn;
mod platform_ssrn_search;
mod platform_x;
mod platform_xiaohongshu;
mod platform_xiaohongshu_comments;
mod platform_youtube;
mod platform_youtube_comments;
mod platform_youtube_model;
mod platform_youtube_videos;
mod research;
mod search;

pub use attempt::{AttemptDisposition, AttemptTarget, ProviderAttempt};
pub use capability::{Capability, CapabilitySet, FallbackPolicy, PlanCapability};
pub(crate) use chatgpt_research::{CHATGPT_RESEARCH_RESULT, CHATGPT_RESEARCH_START};
pub use chatgpt_research::{
    ChatgptConversationId, ChatgptPlan, ChatgptPlanStep, ChatgptProgress, ChatgptResearchFailure,
    ChatgptResearchResult, ChatgptResearchStarted, ChatgptResearchState,
};
pub(crate) use deadline::{Deadline, MIN_USEFUL_SLICE_SECONDS};
pub use delegated_report::{DelegatedReport, ReportFiles, ReportSource};
pub use error::{AttemptErrorKind, ErrorFamily, ErrorKind, ProviderError};
pub(crate) use gemini_research::{GEMINI_RESEARCH_RESULT, GEMINI_RESEARCH_START};
pub use gemini_research::{
    GeminiConversationId, GeminiPlan, GeminiPlanStep, GeminiProgress, GeminiResearchFailure,
    GeminiResearchResult, GeminiResearchStarted, GeminiResearchState,
};
pub use outcome::{
    Context7DocsOutcome, Context7LibraryOutcome, Context7Outcome, ExaInput, ExaOutcome,
    FetchOutcome, JournalOutcome, LibraryCandidate, MapOutcome,
};
pub(crate) use outcome::{DENSITY_MAX_CHARS, DENSITY_MAX_UNIQUE_LINES, MIN_FETCH_CONTENT_CHARS};
pub(crate) use platform::{
    ASK, COMMENTS, FullTextSource, LocalFile, LocalMediaType, PlatformAskRequest,
    PlatformCommentsRequest, PlatformFetchOptions, PlatformFetchOutcome, PlatformFetchRequest,
    PlatformSearchOutcome, PlatformSearchRequest,
};
pub use platform::{
    ContentDepth, Platform, PlatformAskResult, PlatformCollection, PlatformCommentsPage,
    PlatformContent, PlatformFetchResult, PlatformItem, PlatformItemData, PlatformRef,
    PlatformRefError, PlatformSearchOptions, PlatformSearchPage, TranscriptInfo, TranscriptSource,
};
pub use platform_arxiv::{ArxivItemData, ArxivRef, ArxivSearchOptions, ArxivSort};
pub use platform_hn::{
    HnItemData, HnItemType, HnRankedItem, HnRef, HnSearchOptions, HnSearchType, HnSort,
    HnStoriesList, HnStoriesResult, HnThreadOrder,
};
pub(crate) use platform_hn::{HnStoriesOutcome, HnStoriesRequest, STORIES};
pub(crate) use platform_polymarket::{
    BookTop, Price, QuoteSource, event_status, market_status, price_market,
};
pub use platform_polymarket::{
    PolymarketBasis, PolymarketEventFields, PolymarketEventLink, PolymarketItemData,
    PolymarketKind, PolymarketMarket, PolymarketMarketFields, PolymarketOutcome,
    PolymarketPriceSource, PolymarketRef, PolymarketSearchOptions, PolymarketStatus,
    PolymarketTopMarket,
};
pub(crate) use platform_polymarket_history::{
    HISTORY, PolymarketHistoryOutcome, PolymarketHistoryRequest,
};
pub use platform_polymarket_history::{
    PolymarketBucket, PolymarketHistoryMarket, PolymarketHistoryResult, PolymarketInterval,
    PolymarketPricePoint, PolymarketWindow,
};
pub use platform_reddit::{
    RedditItemData, RedditRef, RedditSearchOptions, RedditSnippetSource, RedditSort,
    RedditThreadOrder, RedditTime,
};
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
pub(crate) use platform_x::{XAskOutcome, XAskRequest};
pub use platform_x::{
    XAskResult, XPostData, XPostRole, XRef, XSearchOptions, XSource, XThreadPost,
};
pub(crate) use platform_xiaohongshu::AccessToken;
pub use platform_xiaohongshu::{
    XiaohongshuImage, XiaohongshuItemData, XiaohongshuNoteData, XiaohongshuNoteType,
    XiaohongshuPublishTime, XiaohongshuRef, XiaohongshuSearchOptions, XiaohongshuSort,
    XiaohongshuVideo,
};
pub use platform_xiaohongshu_comments::{
    XiaohongshuComment, XiaohongshuCommentsPage, XiaohongshuReply,
};
pub(crate) use platform_xiaohongshu_comments::{
    XiaohongshuCommentsOutcome, XiaohongshuCommentsRequest,
};
pub use platform_youtube::{
    YoutubeCaptionSource, YoutubeCaptionTrack, YoutubeChapter, YoutubeDuration, YoutubeItemData,
    YoutubeKind, YoutubeLiveStatus, YoutubeRef, YoutubeSearchOptions, YoutubeSort,
};
pub(crate) use platform_youtube::{
    YoutubeCollection, YoutubeFetchOptions, chapters_from_description,
};
pub use platform_youtube_comments::{YoutubeComment, YoutubeCommentsPage, YoutubeReply};
pub(crate) use platform_youtube_comments::{
    YoutubeCommentSort, YoutubeCommentsOutcome, YoutubeCommentsRequest,
};
pub use platform_youtube_model::{
    AskedVideo, ModelUsage, PlatformTranscribeResult, YoutubeAskResult,
};
pub(crate) use platform_youtube_model::{
    ModelAnswerOutcome, ModelTranscriptOutcome, TRANSCRIBE, YoutubeAskRequest,
    YoutubeTranscribeRequest,
};
pub(crate) use platform_youtube_videos::{VIDEOS, YoutubeVideosOutcome, YoutubeVideosRequest};
pub(crate) use research::DocumentationEvidence;
pub use research::{
    ClaimRisk, EvidenceItem, EvidenceLocator, EvidenceStrength, RecencyRequirement, ResearchGap,
    ResearchGapCheck, ResearchIntentSignals, ResearchPlan, ResearchSubquestion,
    UnconsumedCandidates,
};
pub use search::{CapabilityGap, SearchCandidate, SearchOutcome, Source};
pub(crate) use search::{DocumentationSearchOutcome, SupplementalSearchOutcome};
