//! Platform route chains: route planning, page cursors, and chain execution.
//!
//! The routes of one platform form a fallback chain in configured order through the shared
//! chain runner; a result never falls back to another platform. Planning is pure: it decides
//! before any request which routes run, which are skipped, and which preflight error ends the
//! command.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use reqwest::Client;

use crate::catalog::{self, PlatformOperation, ProviderId};
use crate::chain::{
    self, BudgetPolicy, ChainSettings, ChainStep, DiagnosticMerge, StepIdentity, StepSuccess,
    StepVerdict, TerminalPolicy,
};
use crate::config::{PlatformRouteConfig, PlatformRuntimeConfig};
use crate::net::RetryPolicy;
use crate::providers;
use crate::types::{
    AttemptDisposition, AttemptTarget, Deadline, Platform, PlatformSearchPage,
    PlatformSearchRequest, ProviderAttempt, ProviderError,
};

const CURSOR_VERSION: &str = "v1";

/// A failure found before any request is sent.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum PlatformPreflightError {
    /// The arguments or cursor cannot run; exit 2.
    Argument(String),
    /// The configuration leaves the operation without a route; exit 3.
    Config(String),
}

/// The routes a platform search runs, in order, and the routes skipped before it runs.
pub(crate) struct PlatformSearchPlan {
    platform: Platform,
    request: PlatformSearchRequest,
    routes: Vec<PlatformRouteConfig>,
    skipped: Vec<ProviderAttempt>,
}

/// Plans a first-page search over the configured routes that can apply the request options.
pub(crate) fn plan_search(
    config: &PlatformRuntimeConfig,
    request: PlatformSearchRequest,
) -> Result<PlatformSearchPlan, PlatformPreflightError> {
    plan(config, request, None)
}

/// Plans the page a cursor names; only the route that produced the cursor may run it.
pub(crate) fn plan_cursor_search(
    config: &PlatformRuntimeConfig,
    cursor: &str,
) -> Result<PlatformSearchPlan, PlatformPreflightError> {
    let (route, request) =
        decode_cursor(config.platform(), cursor).map_err(PlatformPreflightError::Argument)?;
    request
        .validate()
        .map_err(|error| PlatformPreflightError::Argument(format!("invalid cursor: {error}")))?;
    plan(config, request, Some(route))
}

fn plan(
    config: &PlatformRuntimeConfig,
    request: PlatformSearchRequest,
    pinned: Option<ProviderId>,
) -> Result<PlatformSearchPlan, PlatformPreflightError> {
    let plan = plan_routes(
        PlatformOperation::Search,
        config.platform(),
        &config.order_key(),
        catalog::platform(config.platform()).routes(PlatformOperation::Search),
        &route_candidates(config),
        pinned,
        |id| providers::platform_search_support(id, &request),
    )?;
    Ok(PlatformSearchPlan {
        platform: config.platform(),
        request,
        routes: plan.routes.into_iter().map(|(_, config)| config).collect(),
        skipped: plan.skipped,
    })
}

/// Runs a planned search and encodes the next-page cursor.
pub(crate) async fn search(
    plan: PlatformSearchPlan,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    verbose: bool,
) -> Result<PlatformSearchPage, ProviderError> {
    let PlatformSearchPlan {
        platform,
        request,
        routes,
        mut skipped,
    } = plan;
    let request_ref = &request;
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
            target: search_target(platform),
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
            continue_on_failure: &chain::always_continue,
        },
        deadline,
        |config, step_deadline| {
            let client = client.clone();
            async move {
                let route = config.route();
                let adapter =
                    providers::build_platform_search(config, client, retry_policy, step_deadline);
                match adapter.search(request_ref).await {
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
    match result {
        Ok(outcome) => {
            let (route, items, next_page) = outcome.value;
            skipped.extend(outcome.attempts);
            Ok(PlatformSearchPage {
                platform,
                provider: route.name(),
                items,
                next_cursor: next_page.map(|page| {
                    encode_cursor(
                        route,
                        &PlatformSearchRequest {
                            page: Some(page),
                            ..request.clone()
                        },
                    )
                }),
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

pub(crate) fn route_identity(config: &PlatformRouteConfig) -> StepIdentity {
    StepIdentity {
        provider: config.route().name(),
        model: None,
        endpoint_host: None,
    }
}

pub(crate) fn operation_target(platform: Platform, operation: PlatformOperation) -> AttemptTarget {
    AttemptTarget::platform(platform.as_str(), operation.as_str())
}

fn search_target(platform: Platform) -> AttemptTarget {
    operation_target(platform, PlatformOperation::Search)
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
    operation: PlatformOperation,
    platform: Platform,
    order_key: &str,
    operation_routes: &[ProviderId],
    candidates: &[RouteCandidate<'_, C>],
    pinned: Option<ProviderId>,
    support: impl Fn(ProviderId) -> Option<Result<(), String>>,
) -> Result<RoutePlan<C>, PlatformPreflightError> {
    let operation_name = operation.as_str();
    let available = candidates
        .iter()
        .filter(|candidate| candidate.configured && operation_routes.contains(&candidate.id))
        .collect::<Vec<_>>();
    let check = |id: ProviderId| {
        support(id).unwrap_or_else(|| {
            Err(format!(
                "{} has no {platform} {operation_name} adapter",
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
                    "the cursor route `{}` is no longer available for {platform} {operation_name}; search again without --cursor",
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
        return Err(PlatformPreflightError::Config(format!(
            "{order_key} has no configured route for {platform} {operation_name}"
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
                plan.skipped.push(skipped_attempt(
                    operation_target(platform, operation),
                    candidate.id,
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
        return Err(PlatformPreflightError::Argument(format!(
            "no configured {platform} route supports the requested options (configured routes: {configured}): {}",
            reasons.join("; ")
        )));
    }
    Ok(plan)
}

fn skipped_attempt(target: AttemptTarget, route: ProviderId, reason: &str) -> ProviderAttempt {
    ProviderAttempt {
        provider: route.name(),
        target,
        disposition: AttemptDisposition::Skipped,
        error_kind: None,
        http_status: None,
        duration_ms: 0,
        credential_index: 0,
        retry_count: 0,
        rotation_count: 0,
        message: format!("skipped: {reason}"),
        model: None,
        transport: None,
        endpoint_host: None,
        breaker_event: None,
    }
}

/// Encodes `v1.<route>.<payload>`; the payload restores the complete request and page.
fn encode_cursor(route: ProviderId, request: &PlatformSearchRequest) -> String {
    let payload = serde_json::to_vec(request).expect("a search request always serializes to JSON");
    format!(
        "{CURSOR_VERSION}.{}.{}",
        route.name(),
        URL_SAFE_NO_PAD.encode(payload)
    )
}

fn decode_cursor(
    platform: Platform,
    cursor: &str,
) -> Result<(ProviderId, PlatformSearchRequest), String> {
    let mut parts = cursor.splitn(3, '.');
    let (Some(version), Some(route), Some(payload)) = (parts.next(), parts.next(), parts.next())
    else {
        return Err("the cursor cannot be decoded; search again without --cursor".into());
    };
    if version != CURSOR_VERSION {
        return Err(format!(
            "unsupported cursor version `{version}`; search again without --cursor"
        ));
    }
    let route = ProviderId::parse(route)
        .ok_or_else(|| format!("the cursor names unknown route `{route}`"))?;
    let request = URL_SAFE_NO_PAD
        .decode(payload)
        .ok()
        .and_then(|payload| serde_json::from_slice::<PlatformSearchRequest>(&payload).ok())
        .ok_or("the cursor cannot be decoded; search again without --cursor")?;
    let owner = request.options.platform();
    if owner != platform {
        return Err(format!(
            "the cursor belongs to {owner} search, not {platform} search"
        ));
    }
    Ok((route, request))
}

#[cfg(test)]
mod tests {
    use super::{
        PlatformPreflightError, RouteCandidate, RoutePlan, decode_cursor, encode_cursor,
        plan_routes,
    };
    use crate::catalog::{PlatformOperation, ProviderId};
    use crate::types::{
        ArxivSearchOptions, AttemptDisposition, Platform, PlatformSearchOptions,
        PlatformSearchRequest,
    };

    const ORDER_KEY: &str = "platforms.arxiv.order";
    // A test-only route set: `jina` stands in for a second arXiv route that cannot apply
    // `--category`.
    const TEST_ROUTES: &[ProviderId] = &[ProviderId::ArxivApi, ProviderId::Jina];

    fn options() -> PlatformSearchOptions {
        PlatformSearchOptions::Arxiv(ArxivSearchOptions {
            categories: vec!["cs.AI".into()],
            ..ArxivSearchOptions::default()
        })
    }

    fn first_page() -> PlatformSearchRequest {
        PlatformSearchRequest {
            page: None,
            ..request()
        }
    }

    fn candidate(id: ProviderId, configured: bool) -> RouteCandidate<'static, ()> {
        RouteCandidate {
            id,
            config: &(),
            configured,
        }
    }

    #[expect(
        clippy::unnecessary_wraps,
        reason = "matches the support-check signature of the platform factory"
    )]
    fn jina_without_categories(
        id: ProviderId,
        _: &PlatformSearchRequest,
    ) -> Option<Result<(), String>> {
        Some(if id == ProviderId::Jina {
            Err("jina does not support --category".into())
        } else {
            Ok(())
        })
    }

    fn plan(
        candidates: &[RouteCandidate<'_, ()>],
        pinned: Option<ProviderId>,
        support: fn(ProviderId, &PlatformSearchRequest) -> Option<Result<(), String>>,
    ) -> Result<RoutePlan<()>, PlatformPreflightError> {
        let request = first_page();
        plan_routes(
            PlatformOperation::Search,
            Platform::Arxiv,
            ORDER_KEY,
            TEST_ROUTES,
            candidates,
            pinned,
            |id| support(id, &request),
        )
    }

    fn route_ids(plan: &RoutePlan<()>) -> Vec<ProviderId> {
        plan.routes.iter().map(|(id, ())| *id).collect()
    }

    #[test]
    fn a_partially_supported_order_runs_supported_routes_and_skips_the_rest() {
        let plan = plan(
            &[
                candidate(ProviderId::Jina, true),
                candidate(ProviderId::ArxivApi, true),
            ],
            None,
            jina_without_categories,
        )
        .expect("plan");
        let skipped = plan
            .skipped
            .iter()
            .map(|attempt| {
                (
                    attempt.provider,
                    attempt.disposition,
                    attempt.error_kind,
                    attempt.message.as_str(),
                    serde_json::to_value(attempt.target).expect("serialize target"),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            (route_ids(&plan), skipped),
            (
                vec![ProviderId::ArxivApi],
                vec![(
                    "jina",
                    AttemptDisposition::Skipped,
                    None,
                    "skipped: jina does not support --category",
                    serde_json::json!({"platform": "arxiv", "operation": "search"}),
                )]
            )
        );
    }

    #[test]
    fn options_that_no_configured_route_supports_are_an_argument_error() {
        let result = plan(
            &[candidate(ProviderId::Jina, true)],
            None,
            jina_without_categories,
        );

        assert_eq!(
            result.err(),
            Some(PlatformPreflightError::Argument(
                "no configured arxiv route supports the requested options (configured routes: jina): jina does not support --category".into()
            ))
        );
    }

    #[test]
    fn routes_outside_the_operation_or_without_configuration_do_not_run() {
        let result = plan(
            &[
                candidate(ProviderId::Tavily, true),
                candidate(ProviderId::ArxivApi, false),
            ],
            None,
            jina_without_categories,
        );

        assert_eq!(
            result.err(),
            Some(PlatformPreflightError::Config(
                "platforms.arxiv.order has no configured route for arxiv search".into()
            ))
        );
    }

    #[test]
    fn an_empty_order_is_a_configuration_error() {
        let result = plan(&[], None, jina_without_categories);

        assert_eq!(
            result.err(),
            Some(PlatformPreflightError::Config(
                "platforms.arxiv.order has no configured route for arxiv search".into()
            ))
        );
    }

    #[test]
    fn a_cursor_runs_only_its_route() {
        let plan = plan(
            &[
                candidate(ProviderId::Jina, true),
                candidate(ProviderId::ArxivApi, true),
            ],
            Some(ProviderId::ArxivApi),
            |_, _| Some(Ok(())),
        )
        .expect("plan");

        assert_eq!(route_ids(&plan), vec![ProviderId::ArxivApi]);
    }

    #[test]
    fn a_cursor_its_route_cannot_run_is_an_argument_error() {
        let result = plan(
            &[candidate(ProviderId::ArxivApi, true)],
            Some(ProviderId::ArxivApi),
            |_, _| Some(Err("invalid arXiv page position `bogus`".into())),
        );

        assert_eq!(
            result.err(),
            Some(PlatformPreflightError::Argument(
                "the cursor cannot run: invalid arXiv page position `bogus`; search again without --cursor".into()
            ))
        );
    }

    #[test]
    fn a_cursor_whose_route_left_the_order_is_an_argument_error() {
        let result = plan(
            &[candidate(ProviderId::Jina, true)],
            Some(ProviderId::ArxivApi),
            jina_without_categories,
        );

        assert_eq!(
            result.err(),
            Some(PlatformPreflightError::Argument(
                "the cursor route `arxiv_api` is no longer available for arxiv search; search again without --cursor".into()
            ))
        );
    }

    fn request() -> PlatformSearchRequest {
        PlatformSearchRequest {
            query: "dark matter".into(),
            limit: 5,
            options: options(),
            page: Some("10".into()),
        }
    }

    #[test]
    fn a_cursor_restores_the_route_and_the_complete_request() {
        let cursor = encode_cursor(ProviderId::ArxivApi, &request());

        let decoded = decode_cursor(Platform::Arxiv, &cursor);

        assert_eq!(
            (cursor.starts_with("v1.arxiv_api."), decoded),
            (true, Ok((ProviderId::ArxivApi, request())))
        );
    }

    #[test]
    fn malformed_cursors_are_rejected() {
        let valid = encode_cursor(ProviderId::ArxivApi, &request());
        let payload = valid.rsplit('.').next().expect("payload");
        let results = [
            "garbage".to_owned(),
            format!("v2.arxiv_api.{payload}"),
            format!("v1.unknown_route.{payload}"),
            "v1.arxiv_api.not-base64!".to_owned(),
            "v1.arxiv_api.e30".to_owned(),
        ]
        .map(|cursor| decode_cursor(Platform::Arxiv, &cursor).err());

        assert_eq!(
            results,
            [
                Some("the cursor cannot be decoded; search again without --cursor".into()),
                Some("unsupported cursor version `v2`; search again without --cursor".into()),
                Some("the cursor names unknown route `unknown_route`".into()),
                Some("the cursor cannot be decoded; search again without --cursor".into()),
                Some("the cursor cannot be decoded; search again without --cursor".into()),
            ]
        );
    }
}
