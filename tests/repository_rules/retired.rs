//! Identifiers of retired routes, providers, capabilities and commands, and where they may still
//! appear.

/// Identifiers of retired routes, providers, capabilities and commands. A retired command
/// contributes its command phrase and the output fields and terms only it used.
pub const RETIRED: &[&str] = &[
    "opencli",
    "ssrn_browser",
    "reddit_web",
    "openai_compatible",
    "anysearch",
    "gemini_browser",
    "chatgpt_browser",
    "vertical_search",
    "forager research",
    "research_plan",
    "research evidence pipeline",
    "research evidence index",
    "research recovery manifest",
    "evidence_items",
    "evidence_dir",
    "plan_path",
    "unconsumed_candidates",
    "synthesis_policy",
    "gap_check",
    "summary_path",
    "fetch_before_claim",
    "classifier",
];

/// Paths where a retired identifier legitimately appears, each with its reason. A `*` matches
/// any run of characters.
pub const ALLOWED: &[(&str, &str)] = &[
    ("docs/adr/*", "ADRs record what was retired and why"),
    (
        "tests/*_retirement.rs",
        "retirement tests prove the retired name is rejected",
    ),
    ("tests/repository_rules/retired.rs", "this list"),
];

pub fn allowed(path: &str) -> bool {
    ALLOWED
        .iter()
        .any(|(pattern, _)| match pattern.split_once('*') {
            Some((prefix, suffix)) => {
                path.len() >= prefix.len() + suffix.len()
                    && path.starts_with(prefix)
                    && path.ends_with(suffix)
            }
            None => path == *pattern,
        })
}

/// The retired identifiers `text` mentions, ignoring case and treating `-` and `_` as equal.
pub fn mentions(text: &str) -> Vec<&'static str> {
    let normalized = text.to_lowercase().replace('-', "_");
    RETIRED
        .iter()
        .copied()
        .filter(|identifier| normalized.contains(identifier))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_a_retired_identifier_in_any_case_or_separator() {
        assert_eq!(
            mentions("Install the OpenCLI adapter, then try `ssrn-browser`."),
            ["opencli", "ssrn_browser"]
        );
    }
}
