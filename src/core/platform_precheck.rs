//! The precheck of a YouTube model operation: the video read that comes before any model
//! request.
//!
//! A model service answers a video it cannot open the way it answers a bad key, and a gateway
//! may then lock that key for every model, so a model operation first reads the video through
//! YouTube's fetch chain (order ∩ fetch routes ∩ configured routes). A failed read fails the
//! command without a model request, and so does a stream that is still live. The route that
//! serves the model operation never reads the platform itself; it gets the item read here, and
//! the trace of the read leads the trace of its route chain.

use reqwest::Client;

use crate::config::PlatformRuntimeConfig;
use crate::net::RetryPolicy;
use crate::platform_chain::PlatformPreflightError;
use crate::platform_fetch::{self, PlatformFetchPlan};
use crate::types::{
    AttemptErrorKind, ContentDepth, Deadline, PlatformFetchOptions, PlatformFetchRequest,
    PlatformItem, PlatformItemData, PlatformRef, ProviderAttempt, ProviderError,
    YoutubeFetchOptions, YoutubeLiveStatus, YoutubeRef,
};

/// What the precheck reads: the video's fields only, or its description too, for a model
/// operation whose document carries it.
#[derive(Clone, Copy)]
pub(crate) enum PrecheckDepth {
    Metadata,
    Description,
}

/// The planned precheck of one YouTube video.
pub(crate) struct PrecheckPlan {
    fetch: PlatformFetchPlan,
}

/// Plans the precheck of `video` over YouTube's fetch routes; `config` is YouTube's.
pub(crate) fn plan_precheck(
    config: &PlatformRuntimeConfig,
    video: YoutubeRef,
    depth: PrecheckDepth,
) -> Result<PrecheckPlan, PlatformPreflightError> {
    let reference = PlatformRef::Youtube(video);
    let request = match depth {
        PrecheckDepth::Metadata => PlatformFetchRequest::new(reference, ContentDepth::Metadata),
        // The description of a video without one is empty, not a failure: the video may still
        // be read by a model.
        PrecheckDepth::Description => PlatformFetchRequest {
            options: PlatformFetchOptions::Youtube(YoutubeFetchOptions {
                allow_empty_description: true,
                ..YoutubeFetchOptions::default()
            }),
            ..PlatformFetchRequest::new(reference, ContentDepth::Abstract)
        },
    };
    Ok(PrecheckPlan {
        fetch: platform_fetch::plan_fetch(config, request)?,
    })
}

/// The video the precheck read, the attempts of the read led by those recorded before it, and the
/// read's diagnostic.
pub(crate) struct Prechecked {
    pub(crate) video: PlatformItem,
    pub(crate) attempts: Vec<ProviderAttempt>,
    pub(crate) diagnostic: Option<String>,
}

/// Reads the video; a failed read, or a stream that is live, fails with every attempt of the
/// read. `prior` holds the attempts recorded before the read, such as skipped routes; they
/// lead the attempts of the read on success and on failure.
pub(crate) async fn run_precheck(
    plan: PrecheckPlan,
    mut prior: Vec<ProviderAttempt>,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    verbose: bool,
) -> Result<Prechecked, ProviderError> {
    let checked =
        match platform_fetch::fetch_item(plan.fetch, client, retry_policy, deadline, verbose).await
        {
            Ok(checked) => checked,
            Err(mut error) => {
                prior.append(&mut error.attempts);
                error.attempts = prior;
                return Err(error);
            }
        };
    prior.extend(checked.attempts);
    let video = checked.item;
    if matches!(
        &video.data,
        PlatformItemData::Youtube(data) if data.live_status == Some(YoutubeLiveStatus::Live)
    ) {
        return Err(ProviderError {
            kind: AttemptErrorKind::Quality,
            message: format!(
                "{} is streaming live; a model can read it only after the stream ends",
                video.reference
            ),
            attempts: prior,
            verbose,
            diagnostic: checked.diagnostic,
        });
    }
    Ok(Prechecked {
        video,
        attempts: prior,
        diagnostic: checked.diagnostic,
    })
}
