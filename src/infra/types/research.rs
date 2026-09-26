use std::collections::HashSet;

use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};

use super::{Capability, CapabilitySet, PlanCapability, ProviderAttempt};

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// A strictly parsed caller or classifier research plan using Schema v1.
pub struct ResearchPlan {
    /// Schema version, which must equal one.
    plan_version: u8,
    /// Evidence-policy signals that cannot alter capability selection.
    intent_signals: ResearchIntentSignals,
    /// Non-empty list of subquestions with unique identifiers.
    decomposition: Vec<ResearchSubquestion>,
}

impl ResearchPlan {
    /// Constructs a Schema v1 plan after enforcing every plan invariant.
    ///
    /// # Errors
    ///
    /// Returns an error when the version is unsupported or the decomposition
    /// violates a required field, identifier, or capability invariant.
    pub fn new(
        plan_version: u8,
        intent_signals: ResearchIntentSignals,
        decomposition: Vec<ResearchSubquestion>,
    ) -> Result<Self, String> {
        if plan_version != 1 {
            return Err(format!(
                "unsupported research plan version {plan_version}; expected version 1"
            ));
        }
        if decomposition.is_empty() {
            return Err("research plan decomposition must not be empty".into());
        }
        let mut plan = Self {
            plan_version,
            intent_signals,
            decomposition,
        };
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
            if subquestion.question.trim().is_empty() {
                return Err(format!(
                    "research plan decomposition `{}` question must not be empty",
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

    pub(crate) fn parse_json(input: &str) -> Result<Self, String> {
        serde_json::from_str(input).map_err(|error| format!("invalid research plan: {error}"))
    }

    /// Returns the validated schema version.
    #[must_use]
    pub const fn plan_version(&self) -> u8 {
        self.plan_version
    }

    /// Returns the plan's evidence-policy signals.
    #[must_use]
    pub const fn intent_signals(&self) -> &ResearchIntentSignals {
        &self.intent_signals
    }

    /// Returns the plan's ordered, non-empty decomposition.
    #[must_use]
    pub fn decomposition(&self) -> &[ResearchSubquestion] {
        &self.decomposition
    }

    pub(crate) fn truncate_decomposition(&mut self, limit: usize) -> usize {
        let original_len = self.decomposition.len();
        self.decomposition.truncate(limit.max(1));
        original_len
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawResearchPlan {
    plan_version: u8,
    intent_signals: ResearchIntentSignals,
    decomposition: Vec<ResearchSubquestion>,
}

impl<'de> Deserialize<'de> for ResearchPlan {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawResearchPlan::deserialize(deserializer)?;
        Self::new(raw.plan_version, raw.intent_signals, raw.decomposition)
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Serialize)]
/// A file-backed evidence item exposed through the Research Evidence Index.
pub struct EvidenceItem {
    /// Invocation-local stable identifier.
    pub id: String,
    /// Mutually exclusive evidence identity.
    #[serde(flatten)]
    pub locator: EvidenceLocator,
    /// Human-readable evidence title when the provider supplied one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Provider that returned the content.
    pub provider: &'static str,
    /// Whether the content came from Web Fetch or documentation retrieval.
    pub source_type: &'static str,
    /// Plan subquestions covered by this evidence; empty means plan-wide evidence.
    pub subquestion_ids: Vec<String>,
    #[serde(skip)]
    /// Full fetched or read content persisted at `path`.
    pub content: String,
    /// Character count of `content`.
    pub content_len: usize,
    /// Whether non-empty content passed the evidence boundary.
    pub verified: bool,
    /// Directly readable Markdown artifact containing the evidence body.
    pub path: String,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
/// A retrievable evidence identity.
pub enum EvidenceLocator {
    /// A real HTTP(S) URL consumed through Web Fetch.
    Url(String),
    /// A Context7 library consumed through `query-docs`.
    Context7Library(String),
}

impl EvidenceLocator {
    /// Returns the evidence URL when this locator has one.
    #[must_use]
    pub fn url(&self) -> Option<&str> {
        match self {
            Self::Url(url) => Some(url),
            Self::Context7Library(_) => None,
        }
    }

    /// Returns the Context7 library ID when this locator identifies one.
    #[must_use]
    pub fn library_id(&self) -> Option<&str> {
        match self {
            Self::Url(_) => None,
            Self::Context7Library(library_id) => Some(library_id),
        }
    }

    pub(crate) fn provider(&self) -> Option<&'static str> {
        match self {
            Self::Url(_) => None,
            Self::Context7Library(_) => Some("context7"),
        }
    }
}

impl Serialize for EvidenceLocator {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(None)?;
        match self {
            Self::Url(url) => map.serialize_entry("url", url)?,
            Self::Context7Library(library_id) => {
                map.serialize_entry("url", &Option::<&str>::None)?;
                map.serialize_entry("library_id", library_id)?;
            }
        }
        map.end()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct DocumentationEvidence {
    pub(crate) locator: EvidenceLocator,
    pub(crate) provider: &'static str,
    pub(crate) content: String,
    pub(crate) attempts: Vec<ProviderAttempt>,
    pub(crate) diagnostic: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
/// Count and readable path for candidates that were discovered but not fetched.
pub struct UnconsumedCandidates {
    /// Number of candidate records stored at `path`.
    pub count: usize,
    /// Directly readable JSON artifact containing the candidate metadata.
    pub path: String,
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

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{EvidenceLocator, ResearchPlan};

    #[test]
    fn evidence_locators_project_their_mutually_exclusive_public_identity() {
        let url =
            serde_json::to_value(EvidenceLocator::Url("https://example.test/evidence".into()))
                .expect("serialize URL locator");
        let context7 =
            serde_json::to_value(EvidenceLocator::Context7Library("/rust-lang/rust".into()))
                .expect("serialize Context7 locator");

        assert_eq!(
            (url, context7),
            (
                json!({"url": "https://example.test/evidence"}),
                json!({"url": null, "library_id": "/rust-lang/rust"}),
            )
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
        let mut empty_question = valid.clone();
        empty_question["decomposition"][0]["question"] = json!(" ");
        invalid.push(empty_question);
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

        assert!(invalid.iter().all(|plan| {
            ResearchPlan::parse_json(&plan.to_string()).is_err()
                && serde_json::from_value::<ResearchPlan>(plan.clone()).is_err()
        }));
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
