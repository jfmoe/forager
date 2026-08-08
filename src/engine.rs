use std::collections::HashSet;
use std::sync::Arc;

use futures_util::{StreamExt, stream};
use reqwest::Client;

use crate::config::{
    DocsSearchRuntimeConfig, MainSearchRuntimeConfig, RuntimeConfig, SeamEntry,
    VerticalSearchRuntimeConfig, WebFetchRuntimeConfig, WebSearchRuntimeConfig,
};
use crate::net::{RetryPolicy, combine_diagnostics, slice_budget};
use crate::providers::{self, FetchRequest, ProviderError, SearchRequest};
use crate::redact::redact_url;
use crate::types::{
    AttemptErrorKind, Capability, CapabilityGap, CapabilitySet, DENSITY_MAX_CHARS,
    DENSITY_MAX_UNIQUE_LINES, Deadline, DocumentationEvidence, DocumentationSearchOutcome,
    EvidenceLocator, FetchOutcome, MIN_FETCH_CONTENT_CHARS, ProviderAttempt, SearchCandidate,
    SearchOutcome, Source, SupplementalSearchOutcome, VerticalSearchOutcome,
};

pub(crate) const FANOUT_CONCURRENCY: usize = 4;
const WEB_FETCH_PREVIEW_CHARS: usize = 300;

#[derive(Clone)]
pub(crate) struct CapabilityExecution {
    pub(crate) fallback: String,
    pub(crate) client: Client,
    pub(crate) retry_policy: RetryPolicy,
    pub(crate) deadline: Deadline,
}

#[derive(Clone, Copy)]
enum BudgetPolicy {
    PrimaryFirst,
    SlicedEven,
}

struct ChainSettings {
    seam: &'static str,
    budget_policy: BudgetPolicy,
    exhausted_message: &'static str,
    error_verbose: bool,
    fallback_off: bool,
}

struct ChainStep<T> {
    value: T,
    attempts: Vec<ProviderAttempt>,
    diagnostic: Option<String>,
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

async fn run_provider_chain<C, T, Run, Fut, Accept>(
    entries: Vec<SeamEntry<C>>,
    settings: ChainSettings,
    deadline: Deadline,
    accept: Accept,
    mut run: Run,
) -> Result<ChainStep<T>, ProviderError>
where
    Run: FnMut(providers::ProviderId, C, Deadline) -> Fut,
    Fut: std::future::Future<Output = Result<ChainStep<T>, ProviderError>>,
    Accept: Fn(&T) -> bool,
{
    let entries = if settings.fallback_off {
        entries.into_iter().take(1).collect::<Vec<_>>()
    } else {
        entries
            .into_iter()
            .filter(SeamEntry::configured)
            .collect::<Vec<_>>()
    };
    let total = entries.len();
    let mut attempts = Vec::new();
    let mut diagnostics = Vec::new();
    let mut unconsumed_success = None;
    for (index, entry) in entries.into_iter().enumerate() {
        let (id, config, configured) = entry.into_parts();
        if !configured {
            attempts.push(synthetic_attempt(
                id,
                settings.seam,
                AttemptErrorKind::Auth,
                format!("{} has no configured credentials", id.name()),
            ));
            break;
        }
        let Some(remaining) = deadline.remaining() else {
            break;
        };
        let budget = match settings.budget_policy {
            BudgetPolicy::PrimaryFirst => remaining,
            BudgetPolicy::SlicedEven => {
                let Some(budget) = slice_budget(remaining, total - index) else {
                    attempts.push(synthetic_attempt(
                        id,
                        settings.seam,
                        AttemptErrorKind::Timeout,
                        "skipped to preserve fallback deadline budget".into(),
                    ));
                    continue;
                };
                budget
            }
        };
        match run(id, config, Deadline::new(budget)).await {
            Ok(mut step) => {
                attempts.append(&mut step.attempts);
                if let Some(diagnostic) = step.diagnostic.take() {
                    diagnostics.push(diagnostic);
                }
                if accept(&step.value) {
                    return Ok(ChainStep {
                        value: step.value,
                        attempts,
                        diagnostic: combine_diagnostics(diagnostics),
                    });
                }
                unconsumed_success = Some(step.value);
            }
            Err(mut error) => {
                attempts.append(&mut error.attempts);
                if let Some(diagnostic) = error.diagnostic {
                    diagnostics.push(diagnostic);
                }
            }
        }
    }
    if let Some(value) = unconsumed_success {
        return Ok(ChainStep {
            value,
            attempts,
            diagnostic: combine_diagnostics(diagnostics),
        });
    }
    Err(provider_chain_error(
        attempts,
        settings.error_verbose,
        combine_diagnostics(diagnostics),
        settings.exhausted_message,
    ))
}

fn provider_chain_error(
    attempts: Vec<ProviderAttempt>,
    verbose: bool,
    diagnostic: Option<String>,
    exhausted_message: &str,
) -> ProviderError {
    let terminal = terminal_attempt(&attempts);
    ProviderError {
        kind: terminal
            .and_then(|attempt| attempt.error_kind)
            .unwrap_or(AttemptErrorKind::Timeout),
        message: terminal.map_or_else(
            || exhausted_message.to_owned(),
            |attempt| attempt.message.clone(),
        ),
        attempts,
        verbose,
        diagnostic,
        redirected_library_id: None,
    }
}

fn synthetic_attempt(
    id: providers::ProviderId,
    seam: &'static str,
    kind: AttemptErrorKind,
    message: String,
) -> ProviderAttempt {
    ProviderAttempt {
        provider: id.name(),
        seam,
        error_kind: Some(kind),
        http_status: None,
        duration_ms: 0,
        credential_index: 0,
        retry_count: 0,
        rotation_count: 0,
        message,
        model: None,
        transport: None,
        endpoint_host: None,
        breaker_event: None,
    }
}

impl CapabilityExecution {
    pub(crate) fn new(
        fallback: &str,
        client: Client,
        retry_policy: RetryPolicy,
        deadline: Deadline,
    ) -> Self {
        Self {
            fallback: fallback.into(),
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
            let provider = successful_provider(&supplemental.attempts, "web_search");
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

fn successful_provider(attempts: &[ProviderAttempt], seam: &str) -> &'static str {
    attempts
        .iter()
        .rev()
        .find(|attempt| attempt.seam == seam && attempt.error_kind.is_none())
        .map_or_else(
            || unreachable!("successful search outcome records a provider attempt"),
            |attempt| attempt.provider,
        )
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
    request: SearchRequest,
    config: MainSearchRuntimeConfig,
    fallback: &str,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    model_breakers: Arc<providers::ModelBreakers>,
) -> Result<SearchOutcome, ProviderError> {
    let step = run_provider_chain(
        config.into_entries(),
        ChainSettings {
            seam: "main_search",
            budget_policy: BudgetPolicy::PrimaryFirst,
            exhausted_message: "main search deadline elapsed",
            error_verbose: request.verbose,
            fallback_off: fallback == "off",
        },
        deadline,
        |_| true,
        |id, provider_config, provider_deadline| {
            let client = client.clone();
            let request = request.clone();
            let model_breakers = model_breakers.clone();
            async move {
                let mut outcome = providers::build_main_search(
                    id,
                    provider_config,
                    client,
                    retry_policy,
                    provider_deadline,
                    model_breakers,
                )
                .search(request)
                .await?;
                Ok(ChainStep {
                    attempts: std::mem::take(&mut outcome.attempts),
                    diagnostic: outcome.diagnostic.take(),
                    value: outcome,
                })
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
    let step = run_provider_chain(
        config.into_entries(),
        ChainSettings {
            seam: "web_fetch",
            budget_policy: BudgetPolicy::SlicedEven,
            exhausted_message: "web fetch deadline elapsed",
            error_verbose: request.verbose,
            fallback_off: false,
        },
        deadline,
        |_| true,
        |id, provider_config, provider_deadline| {
            let client = client.clone();
            let request = request.clone();
            async move {
                let mut outcome = providers::build_web_fetch(
                    id,
                    provider_config,
                    client,
                    retry_policy,
                    provider_deadline,
                )
                .fetch(&request)
                .await?;
                if is_thin(&outcome.content, &request.url) {
                    let character_count = outcome.content.chars().count();
                    if let Some(attempt) = outcome.attempts.last_mut() {
                        attempt.error_kind = Some(AttemptErrorKind::Quality);
                        attempt.message =
                            format!("extracted content is too thin ({character_count} characters)");
                    }
                    return Err(provider_chain_error(
                        outcome.attempts,
                        request.verbose,
                        outcome.diagnostic,
                        "web fetch content is too thin",
                    ));
                }
                Ok(ChainStep {
                    value: (outcome.provider, outcome.content),
                    attempts: outcome.attempts,
                    diagnostic: outcome.diagnostic,
                })
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
    let fallback_off = execution.fallback == "off";
    let step = run_provider_chain(
        config.into_entries(),
        ChainSettings {
            seam: "web_search",
            budget_policy: BudgetPolicy::SlicedEven,
            exhausted_message: "supplemental web search has no executable provider",
            error_verbose: false,
            fallback_off,
        },
        execution.deadline,
        |_| true,
        |id, provider_config, deadline| {
            let client = execution.client.clone();
            async move {
                let outcome = providers::build_web_search(
                    id,
                    provider_config,
                    client,
                    execution.retry_policy,
                    deadline,
                )
                .search(query, limit)
                .await?;
                Ok(ChainStep {
                    value: outcome.sources,
                    attempts: outcome.attempts,
                    diagnostic: outcome.diagnostic,
                })
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
    let step = run_provider_chain(
        config.into_entries(),
        ChainSettings {
            seam: "docs_search",
            budget_policy: BudgetPolicy::SlicedEven,
            exhausted_message: "documentation search has no executable provider",
            error_verbose: false,
            fallback_off: execution.fallback == "off",
        },
        execution.deadline,
        |candidate_sources: &Vec<SearchCandidate>| !candidate_sources.is_empty(),
        |id, provider_config, deadline| {
            let client = execution.client.clone();
            async move {
                let mut outcome = providers::build_docs_search(
                    id,
                    provider_config,
                    client,
                    execution.retry_policy,
                    deadline,
                )
                .search(query, limit)
                .await?;
                if outcome.candidate_sources.is_empty() {
                    if let Some(attempt) = outcome.attempts.last_mut() {
                        attempt.error_kind = Some(AttemptErrorKind::Evidence);
                        attempt.message =
                            "documentation search returned no consumable source".into();
                    }
                    return Err(provider_chain_error(
                        outcome.attempts,
                        false,
                        outcome.diagnostic,
                        "documentation search returned no consumable source",
                    ));
                }
                Ok(ChainStep {
                    value: outcome.candidate_sources,
                    attempts: outcome.attempts,
                    diagnostic: outcome.diagnostic,
                })
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
        if let Some(attempt) = evidence.attempts.last_mut() {
            attempt.error_kind = Some(AttemptErrorKind::Quality);
            attempt.message = "documentation content is too thin".into();
        }
        return Err(provider_chain_error(
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
    let step = run_provider_chain(
        config.into_entries(),
        ChainSettings {
            seam: "vertical_search",
            budget_policy: BudgetPolicy::SlicedEven,
            exhausted_message: "vertical search has no executable provider",
            error_verbose: false,
            fallback_off: execution.fallback == "off",
        },
        execution.deadline,
        |_| true,
        |id, provider_config, deadline| {
            let client = execution.client.clone();
            async move {
                let outcome = providers::build_vertical_search(
                    id,
                    provider_config,
                    client,
                    execution.retry_policy,
                    deadline,
                )
                .search(query, limit)
                .await?;
                Ok(ChainStep {
                    value: (outcome.results, outcome.sources),
                    attempts: outcome.attempts,
                    diagnostic: outcome.diagnostic,
                })
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

pub(crate) fn terminal_kind(attempts: &[ProviderAttempt]) -> AttemptErrorKind {
    terminal_attempt(attempts)
        .and_then(|attempt| attempt.error_kind)
        .unwrap_or(AttemptErrorKind::Timeout)
}

fn terminal_attempt(attempts: &[ProviderAttempt]) -> Option<&ProviderAttempt> {
    let mut final_providers = HashSet::new();
    attempts
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, attempt)| final_providers.insert(attempt.provider))
        .filter_map(|(index, attempt)| attempt.error_kind.map(|kind| (index, attempt, kind)))
        .max_by_key(|(index, _, kind)| (error_priority(*kind), *index))
        .map(|(_, attempt, _)| attempt)
}

fn error_priority(kind: AttemptErrorKind) -> u8 {
    match kind {
        AttemptErrorKind::Network => 0,
        AttemptErrorKind::Timeout => 1,
        AttemptErrorKind::RateLimited => 2,
        AttemptErrorKind::QuotaExhausted => 3,
        AttemptErrorKind::Auth => 4,
        AttemptErrorKind::Parameter => 5,
        AttemptErrorKind::Runtime => 6,
        AttemptErrorKind::Quality => 7,
        AttemptErrorKind::Evidence => 8,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{
        CapabilityTargets, error_priority, provider_chain_error, terminal_attempt, terminal_kind,
    };
    use crate::net::slice_budget;
    use crate::types::{AttemptErrorKind, ProviderAttempt};

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
    fn provider_budget_preserves_reachable_fallback_slots_at_every_boundary() {
        for (remaining, slots, expected) in [
            (0, 1, Some(Duration::ZERO)),
            (4, 1, Some(Duration::from_secs(4))),
            (9, 2, None),
            (10, 2, Some(Duration::from_secs(5))),
            (11, 2, Some(Duration::from_millis(5_500))),
            (14, 3, None),
            (15, 3, Some(Duration::from_secs(5))),
            (16, 3, Some(Duration::from_nanos(5_333_333_333))),
        ] {
            assert_eq!(
                slice_budget(Duration::from_secs(remaining), slots),
                expected,
                "remaining={remaining}, slots={slots}"
            );
        }
    }

    #[test]
    fn terminal_kind_exhausts_final_kind_pairs_and_ignores_history() {
        for first in ALL_KINDS {
            for second in ALL_KINDS {
                let expected = if error_priority(first) >= error_priority(second) {
                    first
                } else {
                    second
                };
                assert_eq!(
                    terminal_kind(&[
                        attempt("tavily", AttemptErrorKind::Evidence),
                        attempt("tavily", first),
                        attempt("jina", AttemptErrorKind::Evidence),
                        attempt("jina", second),
                    ]),
                    expected,
                    "first={first:?}, second={second:?}"
                );
            }
        }
    }

    #[test]
    fn terminal_attempt_uses_the_later_provider_when_kinds_match() {
        let attempts = vec![
            attempt_with_message("xai", AttemptErrorKind::Auth, "first"),
            attempt_with_message("openai_compatible", AttemptErrorKind::Auth, "second"),
        ];

        assert_eq!(
            terminal_attempt(&attempts).map(|attempt| attempt.message.as_str()),
            Some("second")
        );
    }

    #[test]
    fn fetch_terminal_error_uses_kind_and_message_from_the_same_attempt() {
        let attempts = vec![
            attempt_with_message("tavily", AttemptErrorKind::Quality, "thin content"),
            attempt_with_message("jina", AttemptErrorKind::Network, "connection reset"),
        ];

        let error = provider_chain_error(attempts, false, None, "web fetch deadline elapsed");

        assert_eq!(
            (error.kind, error.message.as_str()),
            (AttemptErrorKind::Quality, "thin content")
        );
    }

    const ALL_KINDS: [AttemptErrorKind; 9] = [
        AttemptErrorKind::Auth,
        AttemptErrorKind::RateLimited,
        AttemptErrorKind::QuotaExhausted,
        AttemptErrorKind::Parameter,
        AttemptErrorKind::Timeout,
        AttemptErrorKind::Network,
        AttemptErrorKind::Quality,
        AttemptErrorKind::Evidence,
        AttemptErrorKind::Runtime,
    ];

    fn attempt(provider: &'static str, kind: AttemptErrorKind) -> ProviderAttempt {
        attempt_with_message(provider, kind, "")
    }

    fn attempt_with_message(
        provider: &'static str,
        kind: AttemptErrorKind,
        message: &str,
    ) -> ProviderAttempt {
        ProviderAttempt {
            provider,
            seam: "web_fetch",
            error_kind: Some(kind),
            http_status: None,
            duration_ms: 0,
            credential_index: 0,
            retry_count: 0,
            rotation_count: 0,
            message: message.into(),
            model: None,
            transport: None,
            endpoint_host: None,
            breaker_event: None,
        }
    }
}
