use std::collections::{HashMap, HashSet, VecDeque};
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
    AttemptErrorKind, Capability, CapabilityGap, Deadline, EvidenceItem, EvidenceLocator,
    EvidenceStrength, PlanCapability, ProviderAttempt, ResearchError, ResearchGap,
    ResearchGapCheck, ResearchOutcome, ResearchPlan, ResearchSubquestion, Source,
    UnconsumedCandidates,
};

const SYNTHESIS_POLICY: &str = "fetch_before_claim";

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
    locator: EvidenceLocator,
    source: Source,
    query: String,
    subquestion_id: String,
    provider: Option<&'static str>,
    source_type: &'static str,
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
    locator: EvidenceLocator,
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
            &request.evidence_dir,
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
    let mut seen_locators = HashSet::new();
    let mut fetch_config = config.web_fetch.clone();
    let docs_config = config.docs_search.clone();
    if request.fallback == "off" {
        fetch_config.retain_first();
    }
    let mut known_url_candidates = VecDeque::from(known_url_candidates);
    while !known_url_candidates.is_empty() {
        let wave_len = known_url_candidates.len().min(engine::FANOUT_CONCURRENCY);
        let wave = known_url_candidates.drain(..wave_len).collect::<Vec<_>>();
        seen_locators.extend(wave.iter().map(|candidate| candidate.locator.clone()));
        let results = join_all(wave.into_iter().map(|candidate| {
            fetch_candidate(
                candidate,
                docs_config.clone(),
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
                push_evidence(&mut evidence_items, &request.evidence_dir, evidence);
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
            if !seen_locators.insert(candidate.locator.clone()) {
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
                docs_config.clone(),
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
                push_evidence(&mut evidence_items, &request.evidence_dir, evidence);
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
    let evidence_dir = request.evidence_dir.display().to_string();
    let plan_path = request
        .evidence_dir
        .join("00-plan.json")
        .display()
        .to_string();
    let candidates_path = request
        .evidence_dir
        .join("candidates.json")
        .display()
        .to_string();
    let candidate_artifact = unconsumed_candidates_artifact(&candidates);
    let unconsumed_candidates = UnconsumedCandidates {
        count: candidates.len(),
        path: candidates_path,
    };
    write_evidence_artifacts(&evidence_items).map_err(|error| {
        runtime_error(
            format!("cannot write research evidence artifact: {error}"),
            attempts.clone(),
            capability_gaps.clone(),
            &request.evidence_dir,
        )
    })?;
    write_json_artifact(
        &request.evidence_dir,
        "candidates.json",
        &candidate_artifact,
    )
    .map_err(|error| {
        runtime_error(
            format!("cannot write research candidate artifact: {error}"),
            attempts.clone(),
            capability_gaps.clone(),
            &request.evidence_dir,
        )
    })?;
    let fallback_used = fallback_used(&attempts);
    if evidence_is_insufficient {
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
            gap_check,
            evidence_dir,
            plan_path,
            unconsumed_candidates,
            synthesis_policy: SYNTHESIS_POLICY,
            diagnostic,
        };
        let _ = write_json_artifact(
            &request.evidence_dir,
            "summary.json",
            &json!({
                "status": "error",
                "error_kind": error.kind.as_str(),
                "message": error.message,
                "query": request.query,
                "budget": request.budget.as_str(),
                "plan_source": request.plan_source,
                "capabilities": capabilities,
                "fallback_used": fallback_used,
                "coverage": {
                    "evidence_count": error.evidence_items.len(),
                    "unconsumed_candidate_count": error.unconsumed_candidates.count,
                    "gap_check": error.gap_check,
                },
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

    let outcome = ResearchOutcome {
        evidence_items,
        capability_gaps,
        gap_check,
        evidence_dir,
        plan_path,
        unconsumed_candidates,
        synthesis_policy: SYNTHESIS_POLICY,
        attempts,
        diagnostic,
    };
    let summary = json!({
        "status": "ok",
        "query": request.query,
        "budget": request.budget.as_str(),
        "plan_source": request.plan_source,
        "capabilities": capabilities,
        "fallback_used": fallback_used,
        "coverage": {
            "evidence_count": outcome.evidence_items.len(),
            "unconsumed_candidate_count": outcome.unconsumed_candidates.count,
            "gap_check": outcome.gap_check,
        },
        "evidence_items": outcome.evidence_items,
        "provider_attempts": outcome.attempts,
        "capability_gaps": outcome.capability_gaps,
    });
    if let Err(error) = write_json_artifact(&request.evidence_dir, "summary.json", &summary) {
        return Err(ResearchError {
            kind: AttemptErrorKind::Runtime,
            message: format!("cannot write research summary artifact: {error}"),
            attempts: outcome.attempts,
            evidence_items: outcome.evidence_items,
            capability_gaps: outcome.capability_gaps,
            gap_check: outcome.gap_check,
            evidence_dir: outcome.evidence_dir,
            plan_path: outcome.plan_path,
            unconsumed_candidates: outcome.unconsumed_candidates,
            synthesis_policy: outcome.synthesis_policy,
            diagnostic: outcome.diagnostic,
        });
    }
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
    let mut locators = subquestions
        .iter()
        .map(|subquestion| (subquestion.id.as_str(), HashSet::<EvidenceLocator>::new()))
        .collect::<HashMap<_, _>>();

    candidates
        .into_iter()
        .filter(|candidate| {
            let Some(count) = counts.get_mut(candidate.subquestion_id.as_str()) else {
                return false;
            };
            let Some(seen) = locators.get_mut(candidate.subquestion_id.as_str()) else {
                return false;
            };
            if *count >= limit || !seen.insert(candidate.locator.clone()) {
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
            block.attempts.append(&mut outcome.attempts);
            push_diagnostic(&mut block.diagnostics, outcome.diagnostic);
            block
                .candidates
                .extend(
                    outcome
                        .candidate_sources
                        .into_iter()
                        .filter_map(|candidate| {
                            let provider = candidate.provider();
                            let (locator, source) = candidate.into_evidence_source()?;
                            Some(Candidate {
                                locator,
                                source,
                                query: subquestion.question.clone(),
                                subquestion_id: subquestion.id.clone(),
                                provider: Some(provider),
                                source_type: "docs_candidate",
                                known_url: false,
                            })
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
            let provider = successful_provider(&outcome.attempts, "web_search");
            block.attempts.append(&mut outcome.attempts);
            push_diagnostic(&mut block.diagnostics, outcome.diagnostic);
            block
                .candidates
                .extend(outcome.sources.into_iter().map(|source| Candidate {
                    locator: EvidenceLocator::Url(source.url.clone()),
                    source,
                    query: subquestion.question.clone(),
                    subquestion_id: subquestion.id.clone(),
                    provider,
                    source_type: "web_candidate",
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
            let provider = successful_provider(&outcome.attempts, "vertical_search");
            block.attempts.append(&mut outcome.attempts);
            push_diagnostic(&mut block.diagnostics, outcome.diagnostic);
            block
                .candidates
                .extend(outcome.sources.into_iter().map(|source| Candidate {
                    locator: EvidenceLocator::Url(source.url.clone()),
                    source,
                    query: subquestion.question.clone(),
                    subquestion_id: subquestion.id.clone(),
                    provider,
                    source_type: "vertical_candidate",
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
    docs_config: crate::config::DocsSearchRuntimeConfig,
    fetch_config: crate::config::WebFetchRuntimeConfig,
    client: reqwest::Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> CandidateFetchBlock {
    if matches!(&candidate.locator, EvidenceLocator::Context7Library(_)) {
        let execution = CapabilityExecution::new("auto", client, retry_policy, deadline);
        return match engine::documentation_read(
            &candidate.locator,
            &candidate.query,
            docs_config,
            execution,
        )
        .await
        {
            Ok(evidence) => CandidateFetchBlock {
                attempts: evidence.attempts,
                diagnostic: evidence.diagnostic,
                evidence: Some(FetchedCandidate {
                    locator: evidence.locator,
                    title: candidate.source.title,
                    provider: evidence.provider,
                    source_type: "docs",
                    subquestion_id: candidate.subquestion_id,
                    content: evidence.content,
                }),
                gap: None,
                missing_provider: false,
            },
            Err(error) => CandidateFetchBlock {
                attempts: error.attempts,
                diagnostic: error.diagnostic,
                evidence: None,
                gap: Some(ResearchGap {
                    subquestion_id: candidate.subquestion_id,
                    reason: "failed to read Context7 library".into(),
                    url: None,
                }),
                missing_provider: false,
            },
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
                locator: EvidenceLocator::Url(fetched.url),
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
        locator: EvidenceLocator::Url(url.clone()),
        source: Source {
            title: redact_url(&url),
            url,
            published_date: None,
            author: None,
            text: None,
            highlights: Vec::new(),
            id: None,
            image: None,
            favicon: None,
        },
        query: String::new(),
        subquestion_id,
        provider: None,
        source_type: "known_url",
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
    evidence_dir: &Path,
    evidence: FetchedCandidate,
) {
    let content_len = evidence.content.chars().count();
    let id = format!("e{}", evidence_items.len() + 1);
    let path = evidence_dir
        .join(format!("{:02}-evidence.md", evidence_items.len() + 1))
        .display()
        .to_string();
    evidence_items.push(EvidenceItem {
        id,
        locator: evidence.locator,
        title: (!evidence.title.is_empty()).then_some(evidence.title),
        provider: evidence.provider,
        source_type: evidence.source_type,
        subquestion_id: evidence.subquestion_id,
        content: evidence.content,
        content_len,
        verified: content_len > 0,
        path,
    });
}

fn unconsumed_candidates_artifact(candidates: &VecDeque<Candidate>) -> Value {
    json!({
        "is_evidence": false,
        "candidates": candidates.iter().map(|candidate| {
            let mut source = json!(candidate.source);
            source["url"] = json!(candidate.locator.url());
            if let Some(library_id) = candidate.locator.library_id() {
                source["library_id"] = json!(library_id);
            }
            json!({
                "source": source,
                "provider": candidate.provider,
                "source_type": candidate.source_type,
                "subquestion_id": candidate.subquestion_id,
            })
        }).collect::<Vec<_>>()
    })
}

fn write_evidence_artifacts(items: &[EvidenceItem]) -> std::io::Result<()> {
    for item in items {
        fs::write(&item.path, &item.content)?;
    }
    Ok(())
}

fn successful_provider(attempts: &[ProviderAttempt], seam: &str) -> Option<&'static str> {
    attempts
        .iter()
        .rev()
        .find(|attempt| attempt.seam == seam && attempt.error_kind.is_none())
        .map(|attempt| attempt.provider)
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
    evidence_dir: &Path,
) -> ResearchError {
    let evidence_dir_display = evidence_dir.display().to_string();
    ResearchError {
        kind: AttemptErrorKind::Runtime,
        message,
        attempts,
        evidence_items: Vec::new(),
        capability_gaps,
        gap_check: ResearchGapCheck {
            status: "degraded",
            gaps: Vec::new(),
            stop_reason: "artifact_write_failed",
        },
        evidence_dir: evidence_dir_display,
        plan_path: evidence_dir.join("00-plan.json").display().to_string(),
        unconsumed_candidates: UnconsumedCandidates {
            count: 0,
            path: evidence_dir.join("candidates.json").display().to_string(),
        },
        synthesis_policy: SYNTHESIS_POLICY,
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
