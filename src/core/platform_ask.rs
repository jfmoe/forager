//! Ask: the route chain of the single-route `ask` operation, dispatched by platform.
//!
//! Planning follows every platform operation: order ∩ the operation's route set ∩ configured
//! routes. Each platform's route set lives in the provider factory because one route implements
//! the operation. A YouTube answer comes from a model that watches the video, so the video is
//! read first by the precheck; X needs none. An answer is never empty, so every route success
//! is accepted as is.

use std::future::Future;

use reqwest::Client;

use crate::catalog::ProviderId;
use crate::chain::{
    self, BudgetPolicy, ChainOutcome, ChainSettings, ChainStep, DiagnosticMerge, StepSuccess,
    StepVerdict, TerminalPolicy,
};
use crate::config::{PlatformRouteConfig, PlatformRuntimeConfig};
use crate::net::RetryPolicy;
use crate::platform_chain::{
    PlatformPreflightError, operation_target, plan_routes, route_candidates, route_identity,
};
use crate::platform_precheck::{
    PrecheckDepth, PrecheckPlan, Prechecked, plan_precheck, run_precheck,
};
use crate::providers;
use crate::types::{
    ASK, AskedVideo, Deadline, Platform, PlatformAskRequest, PlatformAskResult, ProviderAttempt,
    ProviderError, XAskRequest, XAskResult, YoutubeAskRequest, YoutubeAskResult,
};

/// The routes an ask runs, in order, the routes skipped before it runs, and what the ask needs
/// on its platform.
pub(crate) struct AskPlan {
    ask: PlannedAsk,
    routes: Vec<PlatformRouteConfig>,
    skipped: Vec<ProviderAttempt>,
}

/// An ask on its platform: an X ask needs nothing before its routes; a YouTube ask is about one
/// video, which its precheck reads first.
enum PlannedAsk {
    X(XAskRequest),
    Youtube {
        request: YoutubeAskRequest,
        precheck: PrecheckPlan,
    },
}

/// Plans an ask; `config` is the request platform's.
pub(crate) fn plan_ask(
    config: &PlatformRuntimeConfig,
    request: PlatformAskRequest,
) -> Result<AskPlan, PlatformPreflightError> {
    let platform_routes = |routes, support: fn(ProviderId) -> Option<Result<(), String>>| {
        plan_routes(
            ASK,
            config.platform(),
            &config.order_key(),
            routes,
            &route_candidates(config),
            None,
            support,
        )
    };
    let (ask, plan) = match request {
        PlatformAskRequest::X(request) => {
            let plan = platform_routes(providers::X_ASK_ROUTES, providers::x_ask_support)?;
            (PlannedAsk::X(request), plan)
        }
        PlatformAskRequest::Youtube(request) => {
            let plan = platform_routes(
                providers::YOUTUBE_ASK_ROUTES,
                providers::youtube_ask_support,
            )?;
            let precheck = plan_precheck(config, request.video.clone(), PrecheckDepth::Metadata)?;
            (PlannedAsk::Youtube { request, precheck }, plan)
        }
    };
    Ok(AskPlan {
        ask,
        routes: plan.routes.into_iter().map(|(_, config)| config).collect(),
        skipped: plan.skipped,
    })
}

/// Runs a planned ask: the precheck, when the platform has one, then the route chain.
pub(crate) async fn run_ask(
    plan: AskPlan,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    verbose: bool,
) -> Result<PlatformAskResult, ProviderError> {
    let AskPlan {
        ask,
        routes,
        skipped,
    } = plan;
    let run = AskRun {
        client,
        retry_policy,
        deadline,
        verbose,
    };
    match ask {
        PlannedAsk::X(request) => ask_x(&request, routes, skipped, &run).await,
        PlannedAsk::Youtube { request, precheck } => {
            ask_youtube(request, precheck, routes, skipped, &run).await
        }
    }
}

/// What every step of an ask runs with.
struct AskRun {
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    verbose: bool,
}

async fn ask_x(
    request: &XAskRequest,
    routes: Vec<PlatformRouteConfig>,
    mut skipped: Vec<ProviderAttempt>,
    run: &AskRun,
) -> Result<PlatformAskResult, ProviderError> {
    let result = run_routes(Platform::X, routes, run, |config, deadline| {
        let client = run.client.clone();
        async move {
            let outcome =
                providers::x_ask(config, client, run.retry_policy, deadline, request).await?;
            Ok(StepSuccess {
                value: (outcome.answer, outcome.sources),
                attempts: outcome.attempts,
                diagnostic: outcome.diagnostic,
            })
        }
    })
    .await;
    let outcome = match result {
        Ok(mut outcome) => {
            skipped.append(&mut outcome.attempts);
            outcome.attempts = skipped;
            outcome
        }
        Err(mut error) => {
            skipped.append(&mut error.attempts);
            error.attempts = skipped;
            return Err(error);
        }
    };
    let (route, (answer, sources)) = outcome.value;
    Ok(PlatformAskResult::X(XAskResult {
        platform: Platform::X,
        provider: route.name(),
        query: request.query.clone(),
        answer,
        sources,
        attempts: if run.verbose {
            outcome.attempts
        } else {
            Vec::new()
        },
        diagnostic: outcome.diagnostic,
    }))
}

async fn ask_youtube(
    request: YoutubeAskRequest,
    precheck: PrecheckPlan,
    routes: Vec<PlatformRouteConfig>,
    skipped: Vec<ProviderAttempt>,
    run: &AskRun,
) -> Result<PlatformAskResult, ProviderError> {
    let Prechecked { video, trace } = run_precheck(
        precheck,
        skipped,
        run.client.clone(),
        run.retry_policy,
        run.deadline,
        run.verbose,
    )
    .await?;
    let (question, checked) = (&request.question, &video);
    let result = run_routes(Platform::Youtube, routes, run, |config, deadline| {
        let client = run.client.clone();
        async move {
            let outcome = providers::youtube_ask(
                config,
                client,
                run.retry_policy,
                deadline,
                checked,
                question,
            )
            .await?;
            Ok(StepSuccess {
                value: (outcome.answer, outcome.model, outcome.usage),
                attempts: outcome.attempts,
                diagnostic: outcome.diagnostic,
            })
        }
    })
    .await;
    let outcome = trace.lead(result)?;
    let (route, (answer, model, usage)) = outcome.value;
    Ok(PlatformAskResult::Youtube(YoutubeAskResult {
        platform: Platform::Youtube,
        provider: route.name(),
        video: AskedVideo {
            reference: video.reference,
            url: video.url,
            title: video.title,
        },
        question: request.question,
        answer,
        model,
        usage,
        attempts: if run.verbose {
            outcome.attempts
        } else {
            Vec::new()
        },
        diagnostic: outcome.diagnostic,
    }))
}

/// Runs the ask's route chain; `answer` answers on one route, without the route's identity,
/// which the chain's value pairs with it.
async fn run_routes<T, Answer, Fut>(
    platform: Platform,
    routes: Vec<PlatformRouteConfig>,
    run: &AskRun,
    answer: Answer,
) -> Result<ChainOutcome<(ProviderId, T)>, ProviderError>
where
    Answer: Fn(PlatformRouteConfig, Deadline) -> Fut,
    Fut: Future<Output = Result<StepSuccess<T>, ProviderError>>,
{
    let steps = routes
        .into_iter()
        .map(|context| ChainStep {
            context,
            configured: true,
            gate_attempt: None,
        })
        .collect();
    chain::run_chain(
        steps,
        ChainSettings {
            target: operation_target(platform, ASK),
            budget_policy: BudgetPolicy::SlicedEven {
                skipped_message: "skipped to preserve fallback deadline budget",
            },
            fallback_off: false,
            diagnostic_merge: DiagnosticMerge::Join,
            terminal: TerminalPolicy::ChainWide {
                verbose: run.verbose,
                exhausted_message: "platform ask deadline elapsed",
            },
            identity: &route_identity,
            continue_on_failure: &chain::always_continue,
        },
        run.deadline,
        |config: PlatformRouteConfig, step_deadline| {
            let route = config.route();
            let answered = answer(config, step_deadline);
            async move {
                match answered.await {
                    Ok(success) => StepVerdict::Accepted(StepSuccess {
                        value: (route, success.value),
                        attempts: success.attempts,
                        diagnostic: success.diagnostic,
                    }),
                    Err(error) => StepVerdict::Failed(error),
                }
            }
        },
    )
    .await
}
