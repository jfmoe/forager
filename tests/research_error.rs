use std::error::Error;

use forager::types::{AttemptErrorKind, ResearchError, ResearchGapCheck, UnconsumedCandidates};

#[test]
fn research_error_exposes_its_message_through_the_standard_error_interface() {
    let error = ResearchError {
        kind: AttemptErrorKind::Evidence,
        message: "insufficient evidence".into(),
        attempts: Vec::new(),
        evidence_items: Vec::new(),
        capability_gaps: Vec::new(),
        gap_check: ResearchGapCheck {
            status: "degraded",
            gaps: Vec::new(),
            stop_reason: "insufficient_evidence",
        },
        evidence_dir: "/tmp/evidence".into(),
        plan_path: "/tmp/evidence/00-plan.json".into(),
        unconsumed_candidates: UnconsumedCandidates {
            count: 0,
            path: "/tmp/evidence/candidates.json".into(),
        },
        synthesis_policy: "fetch_before_claim",
        diagnostic: None,
    };
    let standard_error: &dyn Error = &error;

    assert_eq!(standard_error.to_string(), "insufficient evidence");
}
