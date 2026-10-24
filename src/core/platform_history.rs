//! Polymarket Price History: the route chain of the single-route `history` operation.
//!
//! Planning follows every platform operation: order ∩ the operation's route set ∩ configured
//! routes. The route set lives in the provider factory because one route implements the
//! operation. An empty series is a legitimate result, so every route success is accepted as is.

use reqwest::Client;

use crate::chain::{
    self, BudgetPolicy, ChainSettings, ChainStep, DiagnosticMerge, StepSuccess, StepVerdict,
    TerminalPolicy,
};
use crate::config::{PlatformRouteConfig, PlatformRuntimeConfig};
use crate::net::RetryPolicy;
use crate::platform_chain::{
    PlatformPreflightError, operation_target, plan_routes, route_candidates, route_identity,
};
use crate::providers;
use crate::types::{
    Deadline, HISTORY, Platform, PolymarketHistoryRequest, PolymarketHistoryResult,
    PolymarketWindow, ProviderAttempt, ProviderError,
};

/// The routes a price history runs, in order, and the routes skipped before it runs.
pub(crate) struct HistoryPlan {
    request: PolymarketHistoryRequest,
    routes: Vec<PlatformRouteConfig>,
    skipped: Vec<ProviderAttempt>,
}

/// Plans a price history; `config` is Polymarket's.
pub(crate) fn plan_history(
    config: &PlatformRuntimeConfig,
    request: PolymarketHistoryRequest,
) -> Result<HistoryPlan, PlatformPreflightError> {
    let plan = plan_routes(
        HISTORY,
        config.platform(),
        &config.order_key(),
        providers::POLYMARKET_HISTORY_ROUTES,
        &route_candidates(config),
        None,
        providers::polymarket_history_support,
    )?;
    Ok(HistoryPlan {
        request,
        routes: plan.routes.into_iter().map(|(_, config)| config).collect(),
        skipped: plan.skipped,
    })
}

/// Runs a planned price history.
pub(crate) async fn run_history(
    plan: HistoryPlan,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    verbose: bool,
) -> Result<PolymarketHistoryResult, ProviderError> {
    let HistoryPlan {
        request,
        routes,
        mut skipped,
    } = plan;
    let request = &request;
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
            target: operation_target(Platform::Polymarket, HISTORY),
            budget_policy: BudgetPolicy::SlicedEven {
                skipped_message: "skipped to preserve fallback deadline budget",
            },
            fallback_off: false,
            diagnostic_merge: DiagnosticMerge::Join,
            terminal: TerminalPolicy::ChainWide {
                verbose,
                exhausted_message: "platform history deadline elapsed",
            },
            identity: &route_identity,
        },
        deadline,
        |config: PlatformRouteConfig, step_deadline| {
            let client = client.clone();
            async move {
                let route = config.route();
                match providers::polymarket_history(
                    config,
                    client,
                    retry_policy,
                    step_deadline,
                    request,
                )
                .await
                {
                    Ok(mut outcome) => StepVerdict::Accepted(StepSuccess {
                        attempts: std::mem::take(&mut outcome.attempts),
                        diagnostic: outcome.diagnostic.take(),
                        value: (route, outcome),
                    }),
                    Err(error) => StepVerdict::Failed(error),
                }
            }
        },
    )
    .await;
    match result {
        Ok(outcome) => {
            let (route, history) = outcome.value;
            skipped.extend(outcome.attempts);
            let (interval, from, to) = match request.window {
                PolymarketWindow::Interval(interval) => (Some(interval), None, None),
                PolymarketWindow::Range { from, to } => (None, Some(from), Some(to)),
            };
            Ok(PolymarketHistoryResult {
                platform: Platform::Polymarket,
                provider: route.name(),
                market: history.market,
                outcome: history.outcome,
                interval,
                from,
                to,
                bucket_seconds: request.bucket.seconds(),
                points: history.points,
                settlement: history.settlement,
                attempts: if verbose { skipped } else { Vec::new() },
                diagnostic: outcome.diagnostic,
            })
        }
        Err(mut error) => {
            skipped.append(&mut error.attempts);
            error.attempts = skipped;
            Err(error)
        }
    }
}
