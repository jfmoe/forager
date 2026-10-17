//! X ask: the route chain of the single-route `ask` operation.
//!
//! Planning follows every platform operation: order ∩ the operation's route set ∩ configured
//! routes. The route set lives in the provider factory because one route implements the
//! operation. An answer is never empty, so every route success is accepted as is.

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
    ASK, Deadline, Platform, ProviderAttempt, ProviderError, XAskRequest, XAskResult,
};

/// The routes an ask runs, in order, and the routes skipped before it runs.
pub(crate) struct AskPlan {
    request: XAskRequest,
    routes: Vec<PlatformRouteConfig>,
    skipped: Vec<ProviderAttempt>,
}

/// Plans an ask; `config` is X's.
pub(crate) fn plan_ask(
    config: &PlatformRuntimeConfig,
    request: XAskRequest,
) -> Result<AskPlan, PlatformPreflightError> {
    let plan = plan_routes(
        ASK,
        config.platform(),
        &config.order_key(),
        providers::X_ASK_ROUTES,
        &route_candidates(config),
        None,
        providers::x_ask_support,
    )?;
    Ok(AskPlan {
        request,
        routes: plan.routes.into_iter().map(|(_, config)| config).collect(),
        skipped: plan.skipped,
    })
}

/// Runs a planned ask.
pub(crate) async fn run_ask(
    plan: AskPlan,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    verbose: bool,
) -> Result<XAskResult, ProviderError> {
    let AskPlan {
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
            gate_attempt: None,
        })
        .collect();
    let result = chain::run_chain(
        steps,
        ChainSettings {
            target: operation_target(Platform::X, ASK),
            budget_policy: BudgetPolicy::SlicedEven {
                skipped_message: "skipped to preserve fallback deadline budget",
            },
            fallback_off: false,
            diagnostic_merge: DiagnosticMerge::Join,
            terminal: TerminalPolicy::ChainWide {
                verbose,
                exhausted_message: "platform ask deadline elapsed",
            },
            identity: &route_identity,
            continue_on_failure: &chain::always_continue,
        },
        deadline,
        |config: PlatformRouteConfig, step_deadline| {
            let client = client.clone();
            async move {
                let route = config.route();
                match providers::x_ask(config, client, retry_policy, step_deadline, request).await {
                    Ok(outcome) => StepVerdict::Accepted(StepSuccess {
                        value: (route, outcome.answer, outcome.sources),
                        attempts: outcome.attempts,
                        diagnostic: outcome.diagnostic,
                    }),
                    Err(error) => StepVerdict::Failed(error),
                }
            }
        },
    )
    .await;
    match result {
        Ok(outcome) => {
            let (route, answer, sources) = outcome.value;
            skipped.extend(outcome.attempts);
            Ok(XAskResult {
                platform: Platform::X,
                provider: route.name(),
                query: request.query.clone(),
                answer,
                sources,
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
