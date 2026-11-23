use super::{
    PlatformPreflightError, RouteCandidate, RoutePlan, plan_cursor_search, plan_routes, plan_search,
};
use crate::catalog::{self, PlatformOperation, ProviderId};
use crate::config::{RuntimeConfig, runtime_config_from_toml};
use crate::page_cursor::{self, PagedRequest as _};
use crate::platform_fetch::plan_fetch;
use crate::platform_operation::{plan_ask, plan_cited_by, plan_cursor_cited_by, plan_history};
use crate::types::{
    ArxivSearchOptions, AttemptDisposition, ContentDepth, Platform, PlatformAskRequest,
    PlatformFetchRequest, PlatformRef, PlatformSearchOptions, PlatformSearchRequest,
    PolymarketHistoryRequest, PolymarketInterval, PolymarketRef, PolymarketSearchOptions,
    PolymarketWindow, ScholarCitedByRequest, ScholarCitedBySort, ScholarRef, ScholarSearchOptions,
    SsrnSearchOptions, XAskRequest, XSearchOptions, YoutubeAskRequest, YoutubeRef,
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

/// What planning answered, without the plan.
type Planned = Result<(), PlatformPreflightError>;

fn search_request(options: PlatformSearchOptions) -> PlatformSearchRequest {
    PlatformSearchRequest {
        query: "momentum".into(),
        limit: 5,
        options,
        page: None,
    }
}

fn plan_first_search(runtime: &RuntimeConfig, options: PlatformSearchOptions) -> Planned {
    let platform = options.platform();
    plan_search(runtime.platforms.get(platform), search_request(options)).map(drop)
}

fn plan_abstract_fetch(runtime: &RuntimeConfig, platform: Platform, reference: &str) -> Planned {
    let reference = PlatformRef::parse(platform, reference).expect("valid ref");
    let request = PlatformFetchRequest::new(reference, ContentDepth::Abstract);
    plan_fetch(runtime.platforms.get(platform), request).map(drop)
}

fn cited_by_request() -> ScholarCitedByRequest {
    ScholarCitedByRequest {
        cited: ScholarRef::parse("scholar:18208131694456651388").expect("valid ref"),
        query: None,
        limit: 20,
        year_from: None,
        year_to: None,
        sort: ScholarCitedBySort::default(),
        page: None,
    }
}

fn scholar_cited_by(runtime: &RuntimeConfig) -> Planned {
    plan_cited_by(
        runtime.platforms.get(Platform::Scholar),
        &runtime.platform_routes,
        cited_by_request(),
    )
    .map(drop)
}

fn polymarket_history(runtime: &RuntimeConfig) -> Planned {
    let market = PolymarketRef::parse("polymarket:market/will-no-fed-rate-cuts-happen-in-2026")
        .expect("valid ref");
    let request = PolymarketHistoryRequest::new(
        market,
        None,
        PolymarketWindow::Interval(PolymarketInterval::Day),
        None,
    )
    .expect("valid request");
    plan_history(
        runtime.platforms.get(Platform::Polymarket),
        &runtime.platform_routes,
        request,
    )
    .map(drop)
}

fn plan_an_ask(runtime: &RuntimeConfig, request: PlatformAskRequest) -> Planned {
    plan_ask(
        runtime.platforms.get(request.platform()),
        &runtime.platform_routes,
        request,
    )
    .map(drop)
}

fn arxiv_search(runtime: &RuntimeConfig) -> Planned {
    plan_first_search(
        runtime,
        PlatformSearchOptions::Arxiv(ArxivSearchOptions::default()),
    )
}

fn ssrn_search(runtime: &RuntimeConfig) -> Planned {
    plan_first_search(
        runtime,
        PlatformSearchOptions::Ssrn(SsrnSearchOptions::default()),
    )
}

fn polymarket_search(runtime: &RuntimeConfig) -> Planned {
    plan_first_search(
        runtime,
        PlatformSearchOptions::Polymarket(PolymarketSearchOptions::default()),
    )
}

fn x_search(runtime: &RuntimeConfig) -> Planned {
    plan_first_search(runtime, PlatformSearchOptions::X(XSearchOptions::default()))
}

fn scholar_search(runtime: &RuntimeConfig) -> Planned {
    plan_first_search(
        runtime,
        PlatformSearchOptions::Scholar(ScholarSearchOptions::default()),
    )
}

fn arxiv_fetch(runtime: &RuntimeConfig) -> Planned {
    plan_abstract_fetch(runtime, Platform::Arxiv, "arxiv:2401.01234")
}

fn polymarket_fetch(runtime: &RuntimeConfig) -> Planned {
    plan_abstract_fetch(
        runtime,
        Platform::Polymarket,
        "polymarket:how-many-fed-rate-cuts-in-2026",
    )
}

fn x_ask(runtime: &RuntimeConfig) -> Planned {
    plan_an_ask(
        runtime,
        PlatformAskRequest::X(XAskRequest {
            query: "Starship news".into(),
            options: XSearchOptions::default(),
        }),
    )
}

fn youtube_ask(runtime: &RuntimeConfig) -> Planned {
    plan_an_ask(
        runtime,
        PlatformAskRequest::Youtube(YoutubeAskRequest {
            video: YoutubeRef::parse("youtube:jNQXAC9IVRw").expect("valid ref"),
            question: "What animal is shown?".into(),
        }),
    )
}

/// Plans one operation over a runtime configuration.
type Planner = fn(&RuntimeConfig) -> Planned;

#[test]
fn an_order_without_a_configured_route_is_a_configuration_error_that_names_the_missing_keys() {
    const SCHOLAR_WITHOUT_ORDER: &str =
        "[providers.serpapi]\nkeys = [\"serpapi-key\"]\n[platforms.scholar]\norder = []\n";
    let cases: [(&str, Planner, &str); 14] = [
        (
            "[platforms.arxiv]\norder = []\n",
            arxiv_search,
            "platforms.arxiv.order has no configured route for arxiv search",
        ),
        (
            "[platforms.arxiv]\norder = []\n",
            arxiv_fetch,
            "platforms.arxiv.order has no configured route for arxiv fetch",
        ),
        (
            "[platforms.ssrn]\norder = []\n",
            ssrn_search,
            "platforms.ssrn.order has no configured route for ssrn search",
        ),
        (
            "[platforms.polymarket]\norder = []\n",
            polymarket_search,
            "platforms.polymarket.order has no configured route for polymarket search",
        ),
        (
            "[platforms.polymarket]\norder = []\n",
            polymarket_fetch,
            "platforms.polymarket.order has no configured route for polymarket fetch",
        ),
        (
            "[platforms.polymarket]\norder = []\n",
            polymarket_history,
            "platforms.polymarket.order has no configured route for polymarket history",
        ),
        (
            "[providers.xai]\nkeys = [\"xai-key\"]\n[platforms.x]\norder = []\n",
            x_search,
            "platforms.x.order has no configured route for x search",
        ),
        (
            SCHOLAR_WITHOUT_ORDER,
            scholar_search,
            "platforms.scholar.order has no configured route for scholar search",
        ),
        (
            SCHOLAR_WITHOUT_ORDER,
            scholar_cited_by,
            "platforms.scholar.order has no configured route for scholar cited_by",
        ),
        (
            "",
            x_search,
            "platforms.x.order has no configured route for x search; set providers.xai.keys",
        ),
        (
            "",
            x_ask,
            "platforms.x.order has no configured route for x ask; set providers.xai.keys",
        ),
        (
            "[providers.youtube_api]\nkeys = [\"youtube-key\"]\n",
            youtube_ask,
            "platforms.youtube.order has no configured route for youtube ask; set providers.gemini_api.keys",
        ),
        (
            "",
            scholar_search,
            "platforms.scholar.order has no configured route for scholar search; set providers.serpapi.keys",
        ),
        (
            "",
            scholar_cited_by,
            "platforms.scholar.order has no configured route for scholar cited_by; set providers.serpapi.keys",
        ),
    ];

    for (document, plan, message) in cases {
        let runtime = runtime_config_from_toml(document).expect("valid configuration");

        assert_eq!(
            plan(&runtime),
            Err(PlatformPreflightError::Config(message.to_owned())),
            "over:\n{document}"
        );
    }
}

#[test]
fn a_cursor_whose_route_lost_its_keys_is_an_argument_error() {
    let runtime = runtime_config_from_toml("").expect("valid configuration");
    let scholar = runtime.platforms.get(Platform::Scholar);
    let search_cursor = page_cursor::encode(
        ProviderId::Serpapi,
        search_request(PlatformSearchOptions::Scholar(
            ScholarSearchOptions::default(),
        ))
        .at_page("20".into()),
    );
    let cited_by_cursor =
        page_cursor::encode(ProviderId::Serpapi, cited_by_request().at_page("20".into()));

    let refusals = [
        plan_cursor_search(scholar, &search_cursor).map(drop),
        plan_cursor_cited_by(scholar, &runtime.platform_routes, &cited_by_cursor).map(drop),
    ];

    assert_eq!(
        refusals,
        ["search", "cited_by"].map(|operation| Err(PlatformPreflightError::Argument(format!(
            "the cursor route `serpapi` is no longer available for scholar {operation}; search again without --cursor"
        ))))
    );
}
