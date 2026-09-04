use std::collections::HashSet;
use std::sync::Arc;

use futures_util::{StreamExt, stream};
use reqwest::Client;

use crate::attempt_trace;
use crate::chain::{
    self, BudgetPolicy, ChainSettings, ChainStep, StepIdentity, StepRejection, StepSuccess,
    StepVerdict,
};
use crate::config::{
    DocsSearchRuntimeConfig, MainSearchRuntimeConfig, RuntimeConfig, SeamEntry,
    VerticalSearchRuntimeConfig, WebFetchRuntimeConfig, WebSearchRuntimeConfig,
};
use crate::net::{RetryPolicy, combine_diagnostics};
use crate::providers::{self, FetchRequest, MainSearchRequest, ProviderError};
use crate::redact::redact_url;
use crate::types::{
    AttemptErrorKind, Capability, CapabilityGap, CapabilitySet, DENSITY_MAX_CHARS,
    DENSITY_MAX_UNIQUE_LINES, Deadline, DocumentationEvidence, DocumentationSearchOutcome,
    EvidenceLocator, FallbackPolicy, FetchOutcome, MIN_FETCH_CONTENT_CHARS, ProviderAttempt,
    SearchCandidate, SearchOutcome, Source, SupplementalSearchOutcome, VerticalSearchOutcome,
};

pub(crate) const FANOUT_CONCURRENCY: usize = 4;
const WEB_FETCH_PREVIEW_CHARS: usize = 300;

#[derive(Clone)]
pub(crate) struct CapabilityExecution {
    pub(crate) fallback: FallbackPolicy,
    pub(crate) client: Client,
    pub(crate) retry_policy: RetryPolicy,
    pub(crate) deadline: Deadline,
}

#[derive(Default)]
struct CapabilityBranch {
    attempts: Vec<ProviderAttempt>,
    sources: Vec<SearchCandidate>,
    capability_gaps: Vec<CapabilityGap>,
    diagnostics: Vec<String>,
}

#[derive(Clone, Copy)]
struct CapabilityTargets {
    web: u16,
    documentation: u16,
    vertical: u16,
}

impl CapabilityTargets {
    fn new(requested: u16) -> Self {
        Self {
            web: if requested == 0 { 3 } else { requested },
            documentation: requested.max(1),
            vertical: requested.max(1),
        }
    }
}

impl CapabilityBranch {
    fn record_gap(
        &mut self,
        capability: Capability,
        reason: &'static str,
        providers_skipped: Vec<String>,
        message: &str,
    ) {
        self.capability_gaps.push(CapabilityGap {
            capability,
            reason,
            providers_skipped,
        });
        self.diagnostics.push(format!("capability gap: {message}"));
    }

    fn push_diagnostic(&mut self, diagnostic: Option<String>) {
        if let Some(diagnostic) = diagnostic {
            self.diagnostics.push(diagnostic);
        }
    }
}

fn provider_steps<C>(entries: Vec<SeamEntry<C>>) -> Vec<ChainStep<(providers::ProviderId, C)>> {
    entries
        .into_iter()
        .map(|entry| {
            let (id, config, configured) = entry.into_parts();
            ChainStep {
                context: (id, config),
                configured,
                gate_attempt: None,
            }
        })
        .collect()
}

fn provider_identity<C>(context: &(providers::ProviderId, C)) -> StepIdentity {
    StepIdentity {
        provider: context.0.name(),
        model: None,
        endpoint_host: None,
    }
}

fn provider_chain_settings<C>(
    seam: &'static str,
    budget_policy: BudgetPolicy,
    fallback_off: bool,
    verbose: bool,
    exhausted_message: &'static str,
) -> ChainSettings<'static, (providers::ProviderId, C)> {
    ChainSettings {
        seam,
        budget_policy,
        fallback_off,
        diagnostic_merge: chain::DiagnosticMerge::Join,
        terminal: chain::TerminalPolicy::ChainWide {
            verbose,
            exhausted_message,
        },
        identity: &provider_identity::<C>,
        continue_on_failure: &chain::always_continue,
    }
}

fn web_fetch_chain_settings<C>(
    verbose: bool,
) -> ChainSettings<'static, (providers::ProviderId, C)> {
    provider_chain_settings(
        "web_fetch",
        BudgetPolicy::SlicedEven {
            skipped_message: "skipped to preserve fallback deadline budget",
        },
        false,
        verbose,
        "web fetch deadline elapsed",
    )
}

impl CapabilityExecution {
    pub(crate) fn new(
        fallback: FallbackPolicy,
        client: Client,
        retry_policy: RetryPolicy,
        deadline: Deadline,
    ) -> Self {
        Self {
            fallback,
            client,
            retry_policy,
            deadline,
        }
    }
}

pub(crate) async fn execute_capabilities(
    outcome: &mut SearchOutcome,
    query: &str,
    capabilities: &CapabilitySet,
    requested_target: u16,
    config: &RuntimeConfig,
    execution: CapabilityExecution,
) {
    let targets = CapabilityTargets::new(requested_target);
    let branches = stream::iter(capabilities.iter())
        .map(|capability| {
            let execution = execution.clone();
            async move {
                match capability {
                    Capability::DocsSearch => {
                        execute_docs_search(query, targets.documentation, config, execution).await
                    }
                    Capability::WebSearch => {
                        execute_web_search(query, targets.web, config, execution).await
                    }
                    Capability::WebFetch => execute_web_fetch(query, config, execution).await,
                    Capability::VerticalSearch => {
                        execute_vertical_search(query, targets.vertical, config, execution).await
                    }
                }
            }
        })
        .buffered(FANOUT_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;

    for mut branch in branches {
        outcome.attempts.append(&mut branch.attempts);
        outcome.capability_gaps.append(&mut branch.capability_gaps);
        for diagnostic in branch.diagnostics {
            outcome.diagnostic = combine_diagnostics(
                [outcome.diagnostic.take(), Some(diagnostic)]
                    .into_iter()
                    .flatten(),
            );
        }
        merge_extra_sources(outcome, branch.sources);
    }
}

async fn execute_web_search(
    query: &str,
    limit: u16,
    config: &RuntimeConfig,
    execution: CapabilityExecution,
) -> CapabilityBranch {
    let mut branch = CapabilityBranch::default();
    if config.web_search.configured_provider_count() == 0 {
        branch.record_gap(
            Capability::WebSearch,
            "no_configured_provider",
            config.web_search.names(),
            "web_search has no configured provider",
        );
        return branch;
    }
    match supplemental_web_search(query, limit, config.web_search.clone(), execution).await {
        Ok(mut supplemental) => {
            let Some(provider) =
                attempt_trace::successful_provider(&supplemental.attempts, "web_search")
            else {
                unreachable!("successful search outcome records a provider attempt")
            };
            branch.attempts.append(&mut supplemental.attempts);
            branch.push_diagnostic(supplemental.diagnostic);
            branch.sources = supplemental_candidates(supplemental.sources, provider);
        }
        Err(mut error) => {
            let attempted = error
                .attempts
                .iter()
                .map(|attempt| attempt.provider)
                .collect::<HashSet<_>>();
            branch.attempts.append(&mut error.attempts);
            branch.push_diagnostic(error.diagnostic);
            branch.record_gap(
                Capability::WebSearch,
                "all_attempts_failed",
                config
                    .web_search
                    .entries()
                    .iter()
                    .filter(|entry| !attempted.contains(entry.name()))
                    .map(|entry| entry.name().to_owned())
                    .collect(),
                "all web_search attempts failed",
            );
        }
    }
    branch
}

async fn execute_docs_search(
    query: &str,
    limit: u16,
    config: &RuntimeConfig,
    execution: CapabilityExecution,
) -> CapabilityBranch {
    let mut branch = CapabilityBranch::default();
    if config.docs_search.configured_provider_count() == 0 {
        branch.record_gap(
            Capability::DocsSearch,
            "no_configured_provider",
            config.docs_search.names(),
            "docs_search has no configured provider",
        );
        return branch;
    }
    match documentation_search(query, limit, config.docs_search.clone(), execution).await {
        Ok(mut supplemental) => {
            branch.attempts.append(&mut supplemental.attempts);
            branch.push_diagnostic(supplemental.diagnostic);
            branch.sources = supplemental.candidate_sources;
        }
        Err(mut error) => {
            branch.attempts.append(&mut error.attempts);
            branch.push_diagnostic(error.diagnostic);
            branch.record_gap(
                Capability::DocsSearch,
                "all_attempts_failed",
                config.docs_search.unconfigured_names(),
                "all docs_search attempts failed",
            );
        }
    }
    branch
}

async fn execute_web_fetch(
    query: &str,
    config: &RuntimeConfig,
    execution: CapabilityExecution,
) -> CapabilityBranch {
    let mut branch = CapabilityBranch::default();
    if config.web_fetch.configured_provider_count() == 0 {
        branch.record_gap(
            Capability::WebFetch,
            "no_configured_provider",
            config.web_fetch.names(),
            "web_fetch has no configured provider",
        );
        return branch;
    }
    let urls = known_urls(query);
    if urls.is_empty() {
        branch.record_gap(
            Capability::WebFetch,
            "all_attempts_failed",
            Vec::new(),
            "web_fetch declaration has no known URL target",
        );
        return branch;
    }
    let mut succeeded = false;
    let mut failed = false;
    let results = stream::iter(urls)
        .map(|url| {
            let fetch_config = config.web_fetch.clone();
            let execution = execution.clone();
            async move {
                let result = fetch(
                    FetchRequest {
                        url: url.clone(),
                        verbose: true,
                    },
                    fetch_config,
                    execution.client,
                    execution.retry_policy,
                    execution.deadline,
                )
                .await;
                (url, result)
            }
        })
        .buffered(FANOUT_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
    for (url, result) in results {
        match result {
            Ok(mut fetched) => {
                succeeded = true;
                branch.attempts.append(&mut fetched.attempts);
                branch.push_diagnostic(fetched.diagnostic);
                if let Some(candidate) = SearchCandidate::from_web_fetch(
                    fetched.provider,
                    redact_url(&url),
                    fetched
                        .content
                        .chars()
                        .take(WEB_FETCH_PREVIEW_CHARS)
                        .collect(),
                ) {
                    branch.sources.push(candidate);
                }
            }
            Err(mut error) => {
                failed = true;
                branch.attempts.append(&mut error.attempts);
                branch.push_diagnostic(error.diagnostic);
            }
        }
    }
    if failed {
        let (reason, providers_skipped) = if succeeded {
            ("partial_failure", Vec::new())
        } else {
            ("all_attempts_failed", config.web_fetch.unconfigured_names())
        };
        branch.record_gap(
            Capability::WebFetch,
            reason,
            providers_skipped,
            "one or more web_fetch targets failed",
        );
    }
    branch
}

async fn execute_vertical_search(
    query: &str,
    limit: u16,
    config: &RuntimeConfig,
    execution: CapabilityExecution,
) -> CapabilityBranch {
    let mut branch = CapabilityBranch::default();
    if config.vertical_search.configured_provider_count() == 0 {
        branch.record_gap(
            Capability::VerticalSearch,
            "no_configured_provider",
            config.vertical_search.names(),
            "vertical_search has no configured provider",
        );
        return branch;
    }
    match vertical_search(query, limit, config.vertical_search.clone(), execution).await {
        Ok(mut vertical) => {
            branch.attempts.append(&mut vertical.attempts);
            branch.push_diagnostic(vertical.diagnostic);
            branch.sources = vertical
                .results
                .into_iter()
                .map(SearchCandidate::from_vertical_result)
                .collect();
        }
        Err(mut error) => {
            branch.attempts.append(&mut error.attempts);
            branch.push_diagnostic(error.diagnostic);
            branch.record_gap(
                Capability::VerticalSearch,
                "all_attempts_failed",
                config.vertical_search.unconfigured_names(),
                "all vertical_search attempts failed",
            );
        }
    }
    branch
}

fn merge_extra_sources(outcome: &mut SearchOutcome, sources: Vec<SearchCandidate>) {
    let primary_urls = outcome
        .sources
        .iter()
        .map(|source| source.url.as_str())
        .collect::<HashSet<_>>();
    for source in sources {
        if source.url().is_some_and(|url| primary_urls.contains(url)) {
            continue;
        }
        if let Some(index) = source.url().and_then(|url| {
            outcome
                .extra_sources
                .iter()
                .position(|existing| existing.url() == Some(url))
        }) {
            if source.capability() == Capability::VerticalSearch
                && outcome.extra_sources[index].capability() != Capability::VerticalSearch
            {
                outcome.extra_sources[index] = source;
            }
            continue;
        }
        if !outcome
            .extra_sources
            .iter()
            .any(|existing| existing == &source)
        {
            outcome.extra_sources.push(source);
        }
    }
}

fn supplemental_candidates(sources: Vec<Source>, provider: &'static str) -> Vec<SearchCandidate> {
    sources
        .into_iter()
        .filter_map(|source| SearchCandidate::from_source(source, provider, Capability::WebSearch))
        .collect()
}

pub(crate) fn known_urls(query: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut urls = Vec::new();
    let mut offset = 0;
    while let Some(start) = next_url_start(&query[offset..]).map(|start| offset + start) {
        let value = &query[start..];
        let end = value.find(url_boundary).unwrap_or(value.len());
        let value = value[..end].trim_end_matches(['.', '!', '?', ':']);
        if reqwest::Url::parse(value).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
            && seen.insert(value.to_owned())
        {
            urls.push(value.to_owned());
        }
        offset = start + end;
    }
    urls
}

fn next_url_start(value: &str) -> Option<usize> {
    match (value.find("http://"), value.find("https://")) {
        (Some(http), Some(https)) => Some(http.min(https)),
        (Some(http), None) => Some(http),
        (None, Some(https)) => Some(https),
        (None, None) => None,
    }
}

fn url_boundary(character: char) -> bool {
    character.is_whitespace()
        || matches!(
            character,
            '"' | '\''
                | '<'
                | '>'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | ','
                | ';'
                | '，'
                | '；'
                | '：'
                | '。'
                | '！'
                | '？'
                | '、'
        )
}

pub(crate) async fn search(
    request: MainSearchRequest,
    config: MainSearchRuntimeConfig,
    fallback: FallbackPolicy,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    model_breakers: Arc<providers::ModelBreakers>,
) -> Result<SearchOutcome, ProviderError> {
    let step = chain::run_chain(
        provider_steps(config.into_entries()),
        provider_chain_settings(
            "main_search",
            BudgetPolicy::PrimaryFirst,
            !fallback.allows_fallback(),
            request.verbose,
            "main search deadline elapsed",
        ),
        deadline,
        |(id, provider_config), provider_deadline| {
            let client = client.clone();
            let request = request.clone();
            let model_breakers = model_breakers.clone();
            async move {
                match providers::build_main_search(
                    id,
                    provider_config,
                    client,
                    retry_policy,
                    provider_deadline,
                    model_breakers,
                )
                .search(request)
                .await
                {
                    Ok(mut outcome) => StepVerdict::Accepted(StepSuccess {
                        attempts: std::mem::take(&mut outcome.attempts),
                        diagnostic: outcome.diagnostic.take(),
                        value: outcome,
                    }),
                    Err(error) => StepVerdict::Failed(error),
                }
            }
        },
    )
    .await?;
    let mut outcome = step.value;
    outcome.attempts = step.attempts;
    outcome.diagnostic = step.diagnostic;
    Ok(outcome)
}

pub(crate) async fn fetch(
    request: FetchRequest,
    config: WebFetchRuntimeConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Result<FetchOutcome, ProviderError> {
    let step = chain::run_chain(
        provider_steps(config.into_entries()),
        web_fetch_chain_settings(request.verbose),
        deadline,
        |(id, provider_config), provider_deadline| {
            let client = client.clone();
            let request = request.clone();
            async move {
                match providers::build_web_fetch(
                    id,
                    provider_config,
                    client,
                    retry_policy,
                    provider_deadline,
                )
                .fetch(&request)
                .await
                {
                    Ok(outcome) => {
                        if is_thin(&outcome.content, &request.url) {
                            let character_count = outcome.content.chars().count();
                            StepVerdict::QualityRejected(StepRejection {
                                attempts: outcome.attempts,
                                diagnostic: outcome.diagnostic,
                                kind: AttemptErrorKind::Quality,
                                message: format!(
                                    "extracted content is too thin ({character_count} characters)"
                                ),
                            })
                        } else {
                            StepVerdict::Accepted(StepSuccess {
                                value: (outcome.provider, outcome.content),
                                attempts: outcome.attempts,
                                diagnostic: outcome.diagnostic,
                            })
                        }
                    }
                    Err(error) => StepVerdict::Failed(error),
                }
            }
        },
    )
    .await?;
    Ok(FetchOutcome {
        provider: step.value.0,
        url: redact_url(&request.url),
        content: step.value.1,
        attempts: if request.verbose {
            step.attempts
        } else {
            Vec::new()
        },
        diagnostic: step.diagnostic,
    })
}

pub(crate) async fn supplemental_web_search(
    query: &str,
    limit: u16,
    config: WebSearchRuntimeConfig,
    execution: CapabilityExecution,
) -> Result<SupplementalSearchOutcome, ProviderError> {
    let fallback_off = !execution.fallback.allows_fallback();
    let step = chain::run_chain(
        provider_steps(config.into_entries()),
        provider_chain_settings(
            "web_search",
            BudgetPolicy::SlicedEven {
                skipped_message: "skipped to preserve fallback deadline budget",
            },
            fallback_off,
            false,
            "supplemental web search has no executable provider",
        ),
        execution.deadline,
        |(id, provider_config), deadline| {
            let client = execution.client.clone();
            async move {
                match providers::build_web_search(
                    id,
                    provider_config,
                    client,
                    execution.retry_policy,
                    deadline,
                )
                .search(query, limit)
                .await
                {
                    Ok(outcome) => {
                        let success = StepSuccess {
                            value: outcome.sources,
                            attempts: outcome.attempts,
                            diagnostic: outcome.diagnostic,
                        };
                        if success.value.is_empty() {
                            StepVerdict::LegitimateEmpty(success)
                        } else {
                            StepVerdict::Accepted(success)
                        }
                    }
                    Err(error) => StepVerdict::Failed(error),
                }
            }
        },
    )
    .await?;
    Ok(SupplementalSearchOutcome {
        sources: step.value,
        attempts: step.attempts,
        diagnostic: step.diagnostic,
    })
}

pub(crate) async fn documentation_search(
    query: &str,
    limit: u16,
    config: DocsSearchRuntimeConfig,
    execution: CapabilityExecution,
) -> Result<DocumentationSearchOutcome, ProviderError> {
    let step = chain::run_chain(
        provider_steps(config.into_entries()),
        provider_chain_settings(
            "docs_search",
            BudgetPolicy::SlicedEven {
                skipped_message: "skipped to preserve fallback deadline budget",
            },
            !execution.fallback.allows_fallback(),
            false,
            "documentation search has no executable provider",
        ),
        execution.deadline,
        |(id, provider_config), deadline| {
            let client = execution.client.clone();
            async move {
                match providers::build_docs_search(
                    id,
                    provider_config,
                    client,
                    execution.retry_policy,
                    deadline,
                )
                .search(query, limit)
                .await
                {
                    Ok(outcome) => {
                        if outcome.candidate_sources.is_empty() {
                            StepVerdict::QualityRejected(StepRejection {
                                attempts: outcome.attempts,
                                diagnostic: outcome.diagnostic,
                                kind: AttemptErrorKind::Evidence,
                                message: "documentation search returned no consumable source"
                                    .into(),
                            })
                        } else {
                            StepVerdict::Accepted(StepSuccess {
                                value: outcome.candidate_sources,
                                attempts: outcome.attempts,
                                diagnostic: outcome.diagnostic,
                            })
                        }
                    }
                    Err(error) => StepVerdict::Failed(error),
                }
            }
        },
    )
    .await?;
    Ok(DocumentationSearchOutcome {
        candidate_sources: step.value,
        attempts: step.attempts,
        diagnostic: step.diagnostic,
    })
}

pub(crate) async fn documentation_read(
    locator: &EvidenceLocator,
    query: &str,
    config: DocsSearchRuntimeConfig,
    execution: CapabilityExecution,
) -> Result<DocumentationEvidence, ProviderError> {
    let Some(provider) = locator.provider() else {
        return Err(ProviderError {
            kind: AttemptErrorKind::Evidence,
            message: "documentation locator has no provider-owned reader".into(),
            attempts: Vec::new(),
            verbose: false,
            diagnostic: None,
            redirected_library_id: None,
        });
    };
    let Some(entry) = config
        .into_entries()
        .into_iter()
        .find(|entry| entry.configured() && entry.name() == provider)
    else {
        return Err(ProviderError {
            kind: AttemptErrorKind::Evidence,
            message: format!("{provider} documentation reader is not configured"),
            attempts: Vec::new(),
            verbose: false,
            diagnostic: None,
            redirected_library_id: None,
        });
    };
    let (id, provider_config, _) = entry.into_parts();
    let reader = providers::build_docs_search(
        id,
        provider_config,
        execution.client,
        execution.retry_policy,
        execution.deadline,
    );
    let Some(read) = reader.read(locator, query) else {
        return Err(ProviderError {
            kind: AttemptErrorKind::Evidence,
            message: format!("{provider} cannot read this documentation locator"),
            attempts: Vec::new(),
            verbose: false,
            diagnostic: None,
            redirected_library_id: None,
        });
    };
    let mut evidence = read.await?;
    let identity = evidence
        .locator
        .url()
        .or_else(|| evidence.locator.library_id())
        .unwrap_or_default();
    if is_thin(&evidence.content, identity) {
        chain::mark_last_attempt_failed(
            &mut evidence.attempts,
            AttemptErrorKind::Quality,
            "documentation content is too thin",
        );
        return Err(chain::chain_wide_error(
            evidence.attempts,
            false,
            evidence.diagnostic,
            "documentation content is too thin",
        ));
    }
    Ok(evidence)
}

pub(crate) async fn vertical_search(
    query: &str,
    limit: u16,
    config: VerticalSearchRuntimeConfig,
    execution: CapabilityExecution,
) -> Result<VerticalSearchOutcome, ProviderError> {
    let step = chain::run_chain(
        provider_steps(config.into_entries()),
        provider_chain_settings(
            "vertical_search",
            BudgetPolicy::SlicedEven {
                skipped_message: "skipped to preserve fallback deadline budget",
            },
            !execution.fallback.allows_fallback(),
            false,
            "vertical search has no executable provider",
        ),
        execution.deadline,
        |(id, provider_config), deadline| {
            let client = execution.client.clone();
            async move {
                match providers::build_vertical_search(
                    id,
                    provider_config,
                    client,
                    execution.retry_policy,
                    deadline,
                )
                .search(query, limit)
                .await
                {
                    Ok(outcome) => StepVerdict::Accepted(StepSuccess {
                        value: (outcome.results, outcome.sources),
                        attempts: outcome.attempts,
                        diagnostic: outcome.diagnostic,
                    }),
                    Err(error) => StepVerdict::Failed(error),
                }
            }
        },
    )
    .await?;
    Ok(VerticalSearchOutcome {
        results: step.value.0,
        sources: step.value.1,
        attempts: step.attempts,
        diagnostic: step.diagnostic,
    })
}

pub(crate) fn is_thin(content: &str, url: &str) -> bool {
    let content = content.trim();
    let character_count = content.chars().count();
    if character_count < MIN_FETCH_CONTENT_CHARS {
        return true;
    }
    if is_pdf(url) {
        return false;
    }
    let unique_lines = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<HashSet<_>>()
        .len();
    unique_lines <= DENSITY_MAX_UNIQUE_LINES && character_count < DENSITY_MAX_CHARS
}

fn is_pdf(url: &str) -> bool {
    url.split(['?', '#'])
        .next()
        .is_some_and(|path| path.to_ascii_lowercase().ends_with(".pdf"))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;

    use super::{CapabilityTargets, is_thin, web_fetch_chain_settings};
    use crate::chain::{ChainStep, StepSuccess, StepVerdict, run_chain};
    use crate::providers::{ProviderError, ProviderId};
    use crate::types::{AttemptErrorKind, Deadline};

    #[test]
    fn capability_targets_preserve_branch_local_defaults_and_positive_requests() {
        let cases = [(0, (3, 1, 1)), (1, (1, 1, 1)), (20, (20, 20, 20))];

        for (requested, expected) in cases {
            let targets = CapabilityTargets::new(requested);

            assert_eq!(
                (targets.web, targets.documentation, targets.vertical,),
                expected,
                "requested={requested}"
            );
        }
    }

    #[test]
    fn single_line_content_is_thin_for_html_but_not_pdf() {
        let content = "x".repeat(250);

        assert_eq!(
            (
                is_thin(&content, "https://example.test/page"),
                is_thin(&content, "https://example.test/file.pdf"),
            ),
            (true, false)
        );
    }

    #[test]
    fn web_fetch_preserves_fallback_budget_after_an_attempt_timeout() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .start_paused(true)
            .build()
            .expect("test runtime");

        runtime.block_on(async {
            let budgets = Rc::new(RefCell::new(Vec::new()));
            let outcome = run_chain(
                vec![
                    ChainStep {
                        context: (ProviderId::Jina, ()),
                        configured: true,
                        gate_attempt: None,
                    },
                    ChainStep {
                        context: (ProviderId::Tavily, ()),
                        configured: true,
                        gate_attempt: None,
                    },
                ],
                web_fetch_chain_settings(true),
                Deadline::new(Duration::from_secs(12)),
                {
                    let budgets = Rc::clone(&budgets);
                    move |(provider, ()), deadline| {
                        budgets
                            .borrow_mut()
                            .push(deadline.remaining().unwrap_or_default());
                        async move {
                            if provider == ProviderId::Jina {
                                tokio::time::sleep(Duration::from_secs(6)).await;
                                StepVerdict::Failed(ProviderError {
                                    kind: AttemptErrorKind::Timeout,
                                    message: "timed out".into(),
                                    attempts: Vec::new(),
                                    verbose: true,
                                    diagnostic: None,
                                    redirected_library_id: None,
                                })
                            } else {
                                StepVerdict::Accepted(StepSuccess {
                                    value: "fallback content",
                                    attempts: Vec::new(),
                                    diagnostic: None,
                                })
                            }
                        }
                    }
                },
            )
            .await
            .expect("fallback succeeds");

            assert_eq!(outcome.value, "fallback content");
            let budgets = budgets.borrow();
            assert_eq!(budgets[0], Duration::from_secs(6));
            assert!(
                budgets[1].abs_diff(Duration::from_secs(6)) < Duration::from_millis(1),
                "fallback budget: {:?}",
                budgets[1]
            );
        });
    }
}
