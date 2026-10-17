//! Transcribe: the precheck and route chain of the single-route `transcribe` operation.
//!
//! A model watches and listens to one video and writes its transcript. The precheck reads the
//! video and its description first, so no model request goes to a video that cannot be read,
//! and the transcript document carries the description and its chapters. The result is
//! delivered like a platform full text; a fetch never falls back to it.

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
use crate::platform_precheck::{
    PrecheckDepth, PrecheckPlan, Prechecked, plan_precheck, run_precheck,
};
use crate::providers;
use crate::types::{
    ContentDepth, Deadline, ModelTranscriptOutcome, Platform, PlatformContent, PlatformFetchResult,
    PlatformItem, PlatformItemData, PlatformTranscribeResult, ProviderAttempt, ProviderError,
    TRANSCRIBE, YoutubeTranscribeRequest,
};

/// The precheck and the routes a transcription runs, in order, with the routes skipped before
/// it runs.
pub(crate) struct TranscribePlan {
    precheck: PrecheckPlan,
    routes: Vec<PlatformRouteConfig>,
    skipped: Vec<ProviderAttempt>,
}

/// Plans a transcription; `config` is YouTube's.
pub(crate) fn plan_transcribe(
    config: &PlatformRuntimeConfig,
    request: YoutubeTranscribeRequest,
) -> Result<TranscribePlan, PlatformPreflightError> {
    let plan = plan_routes(
        TRANSCRIBE,
        config.platform(),
        &config.order_key(),
        providers::YOUTUBE_TRANSCRIBE_ROUTES,
        &route_candidates(config),
        None,
        providers::youtube_transcribe_support,
    )?;
    let precheck = plan_precheck(config, request.video, PrecheckDepth::Description)?;
    Ok(TranscribePlan {
        precheck,
        routes: plan.routes.into_iter().map(|(_, config)| config).collect(),
        skipped: plan.skipped,
    })
}

/// Runs a planned transcription: the precheck, then the route chain. The transcript document is
/// the result's content, not yet written.
pub(crate) async fn run_transcribe(
    plan: TranscribePlan,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    verbose: bool,
) -> Result<PlatformTranscribeResult, ProviderError> {
    let TranscribePlan {
        precheck,
        routes,
        skipped,
    } = plan;
    let Prechecked { video: item, trace } = run_precheck(
        precheck,
        skipped,
        client.clone(),
        retry_policy,
        deadline,
        verbose,
    )
    .await?;
    let video = &item;
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
            target: operation_target(Platform::Youtube, TRANSCRIBE),
            budget_policy: BudgetPolicy::SlicedEven {
                skipped_message: "skipped to preserve fallback deadline budget",
            },
            fallback_off: false,
            diagnostic_merge: DiagnosticMerge::Join,
            terminal: TerminalPolicy::ChainWide {
                verbose,
                exhausted_message: "platform transcribe deadline elapsed",
            },
            identity: &route_identity,
            continue_on_failure: &chain::always_continue,
        },
        deadline,
        |config: PlatformRouteConfig, step_deadline| {
            let client = client.clone();
            async move {
                let route = config.route();
                match providers::youtube_transcribe(
                    config,
                    client,
                    retry_policy,
                    step_deadline,
                    video,
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
    let chain_outcome = trace.lead(result)?;
    let (route, outcome) = chain_outcome.value;
    Ok(transcribed(
        item,
        route,
        outcome,
        if verbose {
            chain_outcome.attempts
        } else {
            Vec::new()
        },
        chain_outcome.diagnostic,
    ))
}

/// The result of a transcription: the prechecked item at full-text depth, carrying the
/// transcript instead of its description and chapters, which the document holds.
fn transcribed(
    mut item: PlatformItem,
    route: ProviderId,
    outcome: ModelTranscriptOutcome,
    attempts: Vec<ProviderAttempt>,
    diagnostic: Option<String>,
) -> PlatformTranscribeResult {
    item.depth = ContentDepth::FullText;
    if let PlatformItemData::Youtube(data) = &mut item.data {
        data.description = None;
        data.chapters = None;
        data.transcript = Some(outcome.transcript);
    }
    let content = PlatformContent::new(item.url.clone(), route.name(), outcome.markdown);
    PlatformTranscribeResult {
        fetched: PlatformFetchResult {
            platform: Platform::Youtube,
            provider: route.name(),
            item,
            content: Some(content),
            attempts,
            diagnostic,
        },
        model: outcome.model,
        usage: outcome.usage,
    }
}
