use std::collections::HashSet;

use futures_util::{StreamExt, stream};

use crate::attempt_trace;
use crate::config::RuntimeConfig;
use crate::engine::{
    CapabilityExecution, FANOUT_CONCURRENCY, documentation_search, fetch, known_urls,
    supplemental_web_search, vertical_search,
};
use crate::net::combine_diagnostics;
use crate::providers::{FetchRequest, ProviderError};
use crate::redact::redact_url;
use crate::types::{
    Capability, CapabilityGap, CapabilitySet, ProviderAttempt, SearchCandidate, SearchOutcome,
    Source,
};

const WEB_FETCH_PREVIEW_CHARS: usize = 300;

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

pub(crate) struct CapabilityResults {
    branches: Vec<CapabilityBranch>,
}

/// A Default Search Invocation whose main search failed after supplemental capabilities ran.
#[derive(Debug)]
pub struct SearchFailure {
    /// Main-search failure; it alone decides the terminal attribution.
    pub error: ProviderError,
    /// Search Candidates produced by supplemental capabilities before the failure.
    pub extra_sources: Vec<SearchCandidate>,
    /// Supplemental capabilities that could not be covered.
    pub capability_gaps: Vec<CapabilityGap>,
}

impl CapabilityResults {
    pub(crate) fn merge_into(self, outcome: &mut SearchOutcome) {
        for mut branch in self.branches {
            outcome.attempts.append(&mut branch.attempts);
            outcome.capability_gaps.append(&mut branch.capability_gaps);
            outcome.diagnostic = append_diagnostics(outcome.diagnostic.take(), branch.diagnostics);
            merge_extra_sources(&outcome.sources, &mut outcome.extra_sources, branch.sources);
        }
    }

    pub(crate) fn into_failure(self, mut error: ProviderError) -> SearchFailure {
        let mut extra_sources = Vec::new();
        let mut capability_gaps = Vec::new();
        for mut branch in self.branches {
            error.attempts.append(&mut branch.attempts);
            capability_gaps.append(&mut branch.capability_gaps);
            error.diagnostic = append_diagnostics(error.diagnostic.take(), branch.diagnostics);
            merge_extra_sources(&[], &mut extra_sources, branch.sources);
        }
        SearchFailure {
            error,
            extra_sources,
            capability_gaps,
        }
    }
}

fn append_diagnostics(current: Option<String>, diagnostics: Vec<String>) -> Option<String> {
    combine_diagnostics(current.into_iter().chain(diagnostics))
}

pub(crate) async fn execute_capabilities(
    query: &str,
    capabilities: &CapabilitySet,
    requested_target: u16,
    config: &RuntimeConfig,
    execution: CapabilityExecution,
) -> CapabilityResults {
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
    CapabilityResults { branches }
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

fn merge_extra_sources(
    primary: &[Source],
    extra_sources: &mut Vec<SearchCandidate>,
    sources: Vec<SearchCandidate>,
) {
    let primary_urls = primary
        .iter()
        .map(|source| source.url.as_str())
        .collect::<HashSet<_>>();
    for source in sources {
        if source.url().is_some_and(|url| primary_urls.contains(url)) {
            continue;
        }
        if let Some(index) = source.url().and_then(|url| {
            extra_sources
                .iter()
                .position(|existing| existing.url() == Some(url))
        }) {
            if source.capability() == Capability::VerticalSearch
                && extra_sources[index].capability() != Capability::VerticalSearch
            {
                extra_sources[index] = source;
            }
            continue;
        }
        if !extra_sources.iter().any(|existing| existing == &source) {
            extra_sources.push(source);
        }
    }
}

fn supplemental_candidates(sources: Vec<Source>, provider: &'static str) -> Vec<SearchCandidate> {
    sources
        .into_iter()
        .filter_map(|source| SearchCandidate::from_source(source, provider, Capability::WebSearch))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::CapabilityTargets;

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
}
