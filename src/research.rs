use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::config::RuntimeConfig;
use crate::engine::{self, CapabilityExecution};
use crate::net::RetryPolicy;
use crate::providers::FetchRequest;
use crate::types::{
    AttemptErrorKind, Capability, CapabilityGap, Citation, Deadline, EvidenceItem,
    EvidenceStrength, PlanCapability, ProviderAttempt, ResearchError, ResearchGap,
    ResearchGapCheck, ResearchOutcome, ResearchPlan, Source,
};

#[derive(Clone, Copy, Debug)]
pub(crate) enum ResearchBudget {
    Quick,
    Standard,
    Deep,
}

impl ResearchBudget {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Quick => "quick",
            Self::Standard => "standard",
            Self::Deep => "deep",
        }
    }

    fn max_evidence(self) -> usize {
        match self {
            Self::Quick => 1,
            Self::Standard => 3,
            Self::Deep => 5,
        }
    }
}

pub(crate) struct ResearchRequest {
    pub(crate) query: String,
    pub(crate) plan: ResearchPlan,
    pub(crate) budget: ResearchBudget,
    pub(crate) evidence_dir: PathBuf,
    pub(crate) fallback: String,
}

struct Candidate {
    source: Source,
    subquestion_id: String,
    prefetched_provider: Option<&'static str>,
}

pub(crate) async fn execute(
    request: ResearchRequest,
    config: RuntimeConfig,
    client: reqwest::Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Result<ResearchOutcome, ResearchError> {
    let capabilities = request.plan.capabilities().iter().collect::<Vec<_>>();
    let mut attempts = Vec::new();
    let mut capability_gaps = Vec::new();
    let mut research_gaps = Vec::new();
    let mut diagnostics = Vec::new();
    let mut candidates = Vec::new();
    let limit = u16::try_from(request.budget.max_evidence()).unwrap_or(u16::MAX);

    if let Err(error) =
        write_json_artifact(&request.evidence_dir, "00-plan.json", &json!(request.plan))
    {
        return Err(runtime_error(
            format!("cannot write research plan artifact: {error}"),
            attempts,
            capability_gaps,
        ));
    }

    for subquestion in &request.plan.decomposition {
        for capability in &subquestion.required_capabilities {
            let execution =
                CapabilityExecution::new(&request.fallback, client.clone(), retry_policy, deadline);
            match capability {
                PlanCapability::DocsSearch => {
                    if config.docs_search.configured_provider_count() == 0 {
                        capability_gaps.push(CapabilityGap {
                            capability: Capability::DocsSearch,
                            reason: "no_configured_provider",
                            providers_skipped: config.docs_search.order.clone(),
                        });
                        continue;
                    }
                    match engine::documentation_search(
                        &subquestion.question,
                        limit,
                        config.docs_search.clone(),
                        execution,
                    )
                    .await
                    {
                        Ok(mut outcome) => {
                            attempts.append(&mut outcome.attempts);
                            push_diagnostic(&mut diagnostics, outcome.diagnostic);
                            let prefetched_provider = attempts
                                .iter()
                                .rev()
                                .find(|attempt| {
                                    attempt.seam == "docs_search" && attempt.error_kind.is_none()
                                })
                                .map(|attempt| attempt.provider)
                                .filter(|provider| *provider == "context7");
                            candidates.extend(outcome.sources.into_iter().map(|source| {
                                Candidate {
                                    source,
                                    subquestion_id: subquestion.id.clone(),
                                    prefetched_provider,
                                }
                            }));
                        }
                        Err(mut error) => {
                            attempts.append(&mut error.attempts);
                            push_diagnostic(&mut diagnostics, error.diagnostic);
                            capability_gaps.push(CapabilityGap {
                                capability: Capability::DocsSearch,
                                reason: "all_attempts_failed",
                                providers_skipped: unconfigured_docs_providers(&config),
                            });
                        }
                    }
                }
                PlanCapability::WebSearch => {
                    if config.web_search.configured_provider_count() == 0 {
                        capability_gaps.push(CapabilityGap {
                            capability: Capability::WebSearch,
                            reason: "no_configured_provider",
                            providers_skipped: config.web_search.order.clone(),
                        });
                        continue;
                    }
                    match engine::supplemental_web_search(
                        &subquestion.question,
                        limit,
                        config.web_search.clone(),
                        execution,
                    )
                    .await
                    {
                        Ok(mut outcome) => {
                            attempts.append(&mut outcome.attempts);
                            push_diagnostic(&mut diagnostics, outcome.diagnostic);
                            candidates.extend(outcome.sources.into_iter().map(|source| {
                                Candidate {
                                    source,
                                    subquestion_id: subquestion.id.clone(),
                                    prefetched_provider: None,
                                }
                            }));
                        }
                        Err(mut error) => {
                            attempts.append(&mut error.attempts);
                            push_diagnostic(&mut diagnostics, error.diagnostic);
                            capability_gaps.push(CapabilityGap {
                                capability: Capability::WebSearch,
                                reason: "all_attempts_failed",
                                providers_skipped: unconfigured_web_providers(&config),
                            });
                        }
                    }
                }
                PlanCapability::VerticalSearch => {
                    if config.vertical_search.configured_provider_count() == 0 {
                        capability_gaps.push(CapabilityGap {
                            capability: Capability::VerticalSearch,
                            reason: "no_configured_provider",
                            providers_skipped: config.vertical_search.order.clone(),
                        });
                        continue;
                    }
                    match engine::vertical_search(
                        &subquestion.question,
                        limit,
                        config.vertical_search.clone(),
                        execution,
                    )
                    .await
                    {
                        Ok(mut outcome) => {
                            attempts.append(&mut outcome.attempts);
                            push_diagnostic(&mut diagnostics, outcome.diagnostic);
                            candidates.extend(outcome.sources.into_iter().map(|source| {
                                Candidate {
                                    source,
                                    subquestion_id: subquestion.id.clone(),
                                    prefetched_provider: None,
                                }
                            }));
                        }
                        Err(mut error) => {
                            attempts.append(&mut error.attempts);
                            push_diagnostic(&mut diagnostics, error.diagnostic);
                            capability_gaps.push(CapabilityGap {
                                capability: Capability::VerticalSearch,
                                reason: "all_attempts_failed",
                                providers_skipped: unconfigured_vertical_providers(&config),
                            });
                        }
                    }
                }
            }
        }
    }

    let first_subquestion = request
        .plan
        .decomposition
        .first()
        .map_or("", |subquestion| subquestion.id.as_str());
    for url in engine::known_urls(&request.query) {
        let redacted_url = crate::config::redact_url(&url);
        candidates.insert(
            0,
            Candidate {
                source: Source {
                    title: redacted_url,
                    url,
                    published_date: None,
                    author: None,
                    text: None,
                    highlights: Vec::new(),
                },
                subquestion_id: first_subquestion.to_owned(),
                prefetched_provider: None,
            },
        );
    }
    for subquestion in &request.plan.decomposition {
        for url in engine::known_urls(&subquestion.question) {
            let redacted_url = crate::config::redact_url(&url);
            candidates.insert(
                0,
                Candidate {
                    source: Source {
                        title: redacted_url,
                        url,
                        published_date: None,
                        author: None,
                        text: None,
                        highlights: Vec::new(),
                    },
                    subquestion_id: subquestion.id.clone(),
                    prefetched_provider: None,
                },
            );
        }
    }

    let mut evidence_items = Vec::new();
    let mut seen_urls = HashSet::new();
    let mut fetch_config = config.web_fetch.clone();
    if request.fallback == "off" {
        fetch_config.order.truncate(1);
    }
    for candidate in candidates {
        if evidence_items.len() >= request.budget.max_evidence()
            || !seen_urls.insert(candidate.source.url.clone())
        {
            continue;
        }
        if let Some(provider) = candidate.prefetched_provider
            && candidate
                .source
                .text
                .as_deref()
                .is_some_and(|content| !content.trim().is_empty())
        {
            let content = candidate.source.text.unwrap_or_default();
            push_evidence(
                &mut evidence_items,
                candidate.source.url,
                candidate.source.title,
                provider,
                "docs",
                candidate.subquestion_id,
                content,
            );
            continue;
        }
        if fetch_config.configured_provider_count() == 0 {
            if !capability_gaps
                .iter()
                .any(|gap| gap.capability == Capability::WebFetch)
            {
                capability_gaps.push(CapabilityGap {
                    capability: Capability::WebFetch,
                    reason: "no_configured_provider",
                    providers_skipped: fetch_config.order.clone(),
                });
            }
            research_gaps.push(ResearchGap {
                subquestion_id: candidate.subquestion_id,
                reason:
                    "candidate could not be fetched because web_fetch has no configured provider"
                        .into(),
                url: Some(crate::config::redact_url(&candidate.source.url)),
            });
            continue;
        }
        let url = candidate.source.url.clone();
        match engine::fetch(
            FetchRequest {
                url: url.clone(),
                verbose: true,
            },
            fetch_config.clone(),
            client.clone(),
            retry_policy,
            deadline,
        )
        .await
        {
            Ok(mut fetched) => {
                attempts.append(&mut fetched.attempts);
                push_diagnostic(&mut diagnostics, fetched.diagnostic);
                let provider = fetched.provider;
                let content = fetched.content;
                push_evidence(
                    &mut evidence_items,
                    fetched.url,
                    candidate.source.title,
                    provider,
                    "fetched_page",
                    candidate.subquestion_id,
                    content,
                );
            }
            Err(mut error) => {
                attempts.append(&mut error.attempts);
                push_diagnostic(&mut diagnostics, error.diagnostic);
                research_gaps.push(ResearchGap {
                    subquestion_id: candidate.subquestion_id,
                    reason: "candidate fetch failed".into(),
                    url: Some(crate::config::redact_url(&url)),
                });
            }
        }
    }

    let required_evidence = if request.plan.intent_signals.cross_validation_need
        == EvidenceStrength::High
        || request.plan.intent_signals.source_authority_need == EvidenceStrength::High
    {
        2
    } else {
        1
    };
    let evidence_is_insufficient = evidence_items.len() < required_evidence;
    if evidence_is_insufficient {
        research_gaps.push(ResearchGap {
            subquestion_id: String::new(),
            reason: format!(
                "research required {required_evidence} evidence items but obtained {}",
                evidence_items.len()
            ),
            url: None,
        });
    }
    let diagnostic = diagnostics_and_gaps(&diagnostics, &capability_gaps);
    if evidence_is_insufficient {
        if let Err(error) = write_evidence_artifacts(&request.evidence_dir, &evidence_items) {
            return Err(runtime_error(
                format!("cannot write research evidence artifact: {error}"),
                attempts,
                capability_gaps,
            ));
        }
        let citations = citations(&evidence_items);
        let has_quality_failure = attempts
            .iter()
            .any(|attempt| attempt.error_kind == Some(AttemptErrorKind::Quality));
        let entire_chain_failed =
            !attempts.is_empty() && attempts.iter().all(|attempt| attempt.error_kind.is_some());
        let kind = if evidence_items.is_empty() && (has_quality_failure || entire_chain_failed) {
            engine::terminal_kind(&attempts)
        } else {
            AttemptErrorKind::Evidence
        };
        let error = ResearchError {
            kind,
            message: if kind == AttemptErrorKind::Evidence {
                format!(
                    "research required {required_evidence} evidence items but obtained {}",
                    evidence_items.len()
                )
            } else {
                "research providers did not produce a verifiable response".into()
            },
            attempts,
            evidence_items,
            capability_gaps,
            diagnostic,
        };
        let _ = write_json_artifact(
            &request.evidence_dir,
            "summary.json",
            &json!({
                "status": "error",
                "error_kind": error.kind.as_str(),
                "message": error.message,
                "citations": citations,
                "evidence_items": error.evidence_items,
                "provider_attempts": error.attempts,
                "capability_gaps": error.capability_gaps,
            }),
        );
        return Err(error);
    }

    write_evidence_artifacts(&request.evidence_dir, &evidence_items).map_err(|error| {
        runtime_error(
            format!("cannot write research evidence artifact: {error}"),
            attempts.clone(),
            capability_gaps.clone(),
        )
    })?;
    let citations = citations(&evidence_items);
    let final_answer = synthesize(&request.query, &evidence_items, &research_gaps);
    let gap_check = if research_gaps.is_empty() && capability_gaps.is_empty() {
        ResearchGapCheck {
            status: "closed",
            gaps: research_gaps,
            stop_reason: "evidence_converged",
        }
    } else {
        ResearchGapCheck {
            status: "degraded",
            gaps: research_gaps,
            stop_reason: "degraded_with_gaps",
        }
    };
    let fallback_used = fallback_used(&attempts);
    let outcome = ResearchOutcome {
        query: request.query,
        budget: request.budget.as_str(),
        plan_source: "caller",
        research_plan: request.plan,
        capabilities,
        content: final_answer.clone(),
        final_answer,
        citations,
        evidence_items,
        capability_gaps,
        degraded: gap_check.status != "closed",
        gap_check,
        fallback_used,
        evidence_dir: request.evidence_dir.display().to_string(),
        attempts,
        diagnostic,
    };
    let mut summary = serde_json::to_value(&outcome).map_err(|error| {
        runtime_error(
            format!("cannot serialize research summary: {error}"),
            outcome.attempts.clone(),
            outcome.capability_gaps.clone(),
        )
    })?;
    summary
        .as_object_mut()
        .expect("research outcome serializes as object")
        .insert("provider_attempts".into(), json!(outcome.attempts));
    write_json_artifact(&request.evidence_dir, "summary.json", &summary).map_err(|error| {
        runtime_error(
            format!("cannot write research summary artifact: {error}"),
            outcome.attempts.clone(),
            outcome.capability_gaps.clone(),
        )
    })?;
    Ok(outcome)
}

fn push_evidence(
    evidence_items: &mut Vec<EvidenceItem>,
    url: String,
    title: String,
    provider: &'static str,
    source_type: &'static str,
    subquestion_id: String,
    content: String,
) {
    let content_len = content.chars().count();
    evidence_items.push(EvidenceItem {
        id: format!("e{}", evidence_items.len() + 1),
        url,
        title,
        provider,
        source_type,
        subquestion_id,
        content,
        content_len,
        verified: content_len > 0,
    });
}

fn synthesize(query: &str, evidence: &[EvidenceItem], gaps: &[ResearchGap]) -> String {
    let mut report = format!("Research result for: {query}\n\nEvidence-backed findings:");
    for (index, item) in evidence.iter().enumerate() {
        let excerpt = bounded_excerpt(&item.content, 360);
        report.push_str(&format!(
            "\n{}. {} ({})\n   Evidence excerpt: {}\n   Source: {}",
            index + 1,
            item.title,
            item.provider,
            excerpt,
            item.url
        ));
    }
    if !gaps.is_empty() {
        report.push_str("\n\nUnverified gaps:");
        for gap in gaps {
            report.push_str(&format!("\n- {}: {}", gap.subquestion_id, gap.reason));
        }
    }
    report
}

fn bounded_excerpt(content: &str, limit: usize) -> String {
    let mut excerpt = String::with_capacity(limit);
    let mut remaining = limit;
    for word in content.split_whitespace() {
        if !excerpt.is_empty() {
            if remaining == 0 {
                break;
            }
            excerpt.push(' ');
            remaining -= 1;
        }
        for character in word.chars() {
            if remaining == 0 {
                return excerpt;
            }
            excerpt.push(character);
            remaining -= 1;
        }
    }
    excerpt
}

fn citations(evidence_items: &[EvidenceItem]) -> Vec<Citation> {
    evidence_items
        .iter()
        .map(|item| Citation {
            url: item.url.clone(),
            title: item.title.clone(),
            provider: item.provider,
        })
        .collect()
}

fn write_evidence_artifacts(root: &Path, items: &[EvidenceItem]) -> std::io::Result<()> {
    for (index, item) in items.iter().enumerate() {
        fs::write(
            root.join(format!("{:02}-evidence.md", index + 1)),
            &item.content,
        )?;
    }
    Ok(())
}

fn write_json_artifact(root: &Path, name: &str, value: &Value) -> std::io::Result<()> {
    fs::create_dir_all(root)?;
    let encoded = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    fs::write(root.join(name), encoded)
}

fn runtime_error(
    message: String,
    attempts: Vec<ProviderAttempt>,
    capability_gaps: Vec<CapabilityGap>,
) -> ResearchError {
    ResearchError {
        kind: AttemptErrorKind::Runtime,
        message,
        attempts,
        evidence_items: Vec::new(),
        capability_gaps,
        diagnostic: None,
    }
}

fn push_diagnostic(diagnostics: &mut Vec<String>, diagnostic: Option<String>) {
    if let Some(diagnostic) = diagnostic {
        diagnostics.push(diagnostic);
    }
}

fn diagnostics_and_gaps(
    diagnostics: &[String],
    capability_gaps: &[CapabilityGap],
) -> Option<String> {
    let mut messages = diagnostics.to_vec();
    messages.extend(capability_gaps.iter().map(|gap| {
        format!(
            "capability gap: {} ({})",
            gap.capability.as_str(),
            gap.reason
        )
    }));
    (!messages.is_empty()).then(|| messages.join("\n"))
}

fn unconfigured_docs_providers(config: &RuntimeConfig) -> Vec<String> {
    config
        .docs_search
        .order
        .iter()
        .filter(|provider| {
            config
                .docs_search
                .provider(provider)
                .is_none_or(|provider| !provider.configured())
        })
        .cloned()
        .collect()
}

fn unconfigured_web_providers(config: &RuntimeConfig) -> Vec<String> {
    config
        .web_search
        .order
        .iter()
        .filter(|provider| {
            config
                .web_search
                .provider(provider)
                .is_none_or(|provider| provider.keys.is_empty())
        })
        .cloned()
        .collect()
}

fn unconfigured_vertical_providers(config: &RuntimeConfig) -> Vec<String> {
    config
        .vertical_search
        .order
        .iter()
        .filter(|provider| {
            config
                .vertical_search
                .provider(provider)
                .is_none_or(|provider| provider.keys.is_empty())
        })
        .cloned()
        .collect()
}

fn fallback_used(attempts: &[ProviderAttempt]) -> bool {
    let mut providers_by_seam = HashMap::<&str, HashSet<&str>>::new();
    for attempt in attempts {
        providers_by_seam
            .entry(attempt.seam)
            .or_default()
            .insert(attempt.provider);
    }
    providers_by_seam
        .values()
        .any(|providers| providers.len() > 1)
}
