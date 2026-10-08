//! Xiaohongshu comments: the route chain of the single-route `comments` operation.
//!
//! Planning follows every platform operation: order ∩ the operation's route set ∩ configured
//! routes, minus the routes that cannot run here. The route set lives in the provider factory
//! because one route implements the operation. The route judges an empty comment list itself,
//! so every route success is accepted as is.

use crate::chain::{
    self, BudgetPolicy, ChainSettings, ChainStep, DiagnosticMerge, StepSuccess, StepVerdict,
    TerminalPolicy,
};
use crate::config::{PlatformRouteConfig, PlatformRuntimeConfig};
use crate::platform_chain::{
    PlatformPreflightError, operation_target, plan_routes, route_candidates, route_identity,
};
use crate::providers;
use crate::types::{
    COMMENTS, Deadline, Platform, PlatformRef, ProviderAttempt, ProviderError,
    XiaohongshuCommentsPage, XiaohongshuCommentsRequest,
};

/// The routes a comments read runs, in order, and the routes skipped before it runs.
pub(crate) struct CommentsPlan {
    request: XiaohongshuCommentsRequest,
    routes: Vec<PlatformRouteConfig>,
    skipped: Vec<ProviderAttempt>,
}

/// Plans a comments read; `config` is Xiaohongshu's.
pub(crate) fn plan_comments(
    config: &PlatformRuntimeConfig,
    request: XiaohongshuCommentsRequest,
) -> Result<CommentsPlan, PlatformPreflightError> {
    let plan = plan_routes(
        COMMENTS,
        config.platform(),
        &config.order_key(),
        providers::XIAOHONGSHU_COMMENTS_ROUTES,
        &route_candidates(config),
        None,
        providers::xiaohongshu_comments_support,
    )?;
    Ok(CommentsPlan {
        request,
        routes: plan.routes.into_iter().map(|(_, config)| config).collect(),
        skipped: plan.skipped,
    })
}

/// Runs a planned comments read.
pub(crate) async fn run_comments(
    plan: CommentsPlan,
    deadline: Deadline,
    verbose: bool,
) -> Result<XiaohongshuCommentsPage, ProviderError> {
    let CommentsPlan {
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
            target: operation_target(Platform::Xiaohongshu, COMMENTS),
            budget_policy: BudgetPolicy::SlicedEven {
                skipped_message: "skipped to preserve fallback deadline budget",
            },
            fallback_off: false,
            diagnostic_merge: DiagnosticMerge::Join,
            terminal: TerminalPolicy::ChainWide {
                verbose,
                exhausted_message: "platform comments deadline elapsed",
            },
            identity: &route_identity,
            continue_on_failure: &chain::always_continue,
        },
        deadline,
        |config: PlatformRouteConfig, step_deadline| async move {
            let route = config.route();
            match providers::xiaohongshu_comments(config, step_deadline, request).await {
                Ok(outcome) => StepVerdict::Accepted(StepSuccess {
                    value: (route, outcome.comments, outcome.has_more),
                    attempts: outcome.attempts,
                    diagnostic: outcome.diagnostic,
                }),
                Err(error) => StepVerdict::Failed(error),
            }
        },
    )
    .await;
    match result {
        Ok(outcome) => {
            let (route, comments, has_more) = outcome.value;
            skipped.extend(outcome.attempts);
            Ok(XiaohongshuCommentsPage {
                platform: Platform::Xiaohongshu,
                provider: route.name(),
                note: PlatformRef::Xiaohongshu(request.note.clone()),
                comments,
                has_more,
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
