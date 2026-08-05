use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use futures_util::{StreamExt, future::join_all, stream};
use serde_json::{Value, json};

use crate::config::RuntimeConfig;
use crate::engine::{self, CapabilityExecution};
use crate::net::RetryPolicy;
use crate::providers::FetchRequest;
use crate::redact::redact_url;
use crate::types::{
    AttemptErrorKind, Capability, CapabilityGap, Citation, Deadline, EvidenceItem,
    EvidenceStrength, PlanCapability, ProviderAttempt, ResearchError, ResearchGap,
    ResearchGapCheck, ResearchOutcome, ResearchPlan, ResearchSubquestion, Source,
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

    pub(crate) fn max_subquestions(self) -> usize {
        match self {
            Self::Quick => 2,
            Self::Standard => 4,
            Self::Deep => 6,
        }
    }

    fn evidence_cap(self) -> usize {
        match self {
            Self::Quick => 1,
            Self::Standard => 2,
            Self::Deep => 3,
        }
    }

    fn discovery_limit(self) -> usize {
        self.evidence_cap() * 3
    }
}

pub(crate) struct ResearchRequest {
    pub(crate) query: String,
    pub(crate) plan: ResearchPlan,
    pub(crate) plan_source: &'static str,
    pub(crate) budget: ResearchBudget,
    pub(crate) evidence_dir: PathBuf,
    pub(crate) fallback: String,
}

struct Candidate {
    source: Source,
    subquestion_id: String,
    prefetched_provider: Option<&'static str>,
    known_url: bool,
}

#[derive(Default)]
struct DiscoveryBlock {
    attempts: Vec<ProviderAttempt>,
    capability_gaps: Vec<CapabilityGap>,
    diagnostics: Vec<String>,
    candidates: Vec<Candidate>,
}

struct FetchedCandidate {
    url: String,
    title: String,
    provider: &'static str,
    source_type: &'static str,
    subquestion_id: String,
    content: String,
}

#[derive(Default)]
struct CandidateFetchBlock {
    attempts: Vec<ProviderAttempt>,
    diagnostic: Option<String>,
    evidence: Option<FetchedCandidate>,
    gap: Option<ResearchGap>,
    missing_provider: bool,
}

// Research orchestration stays together to preserve deterministic phase and evidence ordering.
#[expect(clippy::too_many_lines)]
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
    let limit = u16::try_from(request.budget.discovery_limit()).unwrap_or(u16::MAX);

    if let Err(error) =
        write_json_artifact(&request.evidence_dir, "00-plan.json", &json!(request.plan))
    {
        return Err(runtime_error(
            format!("cannot write research plan artifact: {error}"),
            attempts,
            capability_gaps,
        ));
    }

    let discovery_blocks = stream::iter(request.plan.decomposition.iter().flat_map(
        |subquestion| {
            subquestion
                .required_capabilities
                .iter()
                .copied()
                .map(move |capability| (subquestion, capability))
        },
    ))
    .map(|(subquestion, capability)| {
        let execution =
            CapabilityExecution::new(&request.fallback, client.clone(), retry_policy, deadline);
        let config = &config;
        async move { discover_capability(subquestion, capability, limit, config, execution).await }
    })
    .buffered(engine::FANOUT_CONCURRENCY)
    .collect::<Vec<_>>()
    .await;
    for mut block in discovery_blocks {
        attempts.append(&mut block.attempts);
        capability_gaps.append(&mut block.capability_gaps);
        diagnostics.append(&mut block.diagnostics);
        candidates.append(&mut block.candidates);
    }
    candidates = bound_discovery_candidates(
        candidates,
        &request.plan.decomposition,
        request.budget.discovery_limit(),
    );

    let known_url_candidates = known_url_candidates(&request.query, &request.plan.decomposition);
    let subquestion_ids = request
        .plan
        .decomposition
        .iter()
        .map(|subquestion| subquestion.id.clone())
        .collect::<Vec<_>>();
    let candidates = interleave_candidates(candidates, &subquestion_ids);

    let mut evidence_items = Vec::new();
    let mut evidence_counts = request
        .plan
        .decomposition
        .iter()
        .map(|subquestion| (subquestion.id.clone(), 0_usize))
        .collect::<HashMap<_, _>>();
    let mut candidate_attempt_counts = evidence_counts.clone();
    let mut seen_urls = HashSet::new();
    let mut fetch_config = config.web_fetch.clone();
    if request.fallback == "off" {
        fetch_config.retain_first();
    }
    let mut known_url_candidates = VecDeque::from(known_url_candidates);
    while !known_url_candidates.is_empty() {
        let wave_len = known_url_candidates.len().min(engine::FANOUT_CONCURRENCY);
        let wave = known_url_candidates.drain(..wave_len).collect::<Vec<_>>();
        seen_urls.extend(wave.iter().map(|candidate| candidate.source.url.clone()));
        let results = join_all(wave.into_iter().map(|candidate| {
            fetch_candidate(
                candidate,
                fetch_config.clone(),
                client.clone(),
                retry_policy,
                deadline,
            )
        }))
        .await;
        for mut block in results {
            attempts.append(&mut block.attempts);
            push_diagnostic(&mut diagnostics, block.diagnostic);
            if block.missing_provider
                && !capability_gaps
                    .iter()
                    .any(|gap| gap.capability == Capability::WebFetch)
            {
                capability_gaps.push(CapabilityGap {
                    capability: Capability::WebFetch,
                    reason: "no_configured_provider",
                    providers_skipped: fetch_config.names(),
                });
            }
            if let Some(gap) = block.gap {
                research_gaps.push(gap);
            }
            if let Some(evidence) = block.evidence {
                push_evidence(
                    &mut evidence_items,
                    evidence.url,
                    evidence.title,
                    evidence.provider,
                    evidence.source_type,
                    evidence.subquestion_id,
                    evidence.content,
                );
            }
        }
    }
    let mut candidates = VecDeque::from(candidates);
    loop {
        let mut wave = Vec::with_capacity(engine::FANOUT_CONCURRENCY);
        let mut reserved = HashMap::<String, usize>::new();
        let candidates_to_consider = candidates.len();
        for _ in 0..candidates_to_consider {
            if wave.len() >= engine::FANOUT_CONCURRENCY {
                break;
            }
            let Some(candidate) = candidates.pop_front() else {
                break;
            };
            let accepted = evidence_counts
                .get(&candidate.subquestion_id)
                .copied()
                .unwrap_or_default();
            let reserved_for_subquestion = reserved
                .get(&candidate.subquestion_id)
                .copied()
                .unwrap_or_default();
            if accepted + reserved_for_subquestion >= request.budget.evidence_cap() {
                candidates.push_back(candidate);
                continue;
            }
            if !seen_urls.insert(candidate.source.url.clone()) {
                continue;
            }
            *reserved
                .entry(candidate.subquestion_id.clone())
                .or_default() += 1;
            *candidate_attempt_counts
                .entry(candidate.subquestion_id.clone())
                .or_default() += 1;
            wave.push(candidate);
        }
        if wave.is_empty() {
            break;
        }
        let results = join_all(wave.into_iter().map(|candidate| {
            fetch_candidate(
                candidate,
                fetch_config.clone(),
                client.clone(),
                retry_policy,
                deadline,
            )
        }))
        .await;
        for mut block in results {
            attempts.append(&mut block.attempts);
            push_diagnostic(&mut diagnostics, block.diagnostic);
            if block.missing_provider
                && !capability_gaps
                    .iter()
                    .any(|gap| gap.capability == Capability::WebFetch)
            {
                capability_gaps.push(CapabilityGap {
                    capability: Capability::WebFetch,
                    reason: "no_configured_provider",
                    providers_skipped: fetch_config.names(),
                });
            }
            if let Some(gap) = block.gap {
                research_gaps.push(gap);
            }
            if let Some(evidence) = block.evidence {
                *evidence_counts
                    .entry(evidence.subquestion_id.clone())
                    .or_default() += 1;
                push_evidence(
                    &mut evidence_items,
                    evidence.url,
                    evidence.title,
                    evidence.provider,
                    evidence.source_type,
                    evidence.subquestion_id,
                    evidence.content,
                );
            }
        }
    }
    for subquestion in &request.plan.decomposition {
        let has_evidence = evidence_items
            .iter()
            .any(|item| item.subquestion_id == subquestion.id);
        let has_gap = research_gaps
            .iter()
            .any(|gap| gap.subquestion_id == subquestion.id);
        if !has_evidence && !has_gap {
            let attempted = candidate_attempt_counts
                .get(&subquestion.id)
                .copied()
                .unwrap_or_default();
            research_gaps.push(ResearchGap {
                subquestion_id: subquestion.id.clone(),
                reason: format!(
                    "no verified evidence was collected after attempting {attempted} candidate URLs"
                ),
                url: None,
            });
        }
    }

    let requested_evidence = if request.plan.intent_signals.cross_validation_need
        == EvidenceStrength::High
        || request.plan.intent_signals.source_authority_need == EvidenceStrength::High
    {
        2
    } else {
        1
    };
    let plan_capacity = request
        .plan
        .decomposition
        .len()
        .saturating_mul(request.budget.evidence_cap());
    let required_evidence = requested_evidence.min(plan_capacity);
    if required_evidence < requested_evidence {
        diagnostics.push(format!(
            "required evidence floor clamped from {requested_evidence} to plan capacity {required_evidence}"
        ));
    }
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
        let mut error = ResearchError {
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
        )
        .inspect_err(|summary_error| {
            let summary_diagnostic =
                format!("cannot write research summary artifact: {summary_error}");
            error.diagnostic = Some(match error.diagnostic.take() {
                Some(diagnostic) => format!("{diagnostic}\n{summary_diagnostic}"),
                None => summary_diagnostic,
            });
        });
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
        plan_source: request.plan_source,
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

fn bound_discovery_candidates(
    candidates: Vec<Candidate>,
    subquestions: &[ResearchSubquestion],
    limit: usize,
) -> Vec<Candidate> {
    let mut counts = subquestions
        .iter()
        .map(|subquestion| (subquestion.id.as_str(), 0_usize))
        .collect::<HashMap<_, _>>();
    let mut urls = subquestions
        .iter()
        .map(|subquestion| (subquestion.id.as_str(), HashSet::<String>::new()))
        .collect::<HashMap<_, _>>();

    candidates
        .into_iter()
        .filter(|candidate| {
            let Some(count) = counts.get_mut(candidate.subquestion_id.as_str()) else {
                return false;
            };
            let Some(seen) = urls.get_mut(candidate.subquestion_id.as_str()) else {
                return false;
            };
            if *count >= limit || !seen.insert(candidate.source.url.clone()) {
                return false;
            }
            *count += 1;
            true
        })
        .collect()
}

async fn discover_capability(
    subquestion: &ResearchSubquestion,
    capability: PlanCapability,
    limit: u16,
    config: &RuntimeConfig,
    execution: CapabilityExecution,
) -> DiscoveryBlock {
    match capability {
        PlanCapability::DocsSearch => discover_docs(subquestion, limit, config, execution).await,
        PlanCapability::WebSearch => discover_web(subquestion, limit, config, execution).await,
        PlanCapability::VerticalSearch => {
            discover_vertical(subquestion, limit, config, execution).await
        }
    }
}

async fn discover_docs(
    subquestion: &ResearchSubquestion,
    limit: u16,
    config: &RuntimeConfig,
    execution: CapabilityExecution,
) -> DiscoveryBlock {
    let mut block = DiscoveryBlock::default();
    if config.docs_search.configured_provider_count() == 0 {
        block.capability_gaps.push(CapabilityGap {
            capability: Capability::DocsSearch,
            reason: "no_configured_provider",
            providers_skipped: config.docs_search.names(),
        });
        return block;
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
            let prefetched_provider = outcome
                .attempts
                .iter()
                .rev()
                .find(|attempt| attempt.seam == "docs_search" && attempt.error_kind.is_none())
                .map(|attempt| attempt.provider);
            block.attempts.append(&mut outcome.attempts);
            push_diagnostic(&mut block.diagnostics, outcome.diagnostic);
            block
                .candidates
                .extend(outcome.read_sources.into_iter().map(|source| Candidate {
                    source,
                    subquestion_id: subquestion.id.clone(),
                    prefetched_provider,
                    known_url: false,
                }));
            block
                .candidates
                .extend(
                    outcome
                        .candidate_sources
                        .into_iter()
                        .map(|source| Candidate {
                            source,
                            subquestion_id: subquestion.id.clone(),
                            prefetched_provider: None,
                            known_url: false,
                        }),
                );
        }
        Err(mut error) => {
            block.attempts.append(&mut error.attempts);
            push_diagnostic(&mut block.diagnostics, error.diagnostic);
            block.capability_gaps.push(CapabilityGap {
                capability: Capability::DocsSearch,
                reason: "all_attempts_failed",
                providers_skipped: unconfigured_docs_providers(config),
            });
        }
    }
    block
}

async fn discover_web(
    subquestion: &ResearchSubquestion,
    limit: u16,
    config: &RuntimeConfig,
    execution: CapabilityExecution,
) -> DiscoveryBlock {
    let mut block = DiscoveryBlock::default();
    if config.web_search.configured_provider_count() == 0 {
        block.capability_gaps.push(CapabilityGap {
            capability: Capability::WebSearch,
            reason: "no_configured_provider",
            providers_skipped: config.web_search.names(),
        });
        return block;
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
            block.attempts.append(&mut outcome.attempts);
            push_diagnostic(&mut block.diagnostics, outcome.diagnostic);
            block
                .candidates
                .extend(outcome.sources.into_iter().map(|source| Candidate {
                    source,
                    subquestion_id: subquestion.id.clone(),
                    prefetched_provider: None,
                    known_url: false,
                }));
        }
        Err(mut error) => {
            block.attempts.append(&mut error.attempts);
            push_diagnostic(&mut block.diagnostics, error.diagnostic);
            block.capability_gaps.push(CapabilityGap {
                capability: Capability::WebSearch,
                reason: "all_attempts_failed",
                providers_skipped: unconfigured_web_providers(config),
            });
        }
    }
    block
}

async fn discover_vertical(
    subquestion: &ResearchSubquestion,
    limit: u16,
    config: &RuntimeConfig,
    execution: CapabilityExecution,
) -> DiscoveryBlock {
    let mut block = DiscoveryBlock::default();
    if config.vertical_search.configured_provider_count() == 0 {
        block.capability_gaps.push(CapabilityGap {
            capability: Capability::VerticalSearch,
            reason: "no_configured_provider",
            providers_skipped: config.vertical_search.names(),
        });
        return block;
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
            block.attempts.append(&mut outcome.attempts);
            push_diagnostic(&mut block.diagnostics, outcome.diagnostic);
            block
                .candidates
                .extend(outcome.sources.into_iter().map(|source| Candidate {
                    source,
                    subquestion_id: subquestion.id.clone(),
                    prefetched_provider: None,
                    known_url: false,
                }));
        }
        Err(mut error) => {
            block.attempts.append(&mut error.attempts);
            push_diagnostic(&mut block.diagnostics, error.diagnostic);
            block.capability_gaps.push(CapabilityGap {
                capability: Capability::VerticalSearch,
                reason: "all_attempts_failed",
                providers_skipped: unconfigured_vertical_providers(config),
            });
        }
    }
    block
}

async fn fetch_candidate(
    candidate: Candidate,
    fetch_config: crate::config::WebFetchRuntimeConfig,
    client: reqwest::Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> CandidateFetchBlock {
    if let Some(provider) = candidate.prefetched_provider
        && candidate
            .source
            .text
            .as_deref()
            .is_some_and(|content| !content.trim().is_empty())
    {
        return CandidateFetchBlock {
            evidence: Some(FetchedCandidate {
                url: candidate.source.url,
                title: candidate.source.title,
                provider,
                source_type: "docs",
                subquestion_id: candidate.subquestion_id,
                content: candidate.source.text.unwrap_or_default(),
            }),
            ..CandidateFetchBlock::default()
        };
    }
    if fetch_config.configured_provider_count() == 0 {
        return CandidateFetchBlock {
            gap: candidate.known_url.then(|| ResearchGap {
                subquestion_id: candidate.subquestion_id,
                reason:
                    "known URL could not be fetched because web_fetch has no configured provider"
                        .into(),
                url: Some(redact_url(&candidate.source.url)),
            }),
            missing_provider: true,
            ..CandidateFetchBlock::default()
        };
    }
    let url = candidate.source.url.clone();
    match engine::fetch(
        FetchRequest {
            url: url.clone(),
            verbose: true,
        },
        fetch_config,
        client,
        retry_policy,
        deadline,
    )
    .await
    {
        Ok(fetched) => CandidateFetchBlock {
            attempts: fetched.attempts,
            diagnostic: fetched.diagnostic,
            evidence: Some(FetchedCandidate {
                url: fetched.url,
                title: candidate.source.title,
                provider: fetched.provider,
                source_type: "fetched_page",
                subquestion_id: candidate.subquestion_id,
                content: fetched.content,
            }),
            gap: None,
            missing_provider: false,
        },
        Err(error) => CandidateFetchBlock {
            attempts: error.attempts,
            diagnostic: error.diagnostic,
            evidence: None,
            gap: candidate.known_url.then(|| ResearchGap {
                subquestion_id: candidate.subquestion_id,
                reason: "failed to fetch known URL".into(),
                url: Some(redact_url(&url)),
            }),
            missing_provider: false,
        },
    }
}

fn known_url_candidates(query: &str, subquestions: &[ResearchSubquestion]) -> Vec<Candidate> {
    let mut seen = HashSet::new();
    let mut candidates = Vec::new();
    for subquestion in subquestions {
        for url in engine::known_urls(&subquestion.question) {
            if seen.insert(url.clone()) {
                candidates.push(known_url_candidate(url, subquestion.id.clone()));
            }
        }
    }
    for url in engine::known_urls(query) {
        if seen.insert(url.clone()) {
            candidates.push(known_url_candidate(url, String::new()));
        }
    }
    candidates
}

fn known_url_candidate(url: String, subquestion_id: String) -> Candidate {
    Candidate {
        source: Source {
            title: redact_url(&url),
            url,
            published_date: None,
            author: None,
            text: None,
            highlights: Vec::new(),
        },
        subquestion_id,
        prefetched_provider: None,
        known_url: true,
    }
}

fn interleave_candidates(candidates: Vec<Candidate>, subquestion_ids: &[String]) -> Vec<Candidate> {
    let mut by_subquestion = subquestion_ids
        .iter()
        .cloned()
        .map(|id| (id, VecDeque::new()))
        .collect::<HashMap<_, _>>();
    let mut unmatched = VecDeque::new();
    for candidate in candidates {
        if let Some(group) = by_subquestion.get_mut(&candidate.subquestion_id) {
            group.push_back(candidate);
        } else {
            unmatched.push_back(candidate);
        }
    }

    let mut interleaved = Vec::new();
    loop {
        let mut added = false;
        for id in subquestion_ids {
            if let Some(candidate) = by_subquestion.get_mut(id).and_then(VecDeque::pop_front) {
                interleaved.push(candidate);
                added = true;
            }
        }
        if let Some(candidate) = unmatched.pop_front() {
            interleaved.push(candidate);
            added = true;
        }
        if !added {
            return interleaved;
        }
    }
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
        let _ = write!(
            report,
            "\n{}. {} ({})\n   Evidence excerpt: {}\n   Source: {}",
            index + 1,
            item.title,
            item.provider,
            excerpt,
            item.url
        );
    }
    if !gaps.is_empty() {
        report.push_str("\n\nUnverified gaps:");
        for gap in gaps {
            let _ = write!(report, "\n- {}: {}", gap.subquestion_id, gap.reason);
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
    config.docs_search.unconfigured_names()
}

fn unconfigured_web_providers(config: &RuntimeConfig) -> Vec<String> {
    config.web_search.unconfigured_names()
}

fn unconfigured_vertical_providers(config: &RuntimeConfig) -> Vec<String> {
    config.vertical_search.unconfigured_names()
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
