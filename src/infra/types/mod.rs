//! Zero-IO shared shapes: capabilities, research plans, errors, attempts, and outcomes.

mod attempt;
mod capability;
mod deadline;
mod error;
mod outcome;
mod research;
mod search;

pub use attempt::{AttemptDisposition, AttemptTarget, ProviderAttempt};
pub use capability::{Capability, CapabilitySet, FallbackPolicy, PlanCapability};
pub(crate) use deadline::{Deadline, MIN_USEFUL_SLICE_SECONDS};
pub use error::{AttemptErrorKind, ErrorFamily, ErrorKind, ProviderError};
pub use outcome::{
    AnysearchDomain, AnysearchDomainsOutcome, AnysearchOutcome, AnysearchResult,
    AnysearchSearchOutcome, Context7DocsOutcome, Context7LibraryOutcome, Context7Outcome, ExaInput,
    ExaOutcome, FetchOutcome, JournalOutcome, LibraryCandidate, MapOutcome, SchemaValidation,
};
pub(crate) use outcome::{DENSITY_MAX_CHARS, DENSITY_MAX_UNIQUE_LINES, MIN_FETCH_CONTENT_CHARS};
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
