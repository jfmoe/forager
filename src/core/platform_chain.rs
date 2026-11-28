//! Platform route chains: route planning, and the chain of platform search.
//!
//! The routes of one platform form a fallback chain in configured order through the shared
//! chain runner; a result never falls back to another platform. Planning is pure: it decides
//! before any request which routes run, which are skipped, and which preflight error ends the
//! command. Search returns a search page and issues cursors bound to its operation.

use reqwest::Client;

use crate::catalog::{self, PlatformOperation, ProviderId, ProviderTransport};
use crate::chain::{
    self, BudgetPolicy, ChainSettings, ChainStep, DiagnosticMerge, LeadingTrace, StepIdentity,
    StepSuccess, StepVerdict, TerminalPolicy,
};
use crate::config::{self, BrowserRuntimeConfig, PlatformRouteConfig, PlatformRuntimeConfig};
use crate::net::RetryPolicy;
use crate::page_cursor::{self, PagedRequest};
use crate::providers;
use crate::types::{
    AttemptTarget, Deadline, Platform, PlatformSearchPage, PlatformSearchRequest, ProviderAttempt,
    ProviderError,
};

/// A failure found before any request is sent.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum PlatformPreflightError {
    /// The arguments or cursor cannot run; exit 2.
    Argument(String),
    /// The configuration leaves the operation without a route; exit 3.
    Config(String),
}

/// The routes a search page runs, in order, and the routes skipped before it runs.
pub(crate) struct SearchPlan {
    platform: Platform,
    request: PlatformSearchRequest,
    routes: Vec<PlatformRouteConfig>,
    skipped: Vec<ProviderAttempt>,
}

/// Plans a first-page search over the configured routes that can apply the request options.
pub(crate) fn plan_search(
    config: &PlatformRuntimeConfig,
    request: PlatformSearchRequest,
) -> Result<SearchPlan, PlatformPreflightError> {
    plan(config, request, None)
}

/// Plans the search page a cursor names; only the route that produced the cursor may run it.
pub(crate) fn plan_cursor_search(
    config: &PlatformRuntimeConfig,
    cursor: &str,
) -> Result<SearchPlan, PlatformPreflightError> {
    let (route, request) = page_cursor::decode(
        cursor,
        config.platform(),
        PlatformOperation::Search.as_str(),
    )
    .map_err(PlatformPreflightError::Argument)?;
    plan(config, request, Some(route))
}

fn plan(
    config: &PlatformRuntimeConfig,
    request: PlatformSearchRequest,
    pinned: Option<ProviderId>,
) -> Result<SearchPlan, PlatformPreflightError> {
    let platform = config.platform();
    let plan = plan_routes(
        PlatformOperation::Search.as_str(),
        platform,
        &config.order_key(),
        catalog::platform(platform).routes(PlatformOperation::Search),
        &route_candidates(config),
        pinned,
        |id| providers::platform_search_support(id, &request),
    )?;
    Ok(SearchPlan {
        platform,
        request,
        routes: plan.routes.into_iter().map(|(_, config)| config).collect(),
        skipped: plan.skipped,
    })
}

/// Runs a planned search page and encodes the next-page cursor.
pub(crate) async fn run_search(
    plan: SearchPlan,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    verbose: bool,
) -> Result<PlatformSearchPage, ProviderError> {
    let SearchPlan {
        platform,
        request,
        routes,
        skipped,
    } = plan;
    let request_ref = &request;
    let steps = routes
        .into_iter()
        .map(|context| ChainStep {
            context,
            configured: true,
        })
        .collect();
    let result = chain::run_chain(
        steps,
        ChainSettings {
            target: operation_target(platform, PlatformOperation::Search.as_str()),
            budget_policy: BudgetPolicy::SlicedEven {
                skipped_message: "skipped to preserve fallback deadline budget",
            },
            fallback_off: false,
            diagnostic_merge: DiagnosticMerge::Join,
            terminal: TerminalPolicy::ChainWide {
                verbose,
                exhausted_message: "platform search deadline elapsed",
            },
            identity: &route_identity,
        },
        deadline,
        |config: PlatformRouteConfig, step_deadline| {
            let client = client.clone();
            async move {
                let route = config.route();
                match providers::build_platform_search(config, client, retry_policy, step_deadline)
                    .search(request_ref)
                    .await
                {
                    Ok(outcome) => {
                        let empty = outcome.items.is_empty();
                        let success = StepSuccess {
                            value: (route, outcome.items, outcome.next_page),
                            attempts: outcome.attempts,
                            diagnostic: outcome.diagnostic,
                        };
                        if empty {
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
    .await;
    let outcome = LeadingTrace::new(skipped).lead(result)?;
    let (route, items, next_page) = outcome.value;
    Ok(PlatformSearchPage {
        platform,
        provider: route.name(),
        collection: None,
        items,
        next_cursor: next_page.map(|page| page_cursor::encode(route, request.at_page(page))),
        attempts: if verbose {
            outcome.attempts
        } else {
            Vec::new()
        },
        diagnostic: outcome.diagnostic,
    })
}

pub(crate) fn route_identity(config: &PlatformRouteConfig) -> StepIdentity {
    StepIdentity {
        provider: config.route().name(),
        model: None,
        endpoint_host: None,
    }
}

pub(crate) fn operation_target(platform: Platform, operation: &'static str) -> AttemptTarget {
    AttemptTarget::platform(platform.as_str(), operation)
}

/// One configured route in platform order.
pub(crate) struct RouteCandidate<'a, C> {
    id: ProviderId,
    config: &'a C,
    configured: bool,
}

pub(crate) struct RoutePlan<C> {
    pub(crate) routes: Vec<(ProviderId, C)>,
    pub(crate) skipped: Vec<ProviderAttempt>,
}

pub(crate) fn route_candidates(
    config: &PlatformRuntimeConfig,
) -> Vec<RouteCandidate<'_, PlatformRouteConfig>> {
    config
        .entries()
        .iter()
        .map(|entry| RouteCandidate {
            id: entry.id(),
            config: entry.config(),
            configured: entry.configured(),
        })
        .collect()
}

/// Selects the routes that run an operation: configured order ∩ the operation's catalog routes
/// ∩ configured routes, minus the routes that cannot run the request. `support` checks the
/// request and must not send requests. A `pinned` route (from a cursor) is the only route that
/// may run.
pub(crate) fn plan_routes<C: Clone>(
    operation: &'static str,
    platform: Platform,
    order_key: &str,
    operation_routes: &[ProviderId],
    candidates: &[RouteCandidate<'_, C>],
    pinned: Option<ProviderId>,
    support: impl Fn(ProviderId) -> Option<Result<(), String>>,
) -> Result<RoutePlan<C>, PlatformPreflightError> {
    let available = candidates
        .iter()
        .filter(|candidate| candidate.configured && operation_routes.contains(&candidate.id))
        .collect::<Vec<_>>();
    let check = |id: ProviderId| {
        support(id).unwrap_or_else(|| {
            Err(format!(
                "{} has no {platform} {operation} adapter",
                id.name()
            ))
        })
    };
    if let Some(route) = pinned {
        let candidate = available
            .iter()
            .find(|candidate| candidate.id == route)
            .ok_or_else(|| {
                PlatformPreflightError::Argument(format!(
                    "the cursor route `{}` is no longer available for {platform} {operation}; search again without --cursor",
                    route.name()
                ))
            })?;
        check(candidate.id).map_err(|reason| {
            PlatformPreflightError::Argument(format!(
                "the cursor cannot run: {reason}; search again without --cursor"
            ))
        })?;
        return Ok(RoutePlan {
            routes: vec![(candidate.id, candidate.config.clone())],
            skipped: Vec::new(),
        });
    }
    if available.is_empty() {
        // Configuration only fails for a route that requires credentials and has none.
        let missing_keys = candidates
            .iter()
            .filter(|candidate| !candidate.configured && operation_routes.contains(&candidate.id))
            .map(|candidate| config::provider_keys_key(candidate.id.name()))
            .collect::<Vec<_>>();
        let opt_in = opt_in_hint(order_key, operation_routes, candidates, |_| true);
        let hint = if missing_keys.is_empty() {
            opt_in
        } else {
            format!("; set {}{opt_in}", missing_keys.join(" or "))
        };
        return Err(PlatformPreflightError::Config(format!(
            "{order_key} has no configured route for {platform} {operation}{hint}"
        )));
    }
    let mut plan = RoutePlan {
        routes: Vec::new(),
        skipped: Vec::new(),
    };
    let mut reasons = Vec::new();
    for candidate in &available {
        match check(candidate.id) {
            Ok(()) => plan.routes.push((candidate.id, candidate.config.clone())),
            Err(reason) => {
                plan.skipped.push(crate::chain::skipped_attempt(
                    operation_target(platform, operation),
                    candidate.id.name(),
                    &reason,
                ));
                reasons.push(reason);
            }
        }
    }
    if plan.routes.is_empty() {
        let configured = available
            .iter()
            .map(|candidate| candidate.id.name())
            .collect::<Vec<_>>()
            .join(", ");
        let opt_in = opt_in_hint(order_key, operation_routes, candidates, |id| {
            check(id).is_ok()
        });
        return Err(PlatformPreflightError::Argument(format!(
            "no configured {platform} route supports the requested options (configured routes: {configured}): {}{opt_in}",
            reasons.join("; ")
        )));
    }
    Ok(plan)
}

/// Says how to enable the operation's routes that the order leaves out, that only users may
/// enable, and that `relevant` keeps: a browser route acts on the user's own account, and an
/// HTTP route may read a site's undocumented endpoints (ADR-0028).
fn opt_in_hint<C>(
    order_key: &str,
    operation_routes: &[ProviderId],
    candidates: &[RouteCandidate<'_, C>],
    relevant: impl Fn(ProviderId) -> bool,
) -> String {
    let order = candidates
        .iter()
        .map(|candidate| candidate.id)
        .collect::<Vec<_>>();
    operation_routes
        .iter()
        .filter(|route| !order.contains(route) && relevant(**route))
        .filter_map(|route| match catalog::registration(*route).transport {
            ProviderTransport::Browser(_) => {
                let login = format!(
                    ", run `forager browser login {platform}` and log in",
                    platform = catalog::route_platform(*route)
                );
                let risk = catalog::opt_in_risk(*route)
                    .map(|risk| format!(". {risk}"))
                    .unwrap_or_default();
                Some(format!(
                    "; to use {route}, run `forager browser install`{login}, then add `{route}` to {order_key}{risk}",
                    route = route.name(),
                ))
            }
            ProviderTransport::Http => {
                catalog::config_set_hint(*route, order_key, &order).map(|text| format!("; {text}"))
            }
        })
        .collect()
}

/// Fails a platform command before its route chain when a browser route in the order cannot
/// run: the browser cannot be found, or its Browser Profile never had the login it needs. The
/// check launches nothing; it is a Config failure, not an attempt.
///
/// # Errors
///
/// Returns the Config message, with the command that fixes the first failing route.
pub(crate) fn browser_preflight(
    config: &PlatformRuntimeConfig,
    browser: &BrowserRuntimeConfig,
) -> Result<(), String> {
    for entry in config.entries() {
        if let ProviderTransport::Browser(_) = catalog::registration(entry.id()).transport {
            providers::browser_preflight(entry.id(), browser)
                .map_err(|reason| format!("{} cannot run: {reason}", entry.id().name()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "platform_chain_tests.rs"]
mod tests;
