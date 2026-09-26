//! Platform fetch: the metadata route chain and the full-text stage.
//!
//! The platform's fetch routes form a fallback chain that returns the item metadata; a metadata
//! failure ends the command. At full-text depth, the route that answered orders the full-text
//! URLs of the version it returned, and each URL runs the global Web Fetch chain until one
//! succeeds. Route adapters never import Web Fetch providers.

use reqwest::Client;

use crate::catalog::{self, PlatformOperation, ProviderId};
use crate::chain::{
    self, BudgetPolicy, ChainSettings, ChainStep, DiagnosticMerge, StepSuccess, StepVerdict,
    TerminalPolicy,
};
use crate::config::{PlatformRouteConfig, PlatformRuntimeConfig, WebFetchRuntimeConfig};
use crate::engine;
use crate::net::{RetryPolicy, combine_diagnostics};
use crate::platform_chain::{
    PlatformPreflightError, operation_target, plan_routes, route_candidates, route_identity,
};
use crate::providers::{self, FetchRequest};
use crate::types::{
    AttemptErrorKind, ContentDepth, Deadline, Platform, PlatformContent, PlatformFetchRequest,
    PlatformFetchResult, PlatformItem, ProviderAttempt, ProviderError,
};

/// The routes a platform fetch runs, in order, and the routes skipped before it runs.
pub(crate) struct PlatformFetchPlan {
    platform: Platform,
    request: PlatformFetchRequest,
    routes: Vec<PlatformRouteConfig>,
    skipped: Vec<ProviderAttempt>,
}

/// Plans a fetch over the configured routes that can serve the requested depth.
pub(crate) fn plan_fetch(
    config: &PlatformRuntimeConfig,
    request: PlatformFetchRequest,
) -> Result<PlatformFetchPlan, PlatformPreflightError> {
    let plan = plan_routes(
        PlatformOperation::Fetch,
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
/// configured provider.
pub(crate) async fn fetch(
    plan: PlatformFetchPlan,
    web_fetch: WebFetchRuntimeConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    verbose: bool,
) -> Result<PlatformFetchResult, ProviderError> {
    let PlatformFetchPlan {
        platform,
        request,
        routes,
        skipped,
    } = plan;
    let mut attempts = skipped;
    let metadata = match fetch_metadata(
        platform,
        &request,
        routes,
        client.clone(),
        retry_policy,
        deadline,
        verbose,
    )
    .await
    {
        Ok(metadata) => metadata,
        Err(mut error) => {
            attempts.append(&mut error.attempts);
            error.attempts = attempts;
            return Err(error);
        }
    };
    attempts.extend(metadata.attempts);
    let mut item = metadata.item;
    let mut diagnostic = metadata.diagnostic;
    let mut content = None;
    if request.depth == ContentDepth::FullText {
        match read_full_text(
            metadata.content_urls,
            &web_fetch,
            &client,
            retry_policy,
            deadline,
        )
        .await
        {
            Ok(body) => {
                attempts.extend(body.attempts);
                item.depth = ContentDepth::FullText;
                diagnostic =
                    combine_diagnostics([diagnostic, body.diagnostic].into_iter().flatten());
                content = Some(body.content);
            }
            Err(mut error) => {
                attempts.append(&mut error.attempts);
                error.attempts = attempts;
                error.verbose = verbose;
                error.diagnostic =
                    combine_diagnostics([diagnostic, error.diagnostic].into_iter().flatten());
                return Err(error);
            }
        }
    }
    Ok(PlatformFetchResult {
        platform,
        provider: metadata.route.name(),
        item,
        content,
        attempts: if verbose { attempts } else { Vec::new() },
        diagnostic,
    })
}

struct Metadata {
    route: ProviderId,
    item: PlatformItem,
    content_urls: Vec<String>,
    attempts: Vec<ProviderAttempt>,
    diagnostic: Option<String>,
}

async fn fetch_metadata(
    platform: Platform,
    request: &PlatformFetchRequest,
    routes: Vec<PlatformRouteConfig>,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    verbose: bool,
) -> Result<Metadata, ProviderError> {
    let steps = routes
        .into_iter()
        .map(|context| ChainStep {
            context,
            configured: true,
            gate_attempt: None,
        })
        .collect();
    let outcome = chain::run_chain(
        steps,
        ChainSettings {
            target: operation_target(platform, PlatformOperation::Fetch),
            budget_policy: BudgetPolicy::SlicedEven {
                skipped_message: "skipped to preserve fallback deadline budget",
            },
            fallback_off: false,
            diagnostic_merge: DiagnosticMerge::Join,
            terminal: TerminalPolicy::ChainWide {
                verbose,
                exhausted_message: "platform fetch deadline elapsed",
            },
            identity: &route_identity,
            continue_on_failure: &chain::always_continue,
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
                        value: (route, outcome.item, outcome.content_urls),
                        attempts: outcome.attempts,
                        diagnostic: outcome.diagnostic,
                    }),
                    Err(error) => StepVerdict::Failed(error),
                }
            }
        },
    )
    .await?;
    let (route, item, content_urls) = outcome.value;
    Ok(Metadata {
        route,
        item,
        content_urls,
        attempts: outcome.attempts,
        diagnostic: outcome.diagnostic,
    })
}

struct FullText {
    content: PlatformContent,
    attempts: Vec<ProviderAttempt>,
    diagnostic: Option<String>,
}

/// Reads the first URL whose Web Fetch chain succeeds. A URL with a later fallback gets half of
/// the remaining budget; the last URL's failure is the terminal state.
async fn read_full_text(
    urls: Vec<String>,
    web_fetch: &WebFetchRuntimeConfig,
    client: &Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
) -> Result<FullText, ProviderError> {
    let mut attempts = Vec::new();
    let mut last_error = None;
    let count = urls.len();
    for (index, url) in urls.into_iter().enumerate() {
        let stage_deadline = if index + 1 < count {
            fallback_share(deadline)
        } else {
            deadline
        };
        // The attempts of every stage are kept; the caller drops them unless verbose.
        let request = FetchRequest {
            url: url.clone(),
            verbose: true,
        };
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
                return Ok(FullText {
                    content: PlatformContent::new(url, outcome.provider, outcome.content),
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
        verbose: false,
        diagnostic: None,
        redirected_library_id: None,
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
