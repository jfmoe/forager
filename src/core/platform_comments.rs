//! Comments: the route chain of the single-route `comments` operation, dispatched by platform.
//!
//! Planning follows every platform operation: order ∩ the operation's route set ∩ configured
//! routes, minus the routes that cannot run here. Each platform's route set lives in the
//! provider factory because one route implements the operation. The route judges an empty
//! comment list itself, so every route success is accepted as is.

use reqwest::Client;

use crate::catalog::ProviderId;
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
    COMMENTS, Deadline, Platform, PlatformCommentsPage, PlatformCommentsRequest, PlatformRef,
    ProviderAttempt, ProviderError, XiaohongshuCommentsPage, YoutubeCommentsPage,
};

/// The routes a comments read runs, in order, and the routes skipped before it runs.
pub(crate) struct CommentsPlan {
    request: PlatformCommentsRequest,
    routes: Vec<PlatformRouteConfig>,
    skipped: Vec<ProviderAttempt>,
}

type SupportCheck = fn(ProviderId) -> Option<Result<(), String>>;

/// Plans a comments read; `config` is the request platform's.
pub(crate) fn plan_comments(
    config: &PlatformRuntimeConfig,
    request: PlatformCommentsRequest,
) -> Result<CommentsPlan, PlatformPreflightError> {
    let (routes, support): (&[ProviderId], SupportCheck) = match &request {
        PlatformCommentsRequest::Xiaohongshu(_) => (
            providers::XIAOHONGSHU_COMMENTS_ROUTES,
            providers::xiaohongshu_comments_support,
        ),
        PlatformCommentsRequest::Youtube(_) => (
            providers::YOUTUBE_COMMENTS_ROUTES,
            providers::youtube_comments_support,
        ),
    };
    let plan = plan_routes(
        COMMENTS,
        config.platform(),
        &config.order_key(),
        routes,
        &route_candidates(config),
        None,
        support,
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
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    verbose: bool,
) -> Result<PlatformCommentsPage, ProviderError> {
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
        })
        .collect();
    let result = chain::run_chain(
        steps,
        ChainSettings {
            target: operation_target(request.platform(), COMMENTS),
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
        },
        deadline,
        |config: PlatformRouteConfig, step_deadline| {
            let client = client.clone();
            async move {
                match read_comments(config, client, retry_policy, step_deadline, request).await {
                    Ok((page, attempts, diagnostic)) => StepVerdict::Accepted(StepSuccess {
                        value: page,
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
            let mut page = outcome.value;
            skipped.extend(outcome.attempts);
            page.attach(
                if verbose { skipped } else { Vec::new() },
                outcome.diagnostic,
            );
            Ok(page)
        }
        Err(mut error) => {
            skipped.append(&mut error.attempts);
            error.attempts = skipped;
            Err(error)
        }
    }
}

/// Reads the comments on one route: the page without its trace, then the route's attempts and
/// diagnostic.
async fn read_comments(
    config: PlatformRouteConfig,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    request: &PlatformCommentsRequest,
) -> Result<(PlatformCommentsPage, Vec<ProviderAttempt>, Option<String>), ProviderError> {
    let provider = config.route().name();
    match request {
        PlatformCommentsRequest::Xiaohongshu(request) => {
            let outcome = providers::xiaohongshu_comments(config, deadline, request).await?;
            let page = XiaohongshuCommentsPage {
                platform: Platform::Xiaohongshu,
                provider,
                note: PlatformRef::Xiaohongshu(request.note.clone()),
                comments: outcome.comments,
                has_more: outcome.has_more,
                attempts: Vec::new(),
                diagnostic: None,
            };
            Ok((
                PlatformCommentsPage::Xiaohongshu(page),
                outcome.attempts,
                outcome.diagnostic,
            ))
        }
        PlatformCommentsRequest::Youtube(request) => {
            let outcome =
                providers::youtube_comments(config, client, retry_policy, deadline, request)
                    .await?;
            let page = YoutubeCommentsPage {
                platform: Platform::Youtube,
                provider,
                video: PlatformRef::Youtube(request.video.clone()),
                comments: outcome.comments,
                has_more: outcome.has_more,
                attempts: Vec::new(),
                diagnostic: None,
            };
            Ok((
                PlatformCommentsPage::Youtube(page),
                outcome.attempts,
                outcome.diagnostic,
            ))
        }
    }
}
