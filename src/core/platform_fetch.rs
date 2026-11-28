//! Platform fetch: the metadata route chain and the full-text stage.
//!
//! The platform's fetch routes form a fallback chain that returns the item metadata; a metadata
//! failure ends the command. At full-text depth, the route that answered declares the full-text
//! source of the version it returned: URLs to read in order, each of which runs the global Web
//! Fetch chain until one succeeds, or a native body the route already read and verified, which
//! needs no Web Fetch. Route adapters never import Web Fetch providers.
//!
//! At thread depth each platform declares how the discussion is delivered: a route that returns
//! a native body (Hacker News) has it delivered by reference like a full text; any other route
//! carries the thread inline in its item (X).

use reqwest::Client;

use crate::catalog::{self, PlatformOperation, ProviderId};
use crate::chain::{
    self, BudgetPolicy, ChainOutcome, ChainSettings, ChainStep, DiagnosticMerge, LeadingTrace,
    StepSuccess, StepVerdict, TerminalPolicy,
};
use crate::config::{PlatformRouteConfig, PlatformRuntimeConfig, WebFetchRuntimeConfig};
use crate::engine;
use crate::net::RetryPolicy;
use crate::platform_chain::{
    PlatformPreflightError, operation_target, plan_routes, route_candidates, route_identity,
};
use crate::providers::{self, FetchRequest};
use crate::types::{
    AttemptErrorKind, ContentDepth, Deadline, FullTextSource, Platform, PlatformContent,
    PlatformFetchRequest, PlatformFetchResult, PlatformItem, ProviderAttempt, ProviderError,
};

/// The routes a platform fetch runs, in order, and the routes skipped before it runs.
pub(crate) struct PlatformFetchPlan {
    platform: Platform,
    request: PlatformFetchRequest,
    routes: Vec<PlatformRouteConfig>,
    skipped: Vec<ProviderAttempt>,
}

impl PlatformFetchPlan {
    /// Returns whether running the plan may read a full text through Web Fetch: the request asks
    /// for the full text and some planned route does not read it natively.
    pub(crate) fn needs_web_fetch(&self) -> bool {
        self.request.depth == ContentDepth::FullText
            && self
                .routes
                .iter()
                .any(|route| !catalog::registration(route.route()).native_full_text)
    }
}

/// Plans a fetch over the configured routes that can serve the requested depth.
pub(crate) fn plan_fetch(
    config: &PlatformRuntimeConfig,
    request: PlatformFetchRequest,
) -> Result<PlatformFetchPlan, PlatformPreflightError> {
    let plan = plan_routes(
        PlatformOperation::Fetch.as_str(),
        config.platform(),
        &config.order_key(),
        catalog::platform(config.platform()).routes(PlatformOperation::Fetch),
        &route_candidates(config),
        None,
        |id| providers::platform_fetch_support(id, &request),
    )?;
    Ok(PlatformFetchPlan {
        platform: config.platform(),
        request,
        routes: plan.routes.into_iter().map(|(_, config)| config).collect(),
        skipped: plan.skipped,
    })
}

/// Runs a planned fetch. At full-text depth the body comes from `web_fetch`, which must have a
/// configured provider when [`PlatformFetchPlan::needs_web_fetch`] says so.
pub(crate) async fn fetch(
    plan: PlatformFetchPlan,
    web_fetch: WebFetchRuntimeConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Result<PlatformFetchResult, ProviderError> {
    let PlatformFetchPlan {
        platform,
        request,
        routes,
        skipped,
    } = plan;
    let ChainOutcome {
        value: (route, mut item, content_source),
        mut attempts,
        mut diagnostic,
    } = LeadingTrace::new(skipped).lead(
        fetch_metadata(
            platform,
            &request,
            routes,
            client.clone(),
            retry_policy,
            deadline,
        )
        .await,
    )?;
    let mut content = None;
    if request.depth == ContentDepth::Thread
        && let FullTextSource::Native(body) = content_source
    {
        content = Some(PlatformContent::new(item.url.clone(), route.name(), body));
    } else if request.depth == ContentDepth::FullText {
        let full_text = match content_source {
            FullTextSource::Native(body) => Ok(ChainOutcome {
                value: PlatformContent::new(item.url.clone(), route.name(), body),
                attempts: Vec::new(),
                diagnostic: None,
            }),
            FullTextSource::Urls(urls) => {
                read_url_full_text(urls, &web_fetch, &client, retry_policy, deadline).await
            }
        };
        let body = LeadingTrace {
            attempts,
            diagnostic,
        }
        .lead(full_text)?;
        item.depth = ContentDepth::FullText;
        content = Some(body.value);
        attempts = body.attempts;
        diagnostic = body.diagnostic;
    }
    Ok(PlatformFetchResult {
        platform,
        provider: route.name(),
        item,
        content,
        attempts,
        diagnostic,
    })
}

/// Runs a planned fetch for its item alone; a full-text source the route declares is left
/// unread. The outcome carries every attempt of the read, the routes planning skipped first, and
/// so does a failure.
pub(crate) async fn fetch_item(
    plan: PlatformFetchPlan,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Result<ChainOutcome<PlatformItem>, ProviderError> {
    let PlatformFetchPlan {
        platform,
        request,
        routes,
        skipped,
    } = plan;
    let metadata = fetch_metadata(platform, &request, routes, client, retry_policy, deadline).await;
    LeadingTrace::new(skipped)
        .lead(metadata)
        .map(|outcome| outcome.map(|(_, item, _)| item))
}

/// Runs the metadata route chain; its value is the route that answered, the item and where the
/// item's full text lives.
async fn fetch_metadata(
    platform: Platform,
    request: &PlatformFetchRequest,
    routes: Vec<PlatformRouteConfig>,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Result<ChainOutcome<(ProviderId, PlatformItem, FullTextSource)>, ProviderError> {
    let steps = routes
        .into_iter()
        .map(|context| ChainStep {
            context,
            configured: true,
        })
        .collect();
    chain::run_chain(
        steps,
        ChainSettings {
            target: operation_target(platform, PlatformOperation::Fetch.as_str()),
            budget_policy: BudgetPolicy::SlicedEven {
                skipped_message: "skipped to preserve fallback deadline budget",
            },
            fallback_off: false,
            diagnostic_merge: DiagnosticMerge::Join,
            terminal: TerminalPolicy::ChainWide {
                exhausted_message: "platform fetch deadline elapsed",
            },
            identity: &route_identity,
        },
        deadline,
        |config, step_deadline| {
            let client = client.clone();
            async move {
                let route = config.route();
                let adapter =
                    providers::build_platform_fetch(config, client, retry_policy, step_deadline);
                match adapter.fetch(request).await {
                    Ok(outcome) => StepVerdict::Accepted(StepSuccess {
                        value: (route, outcome.item, outcome.content_source),
                        attempts: outcome.attempts,
                        diagnostic: outcome.diagnostic,
                    }),
                    Err(error) => StepVerdict::Failed(error),
                }
            }
        },
    )
    .await
}

/// Reads the first URL whose Web Fetch chain succeeds. A URL with a later fallback gets half of
/// the remaining budget; the last URL's failure is the terminal state.
async fn read_url_full_text(
    urls: Vec<String>,
    web_fetch: &WebFetchRuntimeConfig,
    client: &Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Result<ChainOutcome<PlatformContent>, ProviderError> {
    let mut attempts = Vec::new();
    let mut last_error = None;
    let count = urls.len();
    for (index, url) in urls.into_iter().enumerate() {
        let stage_deadline = if index + 1 < count {
            fallback_share(deadline)
        } else {
            deadline
        };
        let request = FetchRequest { url: url.clone() };
        match engine::fetch(
            request,
            web_fetch.clone(),
            client.clone(),
            retry_policy,
            stage_deadline,
        )
        .await
        {
            Ok(outcome) => {
                attempts.extend(outcome.attempts);
                return Ok(ChainOutcome {
                    value: PlatformContent::new(url, outcome.provider, outcome.content),
                    attempts,
                    diagnostic: outcome.diagnostic,
                });
            }
            Err(mut error) => {
                attempts.append(&mut error.attempts);
                last_error = Some(error);
            }
        }
    }
    let mut error = last_error.unwrap_or_else(|| ProviderError {
        kind: AttemptErrorKind::Runtime,
        message: "the route declared no full-text URL".into(),
        attempts: Vec::new(),
        diagnostic: None,
    });
    error.attempts = attempts;
    Err(error)
}

/// Returns a deadline for half of the remaining budget, keeping the other half for fallback.
fn fallback_share(deadline: Deadline) -> Deadline {
    deadline
        .remaining()
        .map_or(deadline, |remaining| Deadline::new(remaining / 2))
}

#[cfg(test)]
#[path = "platform_fetch_tests.rs"]
mod tests;
