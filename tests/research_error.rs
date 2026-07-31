use std::error::Error;

use forager::types::{AttemptErrorKind, ResearchError};

#[test]
fn research_error_exposes_its_message_through_the_standard_error_interface() {
    let error = ResearchError {
        kind: AttemptErrorKind::Evidence,
        message: "insufficient evidence".into(),
        attempts: Vec::new(),
        evidence_items: Vec::new(),
        capability_gaps: Vec::new(),
        diagnostic: None,
    };
    let standard_error: &dyn Error = &error;

    assert_eq!(standard_error.to_string(), "insufficient evidence");
}
