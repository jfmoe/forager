//! Ask: the route chain of the single-route `ask` operation, dispatched by platform.
//!
//! Planning follows every platform operation: order ∩ the operation's route set ∩ configured
//! routes. Each platform's route set lives in the provider factory because one route implements
//! the operation. A YouTube answer comes from a model that watches the video, so the video is
//! read first by the precheck; X needs none. An answer is never empty, so every route success
//! is accepted as is.

use reqwest::Client;

use crate::catalog::ProviderId;
use crate::chain::{
    self, BudgetPolicy, ChainSettings, ChainStep, DiagnosticMerge, StepSuccess, StepVerdict,
    TerminalPolicy,
};
use crate::config::{PlatformRouteConfig, PlatformRuntimeConfig};
use crate::net::{RetryPolicy, combine_diagnostics};
use crate::platform_chain::{
    PlatformPreflightError, operation_target, plan_routes, route_candidates, route_identity,
};
use crate::platform_precheck::{PrecheckDepth, PrecheckPlan, plan_precheck, run_precheck};
use crate::providers;
use crate::types::{
    ASK, AskedVideo, Deadline, ModelUsage, Platform, PlatformAskRequest, PlatformAskResult,
    PlatformItem, PlatformRef, ProviderAttempt, ProviderError, XAskResult, XSource,
    YoutubeAskResult,
};

/// The routes an ask runs, in order, the routes skipped before it runs, and the precheck of a
/// platform whose answer reads one item.
pub(crate) struct AskPlan {
    request: PlatformAskRequest,
    precheck: Option<PrecheckPlan>,
    routes: Vec<PlatformRouteConfig>,
    skipped: Vec<ProviderAttempt>,
}

type SupportCheck = fn(ProviderId) -> Option<Result<(), String>>;

/// Plans an ask; `config` is the request platform's.
pub(crate) fn plan_ask(
    config: &PlatformRuntimeConfig,
    request: PlatformAskRequest,
) -> Result<AskPlan, PlatformPreflightError> {
    let (routes, support): (&[ProviderId], SupportCheck) = match &request {
        PlatformAskRequest::X(_) => (providers::X_ASK_ROUTES, providers::x_ask_support),
        PlatformAskRequest::Youtube(_) => (
            providers::YOUTUBE_ASK_ROUTES,
            providers::youtube_ask_support,
        ),
    };
    let plan = plan_routes(
        ASK,
        config.platform(),
        &config.order_key(),
        routes,
        &route_candidates(config),
        None,
        support,
    )?;
    let precheck = match &request {
        PlatformAskRequest::X(_) => None,
        PlatformAskRequest::Youtube(request) => Some(plan_precheck(
            config,
            PlatformRef::Youtube(request.video.clone()),
            PrecheckDepth::Metadata,
        )?),
    };
    Ok(AskPlan {
        request,
        precheck,
        routes: plan.routes.into_iter().map(|(_, config)| config).collect(),
        skipped: plan.skipped,
    })
}

/// One route's answer, in the platform's own shape.
enum RouteAnswer {
    X {
        answer: String,
        sources: Vec<XSource>,
    },
    Youtube {
        answer: String,
        model: Option<String>,
        usage: Option<ModelUsage>,
    },
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
        request,
        precheck,
        routes,
        skipped,
    } = plan;
    let (video, mut attempts, diagnostic) = match precheck {
        Some(precheck) => {
            let checked = run_precheck(
                precheck,
                skipped,
                client.clone(),
                retry_policy,
                deadline,
                verbose,
            )
            .await?;
            (Some(checked.item), checked.attempts, checked.diagnostic)
        }
        None => (None, skipped, None),
    };
    let (request, video) = (&request, video.as_ref());
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
            target: operation_target(request.platform(), ASK),
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
                match answer(config, client, retry_policy, step_deadline, request, video).await {
                    Ok((answer, attempts, diagnostic)) => StepVerdict::Accepted(StepSuccess {
                        value: (route, answer),
                        attempts,
                        diagnostic,
                    }),
                    Err(error) => StepVerdict::Failed(error),
                }
            }
        },
    )
    .await;
    match result {
        Ok(outcome) => {
            let (route, answer) = outcome.value;
            attempts.extend(outcome.attempts);
            let attempts = if verbose { attempts } else { Vec::new() };
            let diagnostic =
                combine_diagnostics([diagnostic, outcome.diagnostic].into_iter().flatten());
            Ok(asked(request, video, route, answer, attempts, diagnostic))
        }
        Err(mut error) => {
            attempts.append(&mut error.attempts);
            error.attempts = attempts;
            error.diagnostic =
                combine_diagnostics([diagnostic, error.diagnostic].into_iter().flatten());
            Err(error)
        }
    }
}

/// The result of an ask in its platform's shape. `video` is the item the precheck read, for a
/// platform that has one.
fn asked(
    request: &PlatformAskRequest,
    video: Option<&PlatformItem>,
    route: ProviderId,
    answer: RouteAnswer,
    attempts: Vec<ProviderAttempt>,
    diagnostic: Option<String>,
) -> PlatformAskResult {
    match (request, answer) {
        (PlatformAskRequest::X(request), RouteAnswer::X { answer, sources }) => {
            PlatformAskResult::X(XAskResult {
                platform: Platform::X,
                provider: route.name(),
                query: request.query.clone(),
                answer,
                sources,
                attempts,
                diagnostic,
            })
        }
        (
            PlatformAskRequest::Youtube(request),
            RouteAnswer::Youtube {
                answer,
                model,
                usage,
            },
        ) => {
            let video = video.expect("a YouTube ask runs after its precheck");
            PlatformAskResult::Youtube(YoutubeAskResult {
                platform: Platform::Youtube,
                provider: route.name(),
                video: AskedVideo {
                    reference: video.reference.clone(),
                    url: video.url.clone(),
                    title: video.title.clone(),
                },
                question: request.question.clone(),
                answer,
                model,
                usage,
                attempts,
                diagnostic,
            })
        }
        _ => unreachable!("every route answers in its request's platform shape"),
    }
}

/// Answers on one route: the answer without its trace, then the route's attempts and
/// diagnostic. `video` is the item the precheck read, for a platform that has one.
async fn answer(
    config: PlatformRouteConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    request: &PlatformAskRequest,
    video: Option<&PlatformItem>,
) -> Result<(RouteAnswer, Vec<ProviderAttempt>, Option<String>), ProviderError> {
    match request {
        PlatformAskRequest::X(request) => {
            let outcome = providers::x_ask(config, client, retry_policy, deadline, request).await?;
            Ok((
                RouteAnswer::X {
                    answer: outcome.answer,
                    sources: outcome.sources,
                },
                outcome.attempts,
                outcome.diagnostic,
            ))
        }
        PlatformAskRequest::Youtube(request) => {
            let video = video.expect("a YouTube ask runs after its precheck");
            let outcome = providers::youtube_ask(
                config,
                client,
                retry_policy,
                deadline,
                video,
                &request.question,
            )
            .await?;
            Ok((
                RouteAnswer::Youtube {
                    answer: outcome.answer,
                    model: outcome.model,
                    usage: outcome.usage,
                },
                outcome.attempts,
                outcome.diagnostic,
            ))
        }
    }
}
