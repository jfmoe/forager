use super::{PlatformPreflightError, RouteCandidate, RoutePlan, plan_routes};
use crate::catalog::{self, PlatformOperation, ProviderId};
use crate::types::{
    ArxivSearchOptions, AttemptDisposition, Platform, PlatformSearchOptions, PlatformSearchRequest,
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
