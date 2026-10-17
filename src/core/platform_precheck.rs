//! The precheck of a model operation: the item read that comes before any model request.
//!
//! A model service answers a video it cannot open the way it answers a bad key, and a gateway
//! may then lock that key for every model, so a model operation first reads the video through
//! the platform's fetch chain (order ∩ fetch routes ∩ configured routes). A failed read fails
//! the command without a model request, and so does a stream that is still live. The route that
//! serves the model operation never reads the platform itself; it gets the item read here.

use reqwest::Client;

use crate::config::PlatformRuntimeConfig;
use crate::net::RetryPolicy;
use crate::platform_chain::PlatformPreflightError;
use crate::platform_fetch::{self, FetchedItem, PlatformFetchPlan};
use crate::types::{
    AttemptErrorKind, ContentDepth, Deadline, PlatformFetchOptions, PlatformFetchRequest,
    PlatformItemData, PlatformRef, ProviderAttempt, ProviderError, YoutubeFetchOptions,
    YoutubeLiveStatus,
};

/// What the precheck reads: bibliographic fields only, or the description too, for a model
/// operation whose document carries it.
#[derive(Clone, Copy)]
pub(crate) enum PrecheckDepth {
    Metadata,
    Description,
}

/// The planned precheck of one item.
pub(crate) struct PrecheckPlan {
    fetch: PlatformFetchPlan,
}

/// Plans the precheck of `reference` over the platform's fetch routes.
pub(crate) fn plan_precheck(
    config: &PlatformRuntimeConfig,
    reference: PlatformRef,
    depth: PrecheckDepth,
) -> Result<PrecheckPlan, PlatformPreflightError> {
    let request = match depth {
        PrecheckDepth::Metadata => PlatformFetchRequest::new(reference, ContentDepth::Metadata),
        // The description of a video without one is empty, not a failure: the video may still
        // be read by a model.
        PrecheckDepth::Description => {
            let mut request = PlatformFetchRequest::new(reference, ContentDepth::Abstract);
            if let PlatformFetchOptions::Youtube(options) = &mut request.options {
                *options = YoutubeFetchOptions {
                    allow_empty_description: true,
                    ..YoutubeFetchOptions::default()
                };
            }
            request
        }
    };
    Ok(PrecheckPlan {
        fetch: platform_fetch::plan_fetch(config, request)?,
    })
}

/// Reads the item; a failed read, or a live stream, fails with every attempt of the read.
/// `prior` holds the attempts recorded before the read, such as skipped routes; they lead the
/// attempts of the read on success and on failure.
pub(crate) async fn run_precheck(
    plan: PrecheckPlan,
    mut prior: Vec<ProviderAttempt>,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    verbose: bool,
) -> Result<FetchedItem, ProviderError> {
    let mut checked =
        match platform_fetch::fetch_item(plan.fetch, client, retry_policy, deadline, verbose).await
        {
            Ok(checked) => checked,
            Err(mut error) => {
                prior.append(&mut error.attempts);
                error.attempts = prior;
                return Err(error);
            }
        };
    prior.append(&mut checked.attempts);
    checked.attempts = prior;
    if let PlatformItemData::Youtube(data) = &checked.item.data
        && data.live_status == Some(YoutubeLiveStatus::Live)
    {
        return Err(ProviderError {
            kind: AttemptErrorKind::Quality,
            message: format!(
                "{} is streaming live; a model can read it only after the stream ends",
                checked.item.reference
            ),
            attempts: checked.attempts,
            verbose,
            diagnostic: checked.diagnostic,
            redirected_library_id: None,
        });
    }
    Ok(checked)
}
