//! Platform route chains: route planning, and the chain of platform search.
//!
//! The routes of one platform form a fallback chain in configured order through the shared
//! chain runner; a result never falls back to another platform. Planning is pure: it decides
//! before any request which routes run, which are skipped, and which preflight error ends the
//! command. Search returns a search page and issues cursors bound to its operation.

use reqwest::Client;

use crate::catalog::{self, PlatformOperation, ProviderId, ProviderTransport};
use crate::chain::{
    self, BudgetPolicy, ChainSettings, ChainStep, DiagnosticMerge, StepIdentity, StepSuccess,
    StepVerdict, TerminalPolicy,
};
use crate::config::{self, BrowserRuntimeConfig, PlatformRouteConfig, PlatformRuntimeConfig};
use crate::net::RetryPolicy;
use crate::page_cursor::{self, PagedRequest};
use crate::providers;
use crate::types::{
    AttemptTarget, Deadline, Platform, PlatformSearchPage, PlatformSearchRequest, ProviderAttempt,
    ProviderError,
};

/// A failure found before any request is sent.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum PlatformPreflightError {
    /// The arguments or cursor cannot run; exit 2.
    Argument(String),
    /// The configuration leaves the operation without a route; exit 3.
    Config(String),
}

/// The routes a search page runs, in order, and the routes skipped before it runs.
pub(crate) struct SearchPlan {
    platform: Platform,
    request: PlatformSearchRequest,
    routes: Vec<PlatformRouteConfig>,
    skipped: Vec<ProviderAttempt>,
}

/// Plans a first-page search over the configured routes that can apply the request options.
pub(crate) fn plan_search(
    config: &PlatformRuntimeConfig,
    request: PlatformSearchRequest,
) -> Result<SearchPlan, PlatformPreflightError> {
    plan(config, request, None)
}

/// Plans the search page a cursor names; only the route that produced the cursor may run it.
pub(crate) fn plan_cursor_search(
    config: &PlatformRuntimeConfig,
    cursor: &str,
) -> Result<SearchPlan, PlatformPreflightError> {
    let (route, request) = page_cursor::decode(
        cursor,
        config.platform(),
        PlatformOperation::Search.as_str(),
    )
    .map_err(PlatformPreflightError::Argument)?;
    plan(config, request, Some(route))
}

fn plan(
    config: &PlatformRuntimeConfig,
    request: PlatformSearchRequest,
    pinned: Option<ProviderId>,
) -> Result<SearchPlan, PlatformPreflightError> {
    let platform = config.platform();
    let plan = plan_routes(
        PlatformOperation::Search.as_str(),
        platform,
        &config.order_key(),
        catalog::platform(platform).routes(PlatformOperation::Search),
        &route_candidates(config),
        pinned,
        |id| providers::platform_search_support(id, &request),
    )?;
    Ok(SearchPlan {
        platform,
        request,
        routes: plan.routes.into_iter().map(|(_, config)| config).collect(),
        skipped: plan.skipped,
    })
}

/// Runs a planned search page and encodes the next-page cursor.
pub(crate) async fn run_search(
    plan: SearchPlan,
    client: Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    verbose: bool,
) -> Result<PlatformSearchPage, ProviderError> {
    let SearchPlan {
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
        })
        .collect();
    let result = chain::run_chain(
        steps,
        ChainSettings {
            target: operation_target(platform, PlatformOperation::Search.as_str()),
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
        },
        deadline,
        |config: PlatformRouteConfig, step_deadline| {
            let client = client.clone();
            async move {
                let route = config.route();
                match providers::build_platform_search(config, client, retry_policy, step_deadline)
                    .search(request_ref)
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
                collection: None,
                items,
                next_cursor: next_page
                    .map(|page| page_cursor::encode(route, request.at_page(page))),
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
        let opt_in = opt_in_hint(order_key, operation_routes, candidates, |_| true);
        let hint = if missing_keys.is_empty() {
            opt_in
        } else {
            format!("; set {}{opt_in}", missing_keys.join(" or "))
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
        let opt_in = opt_in_hint(order_key, operation_routes, candidates, |id| {
            check(id).is_ok()
        });
        return Err(PlatformPreflightError::Argument(format!(
            "no configured {platform} route supports the requested options (configured routes: {configured}): {}{opt_in}",
            reasons.join("; ")
        )));
    }
    Ok(plan)
}

/// Says how to enable the operation's routes that the order leaves out, that only users may
/// enable, and that `relevant` keeps: a browser route acts on the user's own account, and an
/// HTTP route may read a site's undocumented endpoints (ADR-0028).
fn opt_in_hint<C>(
    order_key: &str,
    operation_routes: &[ProviderId],
    candidates: &[RouteCandidate<'_, C>],
    relevant: impl Fn(ProviderId) -> bool,
) -> String {
    let order = candidates
        .iter()
        .map(|candidate| candidate.id)
        .collect::<Vec<_>>();
    operation_routes
        .iter()
        .filter(|route| !order.contains(route) && relevant(**route))
        .filter_map(|route| match catalog::registration(*route).transport {
            ProviderTransport::Browser(_) => {
                let login = format!(
                    ", run `forager browser login {platform}` and log in",
                    platform = catalog::route_platform(*route)
                );
                let risk = catalog::opt_in_risk(*route)
                    .map(|risk| format!(". {risk}"))
                    .unwrap_or_default();
                Some(format!(
                    "; to use {route}, run `forager browser install`{login}, then add `{route}` to {order_key}{risk}",
                    route = route.name(),
                ))
            }
            ProviderTransport::Http => {
                catalog::config_set_hint(*route, order_key, &order).map(|text| format!("; {text}"))
            }
        })
        .collect()
}

/// Fails a platform command before its route chain when a browser route in the order cannot
/// run: the browser cannot be found, or its Browser Profile never had the login it needs. The
/// check launches nothing; it is a Config failure, not an attempt.
///
/// # Errors
///
/// Returns the Config message, with the command that fixes the first failing route.
pub(crate) fn browser_preflight(
    config: &PlatformRuntimeConfig,
    browser: &BrowserRuntimeConfig,
) -> Result<(), String> {
    for entry in config.entries() {
        if let ProviderTransport::Browser(_) = catalog::registration(entry.id()).transport {
            providers::browser_preflight(entry.id(), browser)
                .map_err(|reason| format!("{} cannot run: {reason}", entry.id().name()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{PlatformPreflightError, RouteCandidate, RoutePlan, plan_routes};
    use crate::catalog::{self, PlatformOperation, ProviderId};
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
            query: "dark matter".into(),
            limit: 5,
            options: options(),
            page: None,
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

    /// Plans a YouTube full-text fetch that only `youtube_web` can serve.
    fn plan_youtube_full_text(
        candidates: &[RouteCandidate<'_, ()>],
    ) -> Result<RoutePlan<()>, PlatformPreflightError> {
        plan_routes(
            PlatformOperation::Fetch.as_str(),
            Platform::Youtube,
            "platforms.youtube.order",
            catalog::YOUTUBE.routes(PlatformOperation::Fetch),
            candidates,
            None,
            |id| {
                Some(if id == ProviderId::YoutubeWeb {
                    Ok(())
                } else {
                    Err(format!("{} does not serve captions", id.name()))
                })
            },
        )
    }

    const YOUTUBE_WEB_ADDED: &str = r#"`forager config set platforms.youtube.order '["youtube_api", "youtube_web", "gemini_api"]'`"#;

    #[test]
    fn options_only_an_omitted_opt_in_route_supports_name_the_order_that_adds_it() {
        let result = plan_youtube_full_text(&[
            candidate(ProviderId::YoutubeApi, true),
            candidate(ProviderId::GeminiApi, true),
        ]);

        let Some(PlatformPreflightError::Argument(message)) = result.err() else {
            panic!("expected an argument error");
        };
        assert!(
            message.starts_with(
                "no configured youtube route supports the requested options (configured routes: youtube_api): youtube_api does not serve captions; youtube_web reads"
            ) && message.ends_with(YOUTUBE_WEB_ADDED),
            "{message}"
        );
    }

    #[test]
    fn an_order_without_configured_routes_names_the_order_that_adds_the_opt_in_route() {
        let result = plan_youtube_full_text(&[
            candidate(ProviderId::YoutubeApi, false),
            candidate(ProviderId::GeminiApi, true),
        ]);

        let Some(PlatformPreflightError::Config(message)) = result.err() else {
            panic!("expected a configuration error");
        };
        assert!(
            message.starts_with(
                "platforms.youtube.order has no configured route for youtube fetch; set providers.youtube_api.keys; youtube_web reads"
            ) && message.ends_with(YOUTUBE_WEB_ADDED),
            "{message}"
        );
    }

    fn empty_order_message(platform: Platform) -> String {
        let catalog = catalog::platform(platform);
        let order_key = format!("platforms.{platform}.order");
        let result = plan_routes::<()>(
            PlatformOperation::Search.as_str(),
            platform,
            &order_key,
            catalog.search,
            &[],
            None,
            |_| Some(Ok(())),
        );
        let Some(PlatformPreflightError::Config(message)) = result.err() else {
            panic!("expected a configuration error");
        };
        message
    }

    #[test]
    fn an_empty_order_gives_each_browser_route_its_steps_and_its_account_risk() {
        let reddit = empty_order_message(Platform::Reddit);
        let xiaohongshu = empty_order_message(Platform::Xiaohongshu);

        assert_eq!(
            reddit,
            "platforms.reddit.order has no configured route for reddit search; to use reddit_browser, run `forager browser install`, run `forager browser login reddit` and log in, then add `reddit_browser` to platforms.reddit.order. reddit_browser reads Reddit in a browser logged into your account, which Reddit's User Agreement does not allow for automated access, so it is off by default; its risk falls on the Reddit account you log in with, not your egress IP"
        );
        assert_eq!(
            xiaohongshu,
            "platforms.xiaohongshu.order has no configured route for xiaohongshu search; to use xiaohongshu_browser, run `forager browser install`, run `forager browser login xiaohongshu` and log in, then add `xiaohongshu_browser` to platforms.xiaohongshu.order. xiaohongshu_browser reads Xiaohongshu in a browser logged into your account, which Xiaohongshu forbids for crawlers, so it is off by default; its risk falls on the Xiaohongshu account you log in with, which the site may rate-limit or block"
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
}
