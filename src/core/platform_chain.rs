//! Platform route chains: route planning, page cursors, and chain execution.
//!
//! The routes of one platform form a fallback chain in configured order through the shared
//! chain runner; a result never falls back to another platform. Planning is pure: it decides
//! before any request which routes run, which are skipped, and which preflight error ends the
//! command. Search and Google Scholar cited-by are the paged operations: both return a search
//! page and issue cursors bound to their own operation.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::catalog::{self, PlatformOperation, ProviderId, ProviderTransport};
use crate::chain::{
    self, BudgetPolicy, ChainSettings, ChainStep, DiagnosticMerge, StepIdentity, StepSuccess,
    StepVerdict, TerminalPolicy,
};
use crate::config::{self, PlatformRouteConfig, PlatformRuntimeConfig};
use crate::net::RetryPolicy;
use crate::providers;
use crate::types::{
    AttemptTarget, CITED_BY, Deadline, Platform, PlatformSearchOutcome, PlatformSearchPage,
    PlatformSearchRequest, ProviderAttempt, ProviderError, ScholarCitedByRequest,
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

/// The request of one page of a paged operation; a page cursor encodes it completely. Each
/// variant denies the other's fields, so a cursor payload decodes as exactly one operation. The
/// enum is untagged so a search payload is the bare search request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
enum PageRequest {
    Search(PlatformSearchRequest),
    ScholarCitedBy(ScholarCitedByRequest),
}

impl PageRequest {
    fn platform(&self) -> Platform {
        match self {
            Self::Search(request) => request.options.platform(),
            Self::ScholarCitedBy(_) => Platform::Scholar,
        }
    }

    fn operation(&self) -> &'static str {
        match self {
            Self::Search(_) => PlatformOperation::Search.as_str(),
            Self::ScholarCitedBy(_) => CITED_BY,
        }
    }

    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Search(request) => request.validate(),
            Self::ScholarCitedBy(request) => request.validate(),
        }
    }

    /// Returns the routes that implement the operation.
    fn routes(&self) -> &'static [ProviderId] {
        match self {
            Self::Search(request) => {
                catalog::platform(request.options.platform()).routes(PlatformOperation::Search)
            }
            Self::ScholarCitedBy(_) => providers::SCHOLAR_CITED_BY_ROUTES,
        }
    }

    fn support(&self, id: ProviderId) -> Option<Result<(), String>> {
        match self {
            Self::Search(request) => providers::platform_search_support(id, request),
            Self::ScholarCitedBy(request) => providers::scholar_cited_by_support(id, request),
        }
    }

    fn deadline_message(&self) -> &'static str {
        match self {
            Self::Search(_) => "platform search deadline elapsed",
            Self::ScholarCitedBy(_) => "platform cited-by deadline elapsed",
        }
    }

    /// Runs the page on one route that planning selected for the operation.
    async fn run(
        &self,
        config: PlatformRouteConfig,
        client: Client,
        retry_policy: RetryPolicy,
        deadline: Deadline,
    ) -> Result<PlatformSearchOutcome, ProviderError> {
        match self {
            Self::Search(request) => {
                providers::build_platform_search(config, client, retry_policy, deadline)
                    .search(request)
                    .await
            }
            Self::ScholarCitedBy(request) => {
                providers::scholar_cited_by(config, client, retry_policy, deadline, request).await
            }
        }
    }

    fn at_page(&self, page: String) -> Self {
        match self {
            Self::Search(request) => Self::Search(PlatformSearchRequest {
                page: Some(page),
                ..request.clone()
            }),
            Self::ScholarCitedBy(request) => Self::ScholarCitedBy(ScholarCitedByRequest {
                page: Some(page),
                ..request.clone()
            }),
        }
    }
}

/// The routes a paged operation runs, in order, and the routes skipped before it runs.
pub(crate) struct PlatformPagePlan {
    platform: Platform,
    request: PageRequest,
    routes: Vec<PlatformRouteConfig>,
    skipped: Vec<ProviderAttempt>,
}

/// Plans a first-page search over the configured routes that can apply the request options.
pub(crate) fn plan_search(
    config: &PlatformRuntimeConfig,
    request: PlatformSearchRequest,
) -> Result<PlatformPagePlan, PlatformPreflightError> {
    plan(config, PageRequest::Search(request), None)
}

/// Plans the search page a cursor names; only the route that produced the cursor may run it.
pub(crate) fn plan_cursor_search(
    config: &PlatformRuntimeConfig,
    cursor: &str,
) -> Result<PlatformPagePlan, PlatformPreflightError> {
    plan_cursor(config, PlatformOperation::Search.as_str(), cursor)
}

/// Plans the first page of the works citing a Google Scholar paper; `config` is Google
/// Scholar's.
pub(crate) fn plan_cited_by(
    config: &PlatformRuntimeConfig,
    request: ScholarCitedByRequest,
) -> Result<PlatformPagePlan, PlatformPreflightError> {
    plan(config, PageRequest::ScholarCitedBy(request), None)
}

/// Plans the cited-by page a cursor names; only the route that produced the cursor may run it.
pub(crate) fn plan_cursor_cited_by(
    config: &PlatformRuntimeConfig,
    cursor: &str,
) -> Result<PlatformPagePlan, PlatformPreflightError> {
    plan_cursor(config, CITED_BY, cursor)
}

fn plan_cursor(
    config: &PlatformRuntimeConfig,
    operation: &'static str,
    cursor: &str,
) -> Result<PlatformPagePlan, PlatformPreflightError> {
    let platform = config.platform();
    let (route, request) = decode_cursor(cursor).map_err(PlatformPreflightError::Argument)?;
    let owner = (request.platform(), request.operation());
    if owner != (platform, operation) {
        return Err(PlatformPreflightError::Argument(format!(
            "the cursor belongs to {} {}, not {platform} {operation}",
            owner.0, owner.1
        )));
    }
    request
        .validate()
        .map_err(|error| PlatformPreflightError::Argument(format!("invalid cursor: {error}")))?;
    plan(config, request, Some(route))
}

fn plan(
    config: &PlatformRuntimeConfig,
    request: PageRequest,
    pinned: Option<ProviderId>,
) -> Result<PlatformPagePlan, PlatformPreflightError> {
    let plan = plan_routes(
        request.operation(),
        config.platform(),
        &config.order_key(),
        request.routes(),
        &route_candidates(config),
        pinned,
        |id| request.support(id),
    )?;
    Ok(PlatformPagePlan {
        platform: config.platform(),
        request,
        routes: plan.routes.into_iter().map(|(_, config)| config).collect(),
        skipped: plan.skipped,
    })
}

/// Runs a planned page and encodes the next-page cursor.
pub(crate) async fn run_page(
    plan: PlatformPagePlan,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    verbose: bool,
) -> Result<PlatformSearchPage, ProviderError> {
    let PlatformPagePlan {
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
            target: operation_target(platform, request.operation()),
            budget_policy: BudgetPolicy::SlicedEven {
                skipped_message: "skipped to preserve fallback deadline budget",
            },
            fallback_off: false,
            diagnostic_merge: DiagnosticMerge::Join,
            terminal: TerminalPolicy::ChainWide {
                verbose,
                exhausted_message: request.deadline_message(),
            },
            identity: &route_identity,
            continue_on_failure: &chain::always_continue,
        },
        deadline,
        |config, step_deadline| {
            let client = client.clone();
            async move {
                let route = config.route();
                match request_ref
                    .run(config, client, retry_policy, step_deadline)
                    .await
                {
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
                next_cursor: next_page.map(|page| encode_cursor(route, &request.at_page(page))),
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

pub(crate) fn operation_target(platform: Platform, operation: &'static str) -> AttemptTarget {
    AttemptTarget::platform(platform.as_str(), operation)
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
    operation: &'static str,
    platform: Platform,
    order_key: &str,
    operation_routes: &[ProviderId],
    candidates: &[RouteCandidate<'_, C>],
    pinned: Option<ProviderId>,
    support: impl Fn(ProviderId) -> Option<Result<(), String>>,
) -> Result<RoutePlan<C>, PlatformPreflightError> {
    let available = candidates
        .iter()
        .filter(|candidate| candidate.configured && operation_routes.contains(&candidate.id))
        .collect::<Vec<_>>();
    let check = |id: ProviderId| {
        support(id).unwrap_or_else(|| {
            Err(format!(
                "{} has no {platform} {operation} adapter",
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
                    "the cursor route `{}` is no longer available for {platform} {operation}; search again without --cursor",
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
        // Configuration only fails for a route that requires credentials and has none.
        let missing_keys = candidates
            .iter()
            .filter(|candidate| !candidate.configured && operation_routes.contains(&candidate.id))
            .map(|candidate| config::provider_keys_key(candidate.id.name()))
            .collect::<Vec<_>>();
        let hint = if missing_keys.is_empty() {
            opt_in_hint(order_key, operation_routes, candidates)
        } else {
            format!("; set {}", missing_keys.join(" or "))
        };
        return Err(PlatformPreflightError::Config(format!(
            "{order_key} has no configured route for {platform} {operation}{hint}"
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
                plan.skipped.push(crate::chain::skipped_attempt(
                    operation_target(platform, operation),
                    candidate.id.name(),
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

/// Says how to enable the operation's process routes that the order leaves out. They drive the
/// user's own browser, so only the user can enable them (ADR 0020).
fn opt_in_hint<C>(
    order_key: &str,
    operation_routes: &[ProviderId],
    candidates: &[RouteCandidate<'_, C>],
) -> String {
    operation_routes
        .iter()
        .filter(|route| !candidates.iter().any(|candidate| candidate.id == **route))
        .filter_map(|route| match catalog::registration(*route).transport {
            ProviderTransport::OpenCli(adapter) => Some(format!(
                "; to use {route}, install the forager OpenCLI adapter `{site}` (copy the `opencli/{site}` directory of the forager skill to `~/.opencli/clis/{site}`), open the site in the Chrome that OpenCLI drives and log in if it asks, then add `{route}` to {order_key}",
                route = route.name(),
                site = adapter.site,
            )),
            ProviderTransport::Http => None,
        })
        .collect()
}

/// Encodes `v1.<route>.<payload>`; the payload restores the complete request and page.
fn encode_cursor(route: ProviderId, request: &PageRequest) -> String {
    let payload = serde_json::to_vec(request).expect("a page request always serializes to JSON");
    format!(
        "{CURSOR_VERSION}.{}.{}",
        route.name(),
        URL_SAFE_NO_PAD.encode(payload)
    )
}

fn decode_cursor(cursor: &str) -> Result<(ProviderId, PageRequest), String> {
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
        .and_then(|payload| serde_json::from_slice::<PageRequest>(&payload).ok())
        .ok_or("the cursor cannot be decoded; search again without --cursor")?;
    Ok((route, request))
}

#[cfg(test)]
mod tests {
    use super::{
        PageRequest, PlatformPreflightError, RouteCandidate, RoutePlan, decode_cursor,
        encode_cursor, plan_routes,
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
            PlatformOperation::Search.as_str(),
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
    fn routes_outside_the_operation_do_not_run() {
        let result = plan(
            &[candidate(ProviderId::Tavily, true)],
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
    fn routes_without_credentials_do_not_run_and_the_error_names_their_keys() {
        let result = plan(
            &[
                candidate(ProviderId::Tavily, true),
                candidate(ProviderId::Jina, false),
            ],
            None,
            jina_without_categories,
        );

        assert_eq!(
            result.err(),
            Some(PlatformPreflightError::Config(
                "platforms.arxiv.order has no configured route for arxiv search; set providers.jina.keys".into()
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
        let cursor = encode_cursor(ProviderId::ArxivApi, &PageRequest::Search(request()));

        let decoded = decode_cursor(&cursor);

        assert_eq!(
            (cursor.starts_with("v1.arxiv_api."), decoded),
            (
                true,
                Ok((ProviderId::ArxivApi, PageRequest::Search(request())))
            )
        );
    }

    #[test]
    fn malformed_cursors_are_rejected() {
        let valid = encode_cursor(ProviderId::ArxivApi, &PageRequest::Search(request()));
        let payload = valid.rsplit('.').next().expect("payload");
        let results = [
            "garbage".to_owned(),
            format!("v2.arxiv_api.{payload}"),
            format!("v1.unknown_route.{payload}"),
            "v1.arxiv_api.not-base64!".to_owned(),
            "v1.arxiv_api.e30".to_owned(),
        ]
        .map(|cursor| decode_cursor(&cursor).err());

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
