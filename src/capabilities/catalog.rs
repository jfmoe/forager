use std::sync::LazyLock;
use std::time::Duration;

use crate::rate_limit::AccessPolicy;
use crate::types::Platform;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CapabilityCatalog {
    pub(crate) seam: &'static str,
    pub(crate) providers: &'static [ProviderId],
}

pub(crate) const MAIN_SEARCH: CapabilityCatalog = CapabilityCatalog {
    seam: "main_search",
    providers: &[ProviderId::Xai, ProviderId::Openai],
};
pub(crate) const WEB_SEARCH: CapabilityCatalog = CapabilityCatalog {
    seam: "web_search",
    providers: &[ProviderId::Tavily, ProviderId::Firecrawl],
};
pub(crate) const WEB_FETCH: CapabilityCatalog = CapabilityCatalog {
    seam: "web_fetch",
    providers: &[ProviderId::Firecrawl, ProviderId::Jina, ProviderId::Tavily],
};
pub(crate) const DOCS_SEARCH: CapabilityCatalog = CapabilityCatalog {
    seam: "docs_search",
    providers: &[ProviderId::Exa],
};

pub(crate) const CATALOGS: &[CapabilityCatalog] =
    &[MAIN_SEARCH, WEB_SEARCH, WEB_FETCH, DOCS_SEARCH];

impl CapabilityCatalog {
    pub(crate) fn contains(self, id: ProviderId) -> bool {
        self.providers.contains(&id)
    }
}

pub(crate) fn by_seam(seam: &str) -> Option<CapabilityCatalog> {
    CATALOGS
        .iter()
        .copied()
        .find(|catalog| catalog.seam == seam)
}

#[path = "live_check.rs"]
mod live_check;
#[path = "platform_catalog.rs"]
mod platform_catalog;

use live_check::LiveArgument::{ContentDir, Input, Text, Timeout};
pub(crate) use live_check::{
    BROWSER_LIMIT, LiveArgument, LiveCheck, LiveCommand, LiveContract, LiveInput, LiveTier,
    MODEL_LIMIT, RoutePin, StopSignal,
};

pub(crate) use platform_catalog::{
    ARXIV, HN, PLATFORMS, POLYMARKET, PlatformCatalog, PlatformOperation, REDDIT, SCHOLAR, SSRN, X,
    XIAOHONGSHU, YOUTUBE, config_set_hint, is_opt_in_route, opt_in_risk, platform, route_platform,
};

pub(crate) fn supports(capability: &str, provider: &str) -> bool {
    let Some(id) = ProviderId::parse(provider) else {
        return false;
    };
    by_seam(capability).is_some_and(|catalog| catalog.contains(id))
}

/// Defines the provider identity once: the enumeration, the list of every identity and each
/// identity's configuration name. The parser is derived from the list and the names.
macro_rules! provider_ids {
    ($($variant:ident => $name:literal,)+) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub(crate) enum ProviderId {
            $($variant,)+
        }

        impl ProviderId {
            pub(crate) const ALL: [Self; [$($name),+].len()] = [$(Self::$variant),+];

            pub(crate) const fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $name,)+
                }
            }
        }
    };
}

provider_ids! {
    Xai => "xai",
    Openai => "openai",
    Exa => "exa",
    Tavily => "tavily",
    Firecrawl => "firecrawl",
    Jina => "jina",
    ArxivApi => "arxiv_api",
    SsrnCrossref => "ssrn_crossref",
    Serpapi => "serpapi",
    XiaohongshuBrowser => "xiaohongshu_browser",
    HnApi => "hn_api",
    YoutubeApi => "youtube_api",
    YoutubeWeb => "youtube_web",
    GeminiApi => "gemini_api",
    PolymarketApi => "polymarket_api",
    RedditBrowser => "reddit_browser",
}

impl ProviderId {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|id| id.name() == value)
    }
}

/// How forager reaches a provider. Configuration checks and doctor branch on it, never on a
/// provider ID.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProviderTransport {
    /// HTTP requests to the configured `url`.
    Http,
    /// Pages that a browser forager launches loads and drives, in the declared environment.
    Browser(BrowserEnvironment),
}

/// The browser environment a browser route declares. The preflight check, doctor, `forager
/// browser login`, and the platform checklist read it instead of branching on a route ID.
///
/// Every browser route runs in the headless Managed Browser, or the browser that `browser.path`
/// names, with the Browser Profile of the platform it serves, and needs a profile that a
/// `forager browser login` once succeeded in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BrowserEnvironment {
    /// The page language, such as `zh-CN`.
    pub(crate) language: &'static str,
    /// The results that end a Live Check run's remaining checks on the site; the browser route
    /// procedure lists the same stop rules for a person.
    pub(crate) stop_signals: &'static [StopSignal],
}

/// A free check of every configured key at the service's account endpoint, which `doctor` runs
/// for one provider. It proves the endpoint and the keys, not any operation's request or
/// decoding, and spends no quota.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AccountCheck {
    pub(crate) name: &'static str,
    pub(crate) transport: &'static str,
}

/// Whether a provider's requests carry a credential from its Provider Credential Pool.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Credentials {
    /// Every request sends a key; without one the provider is not configured.
    Required,
    /// Requests send a key when the pool has one and run anonymously otherwise, or after the
    /// pool fails on its keys (Anonymous Fallback).
    Optional,
    /// Requests never send a key.
    Unused,
}

#[derive(Clone, Copy)]
pub(crate) struct ProviderRegistration {
    pub(crate) id: ProviderId,
    pub(crate) credentials: Credentials,
    pub(crate) transport: ProviderTransport,
    /// The pacing every send to the provider must follow; a browser route paces each browser
    /// command, not each HTTP request the page sends.
    pub(crate) access_policy: Option<AccessPolicy>,
    pub(crate) account_check: Option<AccountCheck>,
    pub(crate) live_checks: &'static [LiveCheck],
    /// Whether the route's platform fetch reads the full text itself, so a full-text fetch
    /// through it needs no Web Fetch provider.
    pub(crate) native_full_text: bool,
}

impl ProviderRegistration {
    /// Returns whether the provider can run with `key_count` configured credentials.
    ///
    /// Only a provider that requires credentials needs a key to be configured.
    pub(crate) fn is_configured(&self, key_count: usize) -> bool {
        self.credentials != Credentials::Required || key_count > 0
    }

    /// Returns whether the provider's credentials are optional and none is configured, so every
    /// request runs anonymously.
    pub(crate) fn lacks_optional_keys(&self, key_count: usize) -> bool {
        self.credentials == Credentials::Optional && key_count == 0
    }
}

// The Live Checks of each registration. Each check pins the routes it reaches, so it proves one
// route whatever the user's orders say, and its tier follows from those routes (ADR-0039). A
// canary is chosen so that its answer is never legitimately empty and does not drift.

const MAIN_CANARY_QUERY: &str = "What is the latest stable Rust release?";
const FETCH_CANARY_URL: &str = "https://www.rust-lang.org/";
const PLATFORM_CANARY_QUERY: &str = "retrieval augmented generation";
// The first YouTube upload (2005), whose metadata no longer changes; only its picture answers
// the ask question, because its speech never names the animals' number or the speaker's clothes.
const YOUTUBE_CANARY_VIDEO: &str = "youtube:jNQXAC9IVRw";
// A Google Scholar paper with thousands of citing works, so its first cited-by page is never
// empty.
const SCHOLAR_CANARY: &str = "scholar:18208131694456651388";

const fn main_search(id: &'static str, provider: ProviderId) -> LiveCheck {
    LiveCheck::new(
        id,
        LiveTier::Metered,
        LiveCommand {
            arguments: &[
                Text("search"),
                Text(MAIN_CANARY_QUERY),
                Text("--capabilities"),
                Text("none"),
                Text("--timeout"),
                Timeout,
            ],
            routes: RoutePin::MainSearch(provider),
        },
        LiveContract::Answer,
    )
    .time_limit(MODEL_LIMIT)
}

/// A supplemental web search, which only the hidden `doctor` entry runs on its own; `arguments`
/// name the provider.
const fn supplemental_search(id: &'static str, arguments: &'static [LiveArgument]) -> LiveCheck {
    LiveCheck::new(
        id,
        LiveTier::Metered,
        LiveCommand {
            arguments,
            routes: RoutePin::Configured,
        },
        LiveContract::Results,
    )
    .time_limit(MODEL_LIMIT)
}

const fn web_fetch(id: &'static str, tier: LiveTier, routes: RoutePin) -> LiveCheck {
    LiveCheck::new(
        id,
        tier,
        LiveCommand {
            arguments: &[
                Text("fetch"),
                Text(FETCH_CANARY_URL),
                Text("--timeout"),
                Timeout,
            ],
            routes,
        },
        LiveContract::Fetch {
            anonymous: matches!(routes, RoutePin::AnonymousWebFetch(_)),
        },
    )
}

/// A check of one platform operation, its order holding only `routes`.
const fn platform_check(
    id: &'static str,
    tier: LiveTier,
    platform: Platform,
    routes: &'static [ProviderId],
    arguments: &'static [LiveArgument],
    contract: LiveContract,
) -> LiveCheck {
    LiveCheck::new(
        id,
        tier,
        LiveCommand {
            arguments,
            routes: RoutePin::Platform(platform, routes),
        },
        contract,
    )
}

const XAI_LIVE: &[LiveCheck] = &[
    main_search("xai:main_search", ProviderId::Xai),
    platform_check(
        "xai:search",
        LiveTier::Metered,
        Platform::X,
        &[ProviderId::Xai],
        &[
            Text("platform"),
            Text("x"),
            Text("search"),
            Text(PLATFORM_CANARY_QUERY),
            Text("--limit"),
            Text("3"),
            Text("--timeout"),
            Timeout,
        ],
        LiveContract::PlatformItems,
    )
    .time_limit(MODEL_LIMIT),
    platform_check(
        "xai:fetch",
        LiveTier::Metered,
        Platform::X,
        &[ProviderId::Xai],
        &[
            Text("platform"),
            Text("x"),
            Text("fetch"),
            Text("x:2098634733248720992"),
            Text("--depth"),
            Text("metadata"),
            Text("--timeout"),
            Timeout,
        ],
        LiveContract::PlatformItem,
    )
    .time_limit(MODEL_LIMIT),
    // SpaceX posts about Starship every week, so the answer always has posts to cite.
    platform_check(
        "xai:ask",
        LiveTier::Metered,
        Platform::X,
        &[ProviderId::Xai],
        &[
            Text("platform"),
            Text("x"),
            Text("ask"),
            Text("What has SpaceX posted about Starship recently?"),
            Text("--timeout"),
            Timeout,
        ],
        LiveContract::Answer,
    )
    .time_limit(MODEL_LIMIT),
];

const OPENAI_LIVE: &[LiveCheck] = &[main_search("openai:main_search", ProviderId::Openai)];

const TAVILY_LIVE: &[LiveCheck] = &[
    supplemental_search(
        "tavily:web_search",
        &[
            Text("doctor"),
            Text("--supplemental-web-search"),
            Text("tavily"),
            Text("--query"),
            Text(MAIN_CANARY_QUERY),
            Text("--timeout"),
            Timeout,
        ],
    ),
    web_fetch(
        "tavily:web_fetch",
        LiveTier::Metered,
        RoutePin::WebFetch(ProviderId::Tavily),
    ),
    // `map` accepts at most 150 seconds.
    LiveCheck::new(
        "tavily:site_map",
        LiveTier::Metered,
        LiveCommand {
            arguments: &[
                Text("map"),
                Text(FETCH_CANARY_URL),
                Text("--limit"),
                Text("10"),
                Text("--timeout"),
                Timeout,
            ],
            routes: RoutePin::Configured,
        },
        LiveContract::Results,
    )
    .time_limit(Duration::from_secs(150)),
];

const FIRECRAWL_LIVE: &[LiveCheck] = &[
    supplemental_search(
        "firecrawl:web_search",
        &[
            Text("doctor"),
            Text("--supplemental-web-search"),
            Text("firecrawl"),
            Text("--query"),
            Text(MAIN_CANARY_QUERY),
            Text("--timeout"),
            Timeout,
        ],
    ),
    web_fetch(
        "firecrawl:web_fetch",
        LiveTier::Metered,
        RoutePin::WebFetch(ProviderId::Firecrawl),
    ),
];

// Every user without a key reads pages through anonymous Jina by default.
const JINA_LIVE: &[LiveCheck] = &[
    web_fetch(
        "jina:web_fetch",
        LiveTier::Metered,
        RoutePin::WebFetch(ProviderId::Jina),
    ),
    web_fetch(
        "jina:web_fetch:anonymous",
        LiveTier::Free,
        RoutePin::AnonymousWebFetch(ProviderId::Jina),
    ),
];

const EXA_LIVE: &[LiveCheck] = &[
    LiveCheck::new(
        "exa:docs_search",
        LiveTier::Metered,
        LiveCommand {
            arguments: &[
                Text("exa"),
                Text("search"),
                Text("Rust async drop proposal"),
                Text("--num-results"),
                Text("3"),
                Text("--timeout"),
                Timeout,
            ],
            routes: RoutePin::Configured,
        },
        LiveContract::Results,
    ),
    LiveCheck::new(
        "exa:similar",
        LiveTier::Metered,
        LiveCommand {
            arguments: &[
                Text("exa"),
                Text("similar"),
                Text(FETCH_CANARY_URL),
                Text("--num-results"),
                Text("3"),
                Text("--timeout"),
                Timeout,
            ],
            routes: RoutePin::Configured,
        },
        LiveContract::Results,
    ),
];

const ARXIV_API_LIVE: &[LiveCheck] = &[
    platform_check(
        "arxiv_api:search",
        LiveTier::Free,
        Platform::Arxiv,
        &[ProviderId::ArxivApi],
        &[
            Text("platform"),
            Text("arxiv"),
            Text("search"),
            Text(PLATFORM_CANARY_QUERY),
            Text("--limit"),
            Text("3"),
            Text("--timeout"),
            Timeout,
        ],
        LiveContract::PlatformItems,
    ),
    platform_check(
        "arxiv_api:fetch",
        LiveTier::Free,
        Platform::Arxiv,
        &[ProviderId::ArxivApi],
        &[
            Text("platform"),
            Text("arxiv"),
            Text("fetch"),
            Text("arxiv:1706.03762"),
            Text("--depth"),
            Text("abstract"),
            Text("--timeout"),
            Timeout,
        ],
        LiveContract::PlatformItem,
    ),
];

const SSRN_CROSSREF_LIVE: &[LiveCheck] = &[
    platform_check(
        "ssrn_crossref:search",
        LiveTier::Free,
        Platform::Ssrn,
        &[ProviderId::SsrnCrossref],
        &[
            Text("platform"),
            Text("ssrn"),
            Text("search"),
            Text(PLATFORM_CANARY_QUERY),
            Text("--limit"),
            Text("3"),
            Text("--timeout"),
            Timeout,
        ],
        LiveContract::PlatformItems,
    ),
    platform_check(
        "ssrn_crossref:fetch",
        LiveTier::Free,
        Platform::Ssrn,
        &[ProviderId::SsrnCrossref],
        &[
            Text("platform"),
            Text("ssrn"),
            Text("fetch"),
            Text("ssrn:2042750"),
            Text("--depth"),
            Text("metadata"),
            Text("--timeout"),
            Timeout,
        ],
        LiveContract::PlatformItem,
    ),
];

const SERPAPI_LIVE: &[LiveCheck] = &[
    platform_check(
        "serpapi:search",
        LiveTier::Metered,
        Platform::Scholar,
        &[ProviderId::Serpapi],
        &[
            Text("platform"),
            Text("scholar"),
            Text("search"),
            Text(PLATFORM_CANARY_QUERY),
            Text("--limit"),
            Text("3"),
            Text("--timeout"),
            Timeout,
        ],
        LiveContract::PlatformItems,
    ),
    platform_check(
        "serpapi:fetch",
        LiveTier::Metered,
        Platform::Scholar,
        &[ProviderId::Serpapi],
        &[
            Text("platform"),
            Text("scholar"),
            Text("fetch"),
            Text(SCHOLAR_CANARY),
            Text("--depth"),
            Text("metadata"),
            Text("--timeout"),
            Timeout,
        ],
        LiveContract::PlatformItem,
    ),
    platform_check(
        "serpapi:cited_by",
        LiveTier::Metered,
        Platform::Scholar,
        &[ProviderId::Serpapi],
        &[
            Text("platform"),
            Text("scholar"),
            Text("cited-by"),
            Text(SCHOLAR_CANARY),
            Text("--limit"),
            Text("3"),
            Text("--timeout"),
            Timeout,
        ],
        LiveContract::PlatformItems,
    ),
];

// A note opens only with an access token, which expires, so the fetch reads the first search
// result and the comments check the most-commented one.
const XIAOHONGSHU_BROWSER_LIVE: &[LiveCheck] = &[
    // Xiaohongshu has new coffee notes every week, so the filtered search reaches its second page.
    platform_check(
        "xiaohongshu_browser:search",
        LiveTier::Attended,
        Platform::Xiaohongshu,
        &[ProviderId::XiaohongshuBrowser],
        &[
            Text("platform"),
            Text("xiaohongshu"),
            Text("search"),
            Text("咖啡"),
            Text("--limit"),
            Text("25"),
            Text("--sort"),
            Text("latest"),
            Text("--publish-time"),
            Text("week"),
            Text("--timeout"),
            Timeout,
        ],
        LiveContract::PlatformItems,
    )
    .time_limit(BROWSER_LIMIT),
    platform_check(
        "xiaohongshu_browser:fetch",
        LiveTier::Attended,
        Platform::Xiaohongshu,
        &[ProviderId::XiaohongshuBrowser],
        &[
            Text("platform"),
            Text("xiaohongshu"),
            Text("fetch"),
            Input(LiveInput::FirstItem("access_url")),
            Text("--depth"),
            Text("full_text"),
            Text("--content-dir"),
            ContentDir,
            Text("--timeout"),
            Timeout,
        ],
        LiveContract::PlatformFullText,
    )
    .depends_on("xiaohongshu_browser:search")
    .time_limit(BROWSER_LIMIT),
    // One comment page holds 10 top-level comments, so 15 needs the second page.
    platform_check(
        "xiaohongshu_browser:comments",
        LiveTier::Attended,
        Platform::Xiaohongshu,
        &[ProviderId::XiaohongshuBrowser],
        &[
            Text("platform"),
            Text("xiaohongshu"),
            Text("comments"),
            Input(LiveInput::MostCommentedAccessUrl),
            Text("--limit"),
            Text("15"),
            Text("--replies"),
            Text("1"),
            Text("--timeout"),
            Timeout,
        ],
        LiveContract::XiaohongshuComments,
    )
    .depends_on("xiaohongshu_browser:search")
    .time_limit(BROWSER_LIMIT),
];

const HN_API_LIVE: &[LiveCheck] = &[
    // Hacker News discusses Rust every day, so the search is never empty.
    platform_check(
        "hn_api:search",
        LiveTier::Free,
        Platform::Hn,
        &[ProviderId::HnApi],
        &[
            Text("platform"),
            Text("hn"),
            Text("search"),
            Text("rust"),
            Text("--limit"),
            Text("5"),
            Text("--timeout"),
            Timeout,
        ],
        LiveContract::PlatformItems,
    ),
    // The 2007 Dropbox launch discussion: about 70 comments that no longer change.
    platform_check(
        "hn_api:fetch",
        LiveTier::Free,
        Platform::Hn,
        &[ProviderId::HnApi],
        &[
            Text("platform"),
            Text("hn"),
            Text("fetch"),
            Text("hn:8863"),
            Text("--depth"),
            Text("thread"),
            Text("--content-dir"),
            ContentDir,
            Text("--timeout"),
            Timeout,
        ],
        LiveContract::PlatformFullText,
    ),
    platform_check(
        "hn_api:stories",
        LiveTier::Free,
        Platform::Hn,
        &[ProviderId::HnApi],
        &[
            Text("platform"),
            Text("hn"),
            Text("stories"),
            Text("top"),
            Text("--limit"),
            Text("10"),
            Text("--timeout"),
            Timeout,
        ],
        LiveContract::PlatformItems,
    ),
];

// The 2024 presidential election, resolved and no longer changing, and its winning market,
// whose history ends in a payout of 1.
const POLYMARKET_CANARY: &str = "polymarket:presidential-election-winner-2024";
const POLYMARKET_CANARY_WINNER: &str =
    "polymarket:market/will-donald-trump-win-the-2024-us-presidential-election";

const POLYMARKET_API_LIVE: &[LiveCheck] = &[
    // Markets on the Fed's rate decisions are always open, so the search is never empty.
    platform_check(
        "polymarket_api:search",
        LiveTier::Free,
        Platform::Polymarket,
        &[ProviderId::PolymarketApi],
        &[
            Text("platform"),
            Text("polymarket"),
            Text("search"),
            Text("fed rate"),
            Text("--limit"),
            Text("3"),
            Text("--timeout"),
            Timeout,
        ],
        LiveContract::PlatformItems,
    ),
    platform_check(
        "polymarket_api:fetch:resolved",
        LiveTier::Free,
        Platform::Polymarket,
        &[ProviderId::PolymarketApi],
        &[
            Text("platform"),
            Text("polymarket"),
            Text("fetch"),
            Text(POLYMARKET_CANARY),
            Text("--depth"),
            Text("metadata"),
            Text("--timeout"),
            Timeout,
        ],
        LiveContract::PolymarketResolved {
            winner: POLYMARKET_CANARY_WINNER,
        },
    ),
    // The open event the search found proves the live CLOB quotes.
    platform_check(
        "polymarket_api:fetch:live",
        LiveTier::Free,
        Platform::Polymarket,
        &[ProviderId::PolymarketApi],
        &[
            Text("platform"),
            Text("polymarket"),
            Text("fetch"),
            Input(LiveInput::FirstItem("ref")),
            Text("--timeout"),
            Timeout,
        ],
        LiveContract::PolymarketLive,
    )
    .depends_on("polymarket_api:search"),
    platform_check(
        "polymarket_api:history",
        LiveTier::Free,
        Platform::Polymarket,
        &[ProviderId::PolymarketApi],
        &[
            Text("platform"),
            Text("polymarket"),
            Text("history"),
            Text(POLYMARKET_CANARY_WINNER),
            Text("--interval"),
            Text("max"),
            Text("--timeout"),
            Timeout,
        ],
        LiveContract::PolymarketSettlement,
    ),
];

// The fetch reads a post the search found, never one chosen from memory, and never one not
// marked as safe.
const REDDIT_BROWSER_LIVE: &[LiveCheck] = &[
    // Reddit discusses Rust every day, so the search is never empty.
    platform_check(
        "reddit_browser:search",
        LiveTier::Attended,
        Platform::Reddit,
        &[ProviderId::RedditBrowser],
        &[
            Text("platform"),
            Text("reddit"),
            Text("search"),
            Text("rust"),
            Text("--limit"),
            Text("3"),
            Text("--timeout"),
            Timeout,
        ],
        LiveContract::PlatformItems,
    )
    .time_limit(BROWSER_LIMIT),
    platform_check(
        "reddit_browser:fetch",
        LiveTier::Attended,
        Platform::Reddit,
        &[ProviderId::RedditBrowser],
        &[
            Text("platform"),
            Text("reddit"),
            Text("fetch"),
            Input(LiveInput::FirstSafeRef),
            Text("--depth"),
            Text("thread"),
            Text("--content-dir"),
            ContentDir,
            Text("--timeout"),
            Timeout,
        ],
        LiveContract::PlatformFullText,
    )
    .depends_on("reddit_browser:search")
    .time_limit(BROWSER_LIMIT),
];

const YOUTUBE_API_LIVE: &[LiveCheck] = &[
    // A search costs 100 units of the daily quota of 10,000.
    // Many channels lecture on attention and transformers, so the search is never empty.
    platform_check(
        "youtube_api:search",
        LiveTier::Metered,
        Platform::Youtube,
        &[ProviderId::YoutubeApi],
        &[
            Text("platform"),
            Text("youtube"),
            Text("search"),
            Text("attention transformer"),
            Text("--limit"),
            Text("5"),
            Text("--timeout"),
            Timeout,
        ],
        LiveContract::PlatformItems,
    )
    .expensive(),
    platform_check(
        "youtube_api:fetch",
        LiveTier::Metered,
        Platform::Youtube,
        &[ProviderId::YoutubeApi],
        &[
            Text("platform"),
            Text("youtube"),
            Text("fetch"),
            Text(YOUTUBE_CANARY_VIDEO),
            Text("--depth"),
            Text("metadata"),
            Text("--timeout"),
            Timeout,
        ],
        LiveContract::PlatformItem,
    ),
    // The canary video has millions of comments.
    platform_check(
        "youtube_api:comments",
        LiveTier::Metered,
        Platform::Youtube,
        &[ProviderId::YoutubeApi],
        &[
            Text("platform"),
            Text("youtube"),
            Text("comments"),
            Text(YOUTUBE_CANARY_VIDEO),
            Text("--limit"),
            Text("5"),
            Text("--timeout"),
            Timeout,
        ],
        LiveContract::Comments,
    ),
    // "Essence of linear algebra": a finished public course of 16 videos.
    platform_check(
        "youtube_api:videos",
        LiveTier::Metered,
        Platform::Youtube,
        &[ProviderId::YoutubeApi],
        &[
            Text("platform"),
            Text("youtube"),
            Text("videos"),
            Text("youtube:playlist/PLZHQObOWTQDPD3MizzM2xVFitgF8hE_ab"),
            Text("--limit"),
            Text("5"),
            Text("--timeout"),
            Timeout,
        ],
        LiveContract::PlatformItems,
    ),
];

// Only this route reads a transcript; its anonymous reads put the egress address at risk.
const YOUTUBE_WEB_LIVE: &[LiveCheck] = &[platform_check(
    "youtube_web:fetch",
    LiveTier::Attended,
    Platform::Youtube,
    &[ProviderId::YoutubeWeb],
    &[
        Text("platform"),
        Text("youtube"),
        Text("fetch"),
        Text(YOUTUBE_CANARY_VIDEO),
        Text("--depth"),
        Text("full_text"),
        Text("--content-dir"),
        ContentDir,
        Text("--timeout"),
        Timeout,
    ],
    LiveContract::PlatformFullText,
)];

// A model operation reads its video through YouTube's fetch chain first, so the order pins that
// read to `youtube_api` and never reaches the attended `youtube_web`. Whether this use of the
// gateway complies with the provider's terms is unverified, so both checks are manual-only.
const GEMINI_API_LIVE: &[LiveCheck] = &[
    platform_check(
        "gemini_api:ask",
        LiveTier::Metered,
        Platform::Youtube,
        &[ProviderId::YoutubeApi, ProviderId::GeminiApi],
        &[
            Text("platform"),
            Text("youtube"),
            Text("ask"),
            Text(YOUTUBE_CANARY_VIDEO),
            Text("How many elephants are visible behind the speaker, and what is he wearing?"),
            Text("--timeout"),
            Timeout,
        ],
        LiveContract::Answer,
    )
    .time_limit(MODEL_LIMIT)
    .manual_only(),
    platform_check(
        "gemini_api:transcribe",
        LiveTier::Metered,
        Platform::Youtube,
        &[ProviderId::YoutubeApi, ProviderId::GeminiApi],
        &[
            Text("platform"),
            Text("youtube"),
            Text("transcribe"),
            Text(YOUTUBE_CANARY_VIDEO),
            Text("--content-dir"),
            ContentDir,
            Text("--timeout"),
            Timeout,
        ],
        LiveContract::PlatformFullText,
    )
    .time_limit(MODEL_LIMIT)
    .manual_only(),
];

// arXiv terms of use allow one request every three seconds over one connection.
const ARXIV_API_ACCESS: AccessPolicy = AccessPolicy {
    min_interval: Duration::from_secs(3),
    max_concurrency: 1,
};

// The anonymous Crossref pool announced 5 requests per second over one connection on
// 2026-09-26; one request per second stays well inside it.
const SSRN_CROSSREF_ACCESS: AccessPolicy = AccessPolicy {
    min_interval: Duration::from_secs(1),
    max_concurrency: 1,
};

// About 55 read-only commands 5 to 10 seconds apart raised no risk control on 2026-10-08; ten
// seconds is the slowest pace measured, not a limit the site states (ADR-0036).
const XIAOHONGSHU_BROWSER_ACCESS: AccessPolicy = AccessPolicy {
    min_interval: Duration::from_secs(10),
    max_concurrency: 1,
};

// Algolia allows 10,000 HN Search requests per IP per hour; the official Firebase API states no
// limit. Both endpoints share this pace, at most 7,200 requests per hour.
const HN_API_ACCESS: AccessPolicy = AccessPolicy {
    min_interval: Duration::from_millis(500),
    max_concurrency: 1,
};

// One pace for all three endpoints: a ninth of the strictest official limit that applies,
// Gamma search at 350 requests per 10 seconds.
const POLYMARKET_API_ACCESS: AccessPolicy = AccessPolicy {
    min_interval: Duration::from_millis(250),
    max_concurrency: 1,
};

// Reddit states no pace for its pages; one browser command every ten seconds, as for
// Xiaohongshu, met no verification page in nine logged-in page opens on 2026-11-21.
const REDDIT_BROWSER_ACCESS: AccessPolicy = AccessPolicy {
    min_interval: Duration::from_secs(10),
    max_concurrency: 1,
};

// The Data API meters quota, not pace, and failed requests still cost quota; one request per
// second keeps a runaway loop from draining the daily search budget in moments.
const YOUTUBE_API_ACCESS: AccessPolicy = AccessPolicy {
    min_interval: Duration::from_secs(1),
    max_concurrency: 1,
};

// The yt-dlp wiki's anonymous pace is about 300 videos an hour, 5 to 10 seconds apart; one
// request every four seconds keeps a fetch's three requests near it. Going faster risks the
// user's egress IP (ADR-0028).
const YOUTUBE_WEB_ACCESS: AccessPolicy = AccessPolicy {
    min_interval: Duration::from_secs(4),
    max_concurrency: 1,
};

// A model request may run for minutes and bills by the token, so one runs at a time across
// processes; the service meters keys, not pace.
const GEMINI_API_ACCESS: AccessPolicy = AccessPolicy {
    min_interval: Duration::ZERO,
    max_concurrency: 1,
};

const REGISTRY: &[ProviderRegistration] = &[
    ProviderRegistration {
        id: ProviderId::Xai,
        credentials: Credentials::Required,
        transport: ProviderTransport::Http,
        access_policy: None,
        account_check: None,
        live_checks: XAI_LIVE,
        native_full_text: false,
    },
    ProviderRegistration {
        id: ProviderId::Openai,
        credentials: Credentials::Required,
        transport: ProviderTransport::Http,
        access_policy: None,
        account_check: None,
        live_checks: OPENAI_LIVE,
        native_full_text: false,
    },
    ProviderRegistration {
        id: ProviderId::Tavily,
        credentials: Credentials::Required,
        transport: ProviderTransport::Http,
        access_policy: None,
        account_check: None,
        live_checks: TAVILY_LIVE,
        native_full_text: false,
    },
    ProviderRegistration {
        id: ProviderId::Firecrawl,
        credentials: Credentials::Required,
        transport: ProviderTransport::Http,
        access_policy: None,
        account_check: None,
        live_checks: FIRECRAWL_LIVE,
        native_full_text: false,
    },
    ProviderRegistration {
        id: ProviderId::Jina,
        credentials: Credentials::Optional,
        transport: ProviderTransport::Http,
        access_policy: None,
        account_check: None,
        live_checks: JINA_LIVE,
        native_full_text: false,
    },
    ProviderRegistration {
        id: ProviderId::Exa,
        credentials: Credentials::Required,
        transport: ProviderTransport::Http,
        access_policy: None,
        account_check: None,
        live_checks: EXA_LIVE,
        native_full_text: false,
    },
    ProviderRegistration {
        id: ProviderId::ArxivApi,
        credentials: Credentials::Unused,
        transport: ProviderTransport::Http,
        access_policy: Some(ARXIV_API_ACCESS),
        account_check: None,
        live_checks: ARXIV_API_LIVE,
        native_full_text: false,
    },
    ProviderRegistration {
        id: ProviderId::SsrnCrossref,
        credentials: Credentials::Unused,
        transport: ProviderTransport::Http,
        access_policy: Some(SSRN_CROSSREF_ACCESS),
        account_check: None,
        live_checks: SSRN_CROSSREF_LIVE,
        native_full_text: false,
    },
    // SerpApi limits throughput per account; a 429 rotates to the next key instead of pacing.
    ProviderRegistration {
        id: ProviderId::Serpapi,
        credentials: Credentials::Required,
        transport: ProviderTransport::Http,
        access_policy: None,
        account_check: Some(AccountCheck {
            name: "account",
            transport: "http",
        }),
        live_checks: SERPAPI_LIVE,
        native_full_text: false,
    },
    ProviderRegistration {
        id: ProviderId::XiaohongshuBrowser,
        credentials: Credentials::Unused,
        transport: ProviderTransport::Browser(BrowserEnvironment {
            language: "zh-CN",
            stop_signals: &[
                StopSignal::Auth,
                StopSignal::Parameter,
                StopSignal::Runtime,
                StopSignal::TwoTimeoutsInARow,
            ],
        }),
        access_policy: Some(XIAOHONGSHU_BROWSER_ACCESS),
        account_check: None,
        live_checks: XIAOHONGSHU_BROWSER_LIVE,
        native_full_text: true,
    },
    // A full-text fetch of a link story reads the linked page through Web Fetch.
    ProviderRegistration {
        id: ProviderId::HnApi,
        credentials: Credentials::Unused,
        transport: ProviderTransport::Http,
        access_policy: Some(HN_API_ACCESS),
        account_check: None,
        live_checks: HN_API_LIVE,
        native_full_text: false,
    },
    // Search and fetch read only official keyless endpoints; a fetch never needs Web Fetch.
    ProviderRegistration {
        id: ProviderId::PolymarketApi,
        credentials: Credentials::Unused,
        transport: ProviderTransport::Http,
        access_policy: Some(POLYMARKET_API_ACCESS),
        account_check: None,
        live_checks: POLYMARKET_API_LIVE,
        native_full_text: false,
    },
    // A full-text fetch of a link post reads the linked page through Web Fetch.
    ProviderRegistration {
        id: ProviderId::RedditBrowser,
        credentials: Credentials::Unused,
        transport: ProviderTransport::Browser(BrowserEnvironment {
            language: "en-US",
            stop_signals: &[StopSignal::Auth, StopSignal::TwoTimeoutsInARow],
        }),
        access_policy: Some(REDDIT_BROWSER_ACCESS),
        account_check: None,
        live_checks: REDDIT_BROWSER_LIVE,
        native_full_text: false,
    },
    ProviderRegistration {
        id: ProviderId::YoutubeApi,
        credentials: Credentials::Required,
        transport: ProviderTransport::Http,
        access_policy: Some(YOUTUBE_API_ACCESS),
        account_check: None,
        live_checks: YOUTUBE_API_LIVE,
        native_full_text: false,
    },
    // Its full text is the transcript it renders itself.
    ProviderRegistration {
        id: ProviderId::YoutubeWeb,
        credentials: Credentials::Unused,
        transport: ProviderTransport::Http,
        access_policy: Some(YOUTUBE_WEB_ACCESS),
        account_check: None,
        live_checks: YOUTUBE_WEB_LIVE,
        native_full_text: true,
    },
    // Its account check lists the models with every key, which spends no model quota.
    ProviderRegistration {
        id: ProviderId::GeminiApi,
        credentials: Credentials::Required,
        transport: ProviderTransport::Http,
        access_policy: Some(GEMINI_API_ACCESS),
        account_check: Some(AccountCheck {
            name: "models",
            transport: "http",
        }),
        live_checks: GEMINI_API_LIVE,
        native_full_text: false,
    },
];

/// Returns whether a registration is legitimate: it belongs to a capability catalog or a
/// platform catalog.
pub(crate) fn has_owner(registration: &ProviderRegistration) -> bool {
    CATALOGS
        .iter()
        .any(|catalog| catalog.contains(registration.id))
        || PLATFORMS
            .iter()
            .any(|platform| platform.contains(registration.id))
}

static VALIDATED_REGISTRY: LazyLock<()> = LazyLock::new(|| {
    if let Err(error) = validate_registrations(REGISTRY) {
        panic!("invalid provider registry: {error}");
    }
});

pub(crate) fn registrations() -> &'static [ProviderRegistration] {
    LazyLock::force(&VALIDATED_REGISTRY);
    REGISTRY
}

pub(crate) fn registration(id: ProviderId) -> &'static ProviderRegistration {
    registrations()
        .iter()
        .find(|registration| registration.id == id)
        .expect("validated registry contains every provider ID")
}

fn validate_registrations(registry: &[ProviderRegistration]) -> Result<(), String> {
    let ids = registry
        .iter()
        .map(|registration| registration.id)
        .collect::<std::collections::BTreeSet<_>>();
    let expected = ProviderId::ALL
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    if registry.len() != expected.len() || ids != expected {
        return Err("provider IDs must appear exactly once".into());
    }
    for registration in registry {
        if !has_owner(registration) {
            return Err(format!(
                "{} belongs to no capability or platform",
                registration.id.name()
            ));
        }
    }
    validate_live_checks(registry)
}

/// The rules of every Live Check declaration: a unique identifier with its provider's prefix, a
/// dependency on an earlier check of the same provider and tier, and the tier that the routes
/// it reaches give it (ADR-0039).
fn validate_live_checks(registry: &[ProviderRegistration]) -> Result<(), String> {
    let lookup = |id: ProviderId| registry.iter().find(|registration| registration.id == id);
    let mut ids = std::collections::BTreeSet::new();
    for registration in registry {
        let provider = registration.id;
        for (index, check) in registration.live_checks.iter().enumerate() {
            let id = check.id;
            if !ids.insert(id) {
                return Err(format!("Live Check `{id}` is declared twice"));
            }
            if id.split(':').next() != Some(provider.name()) {
                return Err(format!(
                    "Live Check `{id}` is declared by `{}` but does not start with its name",
                    provider.name()
                ));
            }
            if let Some(dependency) = check.depends_on {
                match registration.live_checks[..index]
                    .iter()
                    .find(|earlier| earlier.id == dependency)
                {
                    None => {
                        return Err(format!(
                            "Live Check `{id}` depends on `{dependency}`, which is no earlier check of `{}`",
                            provider.name()
                        ));
                    }
                    Some(earlier) if earlier.tier != check.tier => {
                        return Err(format!(
                            "Live Check `{id}` depends on `{dependency}` of another tier"
                        ));
                    }
                    Some(_) => {}
                }
            }
            let pinned = check.command.routes.routes();
            let reached = if pinned.is_empty() {
                std::slice::from_ref(&provider)
            } else {
                pinned
            };
            if !reached.contains(&provider) {
                return Err(format!("Live Check `{id}` does not reach its own provider"));
            }
            let attended = reached.iter().any(|route| is_opt_in_route(*route));
            let keyed = !matches!(check.command.routes, RoutePin::AnonymousWebFetch(_))
                && reached.iter().any(|route| {
                    lookup(*route).is_some_and(|reached| reached.credentials != Credentials::Unused)
                });
            let tier = if attended {
                LiveTier::Attended
            } else if keyed {
                LiveTier::Metered
            } else {
                LiveTier::Free
            };
            if check.tier != tier {
                return Err(format!(
                    "Live Check `{id}` is declared `{}`, but the routes it reaches make it `{}`",
                    check.tier.as_str(),
                    tier.as_str()
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        CATALOGS, Credentials, LiveCheck, LiveTier, PLATFORMS, ProviderId, ProviderRegistration,
        REGISTRY, has_owner, registration, registrations, validate_registrations,
    };

    #[test]
    fn registration_lookup_is_identifier_based_and_rejects_missing_or_duplicate_ids() {
        let mut reordered = REGISTRY.to_vec();
        reordered.swap(0, 6);
        assert!(validate_registrations(&reordered).is_ok());

        let mut misaligned = reordered;
        misaligned[0] = misaligned[1];

        assert!(validate_registrations(&misaligned).is_err());
        for id in ProviderId::ALL {
            assert_eq!(registration(id).id, id);
        }
    }

    #[test]
    fn catalogs_project_every_registration_account_check_and_live_check_consistently() {
        for registration in registrations() {
            assert!(has_owner(registration), "{} owner", registration.id.name());
            // The account check asks a platform route about each of its keys.
            assert!(
                registration.account_check.is_none()
                    || (registration.credentials == Credentials::Required
                        && PLATFORMS
                            .iter()
                            .any(|platform| platform.contains(registration.id))),
                "{} declares an account check but is no platform route that requires keys",
                registration.id.name()
            );
            assert!(
                !registration.live_checks.is_empty(),
                "{} declares no Live Check",
                registration.id.name()
            );
        }
    }

    #[test]
    fn only_a_registration_that_requires_credentials_needs_a_key() {
        let configured = [
            Credentials::Required,
            Credentials::Optional,
            Credentials::Unused,
        ]
        .map(|credentials| {
            let registration = ProviderRegistration {
                credentials,
                ..*registration(ProviderId::Jina)
            };
            (registration.is_configured(0), registration.is_configured(1))
        });

        assert_eq!(configured, [(false, true), (true, true), (true, true)]);
    }

    #[test]
    fn registry_validation_accepts_a_route_that_only_a_platform_catalog_lists() {
        assert_eq!(
            (
                CATALOGS
                    .iter()
                    .any(|catalog| catalog.contains(ProviderId::ArxivApi)),
                validate_registrations(REGISTRY),
            ),
            (false, Ok(()))
        );
    }

    #[test]
    fn registry_validation_accepts_registrations_without_credentials() {
        let mut registry = REGISTRY.to_vec();
        registry[0].credentials = Credentials::Unused;
        // Its keyed checks would now be declared in the wrong tier.
        registry[0].live_checks = &[];

        assert!(validate_registrations(&registry).is_ok());
    }

    /// The registry with `provider`'s Live Checks replaced by `checks`.
    fn with_checks(provider: ProviderId, checks: Vec<LiveCheck>) -> Vec<ProviderRegistration> {
        let checks: &'static [LiveCheck] = Box::leak(checks.into_boxed_slice());
        REGISTRY
            .iter()
            .map(|registration| ProviderRegistration {
                live_checks: if registration.id == provider {
                    checks
                } else {
                    registration.live_checks
                },
                ..*registration
            })
            .collect()
    }

    fn declared(provider: ProviderId, index: usize) -> LiveCheck {
        registration(provider).live_checks[index]
    }

    #[test]
    fn two_live_checks_with_one_identifier_break_the_registry() {
        let checks = vec![
            declared(ProviderId::HnApi, 0),
            declared(ProviderId::HnApi, 0),
        ];

        assert_eq!(
            validate_registrations(&with_checks(ProviderId::HnApi, checks)),
            Err("Live Check `hn_api:search` is declared twice".into())
        );
    }

    #[test]
    fn a_live_check_without_its_providers_prefix_breaks_the_registry() {
        let checks = vec![LiveCheck {
            id: "hn:search",
            ..declared(ProviderId::HnApi, 0)
        }];

        assert_eq!(
            validate_registrations(&with_checks(ProviderId::HnApi, checks)),
            Err(
                "Live Check `hn:search` is declared by `hn_api` but does not start with its name"
                    .into()
            )
        );
    }

    #[test]
    fn a_dependency_on_another_providers_check_breaks_the_registry() {
        let checks = vec![declared(ProviderId::HnApi, 0).depends_on("arxiv_api:search")];

        assert_eq!(
            validate_registrations(&with_checks(ProviderId::HnApi, checks)),
            Err(
                "Live Check `hn_api:search` depends on `arxiv_api:search`, which is no earlier check of `hn_api`"
                    .into()
            )
        );
    }

    #[test]
    fn a_dependency_on_a_check_of_another_tier_breaks_the_registry() {
        let keyed = declared(ProviderId::Jina, 0);
        let anonymous = declared(ProviderId::Jina, 1).depends_on(keyed.id);

        assert_eq!(
            validate_registrations(&with_checks(ProviderId::Jina, vec![keyed, anonymous])),
            Err(
                "Live Check `jina:web_fetch:anonymous` depends on `jina:web_fetch` of another tier"
                    .into()
            )
        );
    }

    #[test]
    fn a_tier_that_the_reached_routes_do_not_give_breaks_the_registry() {
        let cases = [
            (ProviderId::HnApi, 0, LiveTier::Metered, "free"),
            (ProviderId::Serpapi, 0, LiveTier::Free, "metered"),
            (ProviderId::RedditBrowser, 0, LiveTier::Metered, "attended"),
            (ProviderId::YoutubeWeb, 0, LiveTier::Metered, "attended"),
        ];
        for (provider, index, tier, expected) in cases {
            let check = LiveCheck {
                tier,
                depends_on: None,
                ..declared(provider, index)
            };

            let error = validate_registrations(&with_checks(provider, vec![check]))
                .expect_err("the tier is refused");

            assert!(error.ends_with(&format!("make it `{expected}`")), "{error}");
        }
    }

    #[test]
    fn a_model_check_whose_precheck_reaches_an_opt_in_route_is_attended() {
        let check = declared(ProviderId::GeminiApi, 0);
        let through_web = LiveCheck {
            command: super::LiveCommand {
                routes: super::RoutePin::Platform(
                    crate::types::Platform::Youtube,
                    &[ProviderId::YoutubeWeb, ProviderId::GeminiApi],
                ),
                ..check.command
            },
            ..check
        };

        let error = validate_registrations(&with_checks(ProviderId::GeminiApi, vec![through_web]))
            .expect_err("the tier is refused");

        assert!(error.ends_with("make it `attended`"), "{error}");
    }
}
