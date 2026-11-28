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
    BROWSER_LIMIT, Field, LiveArgument, LiveCheck, LiveCommand, LiveContract, LiveInput, LiveTier,
    MODEL_LIMIT, RoutePin, SampleRule, Shape, StopSignal,
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

const fn main_search(
    id: &'static str,
    provider: ProviderId,
    contract: LiveContract,
    hard_sample: &'static str,
) -> LiveCheck {
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
        contract,
        hard_sample,
    )
    .time_limit(MODEL_LIMIT)
}

const MAIN_SEARCH_HARD: &str = "the newest release changes every six weeks, so the answer has to come \
     from a web search the backend cites, and an answer from the model's memory carries no sources";

const SUPPLEMENTAL_CONTRACT: LiveContract = LiveContract::fields(&[
    Field::list("results", 1),
    Field::text("results[].url"),
    Field::text("results[].title").in_some_items(),
    Field::text("results[].text").in_some_items(),
]);

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
        SUPPLEMENTAL_CONTRACT,
        "a question about this week's release, which the index answers with fresh pages whose \
         titles and summaries the route must carry",
    )
    .time_limit(MODEL_LIMIT)
}

const KEYED_FETCH_CONTRACT: LiveContract = LiveContract::fields(&[Field::text("content")])
    .with_sample(&[SampleRule::Credentials { anonymous: false }]);
const ANONYMOUS_FETCH_CONTRACT: LiveContract = LiveContract::fields(&[Field::text("content")])
    .with_sample(&[SampleRule::Credentials { anonymous: true }]);

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
        if matches!(routes, RoutePin::AnonymousWebFetch(_)) {
            ANONYMOUS_FETCH_CONTRACT
        } else {
            KEYED_FETCH_CONTRACT
        },
        "the Rust home page is mostly navigation and links, so a reader that returns only its \
         chrome falls below the content gate",
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
    hard_sample: &'static str,
) -> LiveCheck {
    LiveCheck::new(
        id,
        tier,
        LiveCommand {
            arguments,
            routes: RoutePin::Platform(platform, routes),
        },
        contract,
        hard_sample,
    )
}

const XAI_LIVE: &[LiveCheck] = &[
    // xAI numbers its citation titles, and forager clears a numeric title, so a source may have
    // none.
    main_search(
        "xai:main_search",
        ProviderId::Xai,
        LiveContract::fields(&[
            Field::text("answer"),
            Field::list("sources", 1),
            Field::text("sources[].url"),
        ]),
        MAIN_SEARCH_HARD,
    ),
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
        LiveContract::fields(&[
            Field::list("items", 1),
            Field::text("items[].ref"),
            Field::text("items[].author_handle"),
            Field::text("items[].text"),
        ]),
        "a topic many accounts post about, so every result must be a post the response cites, \
         and a post the model only recalled is dropped",
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
        LiveContract::fields(&[Field::text("author_handle"), Field::text("text")]),
        "a support account's reply inside a conversation, so the read must single out the \
         requested post among the thread's posts instead of taking the first",
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
        LiveContract::fields(&[
            Field::text("answer"),
            Field::list("sources", 1),
            Field::text("sources[].url"),
            Field::text("sources[].ref").in_some_items(),
        ]),
        "one account's recent posts, so the answer must cite posts found today, and an answer \
         from the model's memory cites none",
    )
    .time_limit(MODEL_LIMIT),
];

const OPENAI_LIVE: &[LiveCheck] = &[main_search(
    "openai:main_search",
    ProviderId::Openai,
    LiveContract::fields(&[
        Field::text("answer"),
        Field::list("sources", 1),
        Field::text("sources[].url"),
        Field::text("sources[].title").in_some_items(),
    ]),
    MAIN_SEARCH_HARD,
)];

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
        LiveContract::fields(&[
            Field::text("base_url"),
            Field::list("results", 2),
            Field::text("results[]"),
            Field::number("response_time", 0.0),
        ]),
        "a site with many sections on its own domain, so the map must follow links beyond the \
         page it was given",
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
        LiveContract::fields(&[
            Field::list("results", 1),
            Field::text("results[].url"),
            Field::text("results[].title").in_some_items(),
            Field::text("results[].published_date").in_some_items(),
        ]),
        "a proposal discussed across RFCs, blog posts and issue trackers, so the results come \
         from several kinds of page with different metadata",
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
        LiveContract::fields(&[
            Field::list("results", 1),
            Field::text("results[].url"),
            Field::text("results[].title").in_some_items(),
        ]),
        "a home page gives no query text to match, so only the page's own content finds its \
         neighbours",
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
        LiveContract::fields(&[
            Field::list("items", 3),
            Field::text("items[].ref"),
            Field::text("items[].title"),
            Field::list("items[].authors", 1),
            Field::text("items[].published"),
            Field::text("items[].updated"),
            Field::text("items[].abstract"),
            Field::text("items[].primary_category"),
            Field::list("items[].categories", 1),
            Field::text("items[].pdf_url"),
            Field::text("items[].comment").in_some_items(),
        ]),
        "a busy topic whose results are recent submissions with several authors, cross-listed \
         categories and author comments, which a single old paper would not all carry",
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
        LiveContract::fields(&[
            Field::text("ref"),
            Field::text("title"),
            Field::list("authors", 8),
            Field::text("published"),
            Field::text("updated"),
            Field::text("abstract"),
            Field::text("primary_category"),
            Field::list("categories", 2),
            Field::text("comment"),
            Field::text("pdf_url"),
        ]),
        "a paper revised several times and cross-listed, so an unversioned ref must resolve to \
         its latest version with all eight authors and every category",
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
        LiveContract::fields(&[
            Field::list("items", 1),
            Field::text("items[].ref"),
            Field::text("items[].title"),
            Field::list("items[].authors", 1).in_some_items(),
            Field::text("items[].published"),
            Field::text("items[].doi"),
            Field::text("items[].crossref_type"),
            Field::text("items[].crossref_created"),
            Field::text("items[].abstract").in_some_items(),
        ]),
        "Crossref files SSRN preprints beside every other publisher's works, so the search must \
         keep only SSRN records, some with an abstract and some without",
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
        LiveContract::fields(&[
            Field::text("ref"),
            Field::text("title"),
            Field::list("authors", 1),
            Field::text("published"),
            Field::text("doi"),
            Field::text("crossref_type"),
            Field::text("crossref_created"),
        ]),
        "a paper whose Crossref record is reached only through its SSRN DOI, so the work must be \
         matched to the requested ref and its authors read from Crossref's name parts",
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
        LiveContract::fields(&[
            Field::list("items", 1),
            Field::text("items[].ref"),
            Field::text("items[].title"),
            Field::list("items[].authors", 1).in_some_items(),
            Field::text("items[].published").in_some_items(),
            Field::text("items[].snippet").in_some_items(),
            Field::text("items[].link").in_some_items(),
            Field::text("items[].source").in_some_items(),
            Field::count("items[].cited_by", 1).in_some_items(),
            Field::count("items[].version_count", 1).in_some_items(),
            Field::list("items[].resources", 1).in_some_items(),
            Field::text("items[].resources[].url"),
        ]),
        "a broad topic whose top results mix papers with and without links, PDF resources and \
         citation counts, so every optional part of a result is decoded",
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
        LiveContract::fields(&[
            Field::text("title"),
            Field::list("authors", 1),
            Field::list("versions", 1),
            Field::text("versions[].title"),
            Field::text("versions[].source").in_some_items(),
            Field::text("versions[].link").in_some_items(),
        ]),
        "a much-cited paper with several versions, so the cluster page must be read as one paper \
         with its versions listed",
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
        LiveContract::fields(&[
            Field::list("items", 1),
            Field::text("items[].ref"),
            Field::text("items[].title"),
            Field::text("items[].snippet").in_some_items(),
            Field::text("items[].published").in_some_items(),
            Field::text("items[].source").in_some_items(),
        ]),
        "an unknown paper and an uncited one both answer empty, so only a paper with thousands of \
         citing works proves the cited-by query was read",
    ),
];

// A note opens only with an access token, which expires, so the fetch reads the first search
// result and the comments check the most-commented one.
const XIAOHONGSHU_BROWSER_LIVE: &[LiveCheck] = &[
    // Search pages hold 20 notes, so 25 needs the second page.
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
        LiveContract::fields(&[
            Field::list("items", 21),
            Field::text("items[].ref"),
            Field::text("items[].access_url"),
            Field::text("items[].note_type"),
            Field::text("items[].title").in_some_items(),
            Field::list("items[].authors", 1).in_some_items(),
            Field::text("items[].author_id").in_some_items(),
            Field::text("items[].likes").in_some_items(),
            Field::text("items[].collects").in_some_items(),
            Field::text("items[].comments").in_some_items(),
            Field::text("items[].shares").in_some_items(),
            Field::text("items[].published_text").in_some_items(),
        ]),
        "new coffee notes appear every week, so the newest notes of the week fill a second page \
         under both filters",
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
        LiveContract::fields(&[
            Field::text("ref"),
            Field::text("note_type"),
            Field::list("authors", 1),
            Field::text("author_id"),
            Field::text("published"),
            Field::text("updated"),
            Field::text("likes"),
            Field::text("collects"),
            Field::text("comments"),
            Field::text("shares"),
            Field::count("content_len", 1),
        ]),
        "the newest note of the week, which no cache holds, so the page must open with the \
         access token the search just returned",
    )
    .depends_on("xiaohongshu_browser:search")
    .time_limit(BROWSER_LIMIT),
    // One comment page holds 10 top-level comments, so 15 needs the second page, and a comment
    // carries one reply before it is expanded.
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
        LiveContract::fields(&[
            Field::list("comments", 11),
            Field::text("comments[].id"),
            Field::text("comments[].text").in_some_items(),
            Field::text("comments[].author").in_some_items(),
            Field::text("comments[].published").in_some_items(),
            Field::list("comments[].replies", 2).in_some_items(),
            Field::text("comments[].replies[].id"),
            Field::text("comments[].replies[].reply_to"),
        ]),
        "the week's most-commented note, so the read reaches the second comment page and \
         expands a reply thread",
    )
    .depends_on("xiaohongshu_browser:search")
    .time_limit(BROWSER_LIMIT),
];

const HN_API_LIVE: &[LiveCheck] = &[
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
        LiveContract::fields(&[
            Field::list("items", 5),
            Field::text("items[].ref"),
            Field::text("items[].item_type"),
            Field::text("items[].published"),
            Field::text("items[].title").in_some_items(),
            Field::list("items[].authors", 1).in_some_items(),
            Field::text("items[].link_url").in_some_items(),
            Field::count("items[].points", 1).in_some_items(),
            Field::count("items[].comments", 1).in_some_items(),
        ]),
        "a relevance search over every item type, so the route must tell stories from other \
         items by their tags and read each story's link, points and comment count",
    ),
    platform_check(
        "hn_api:search:comments",
        LiveTier::Free,
        Platform::Hn,
        &[ProviderId::HnApi],
        &[
            Text("platform"),
            Text("hn"),
            Text("search"),
            Text("rust"),
            Text("--type"),
            Text("comment"),
            Text("--sort"),
            Text("date"),
            Text("--limit"),
            Text("5"),
            Text("--timeout"),
            Timeout,
        ],
        LiveContract::fields(&[
            Field::list("items", 5),
            Field::text("items[].ref"),
            Field::text("items[].item_type"),
            Field::text("items[].published"),
            Field::text("items[].text"),
            Field::text("items[].story_ref"),
            Field::text("items[].parent_ref"),
            Field::list("items[].authors", 1).in_some_items(),
            Field::text("items[].story_title").in_some_items(),
        ]),
        "the newest comments that mention Rust, which have no title, link or points, so each is \
         read by its text and the story and comment it answers",
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
        LiveContract::fields(&[
            Field::text("title"),
            Field::list("authors", 1),
            Field::text("published"),
            Field::text("item_type"),
            Field::text("link_url"),
            Field::count("points", 1),
            Field::count("comments", 1),
            Field::count("comments_delivered", 2),
            Field::text("order"),
            Field::count("content_len", 1),
        ]),
        "a closed discussion of about 70 comments nested several levels deep, so the thread \
         must be read from the tree and ranked by the Firebase item",
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
        LiveContract::fields(&[
            Field::list("items", 10),
            Field::count("items[].rank", 1),
            Field::text("items[].ref"),
            Field::text("items[].title"),
            Field::text("items[].item_type"),
            Field::text("items[].published"),
            Field::list("items[].authors", 1).in_some_items(),
            Field::text("items[].link_url").in_some_items(),
            Field::count("items[].points", 1).in_some_items(),
            Field::count("items[].comments", 1).in_some_items(),
        ]),
        "the live front page, which mixes link stories, text posts and job ads, so items with \
         and without links, points and comments are ranked together",
    ),
];

// The 2024 presidential election, resolved and no longer changing, and its winning market,
// whose history ends in a payout of 1.
const POLYMARKET_CANARY: &str = "polymarket:presidential-election-winner-2024";
const POLYMARKET_CANARY_WINNER: &str =
    "polymarket:market/will-donald-trump-win-the-2024-us-presidential-election";

const POLYMARKET_API_LIVE: &[LiveCheck] = &[
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
        LiveContract::fields(&[
            Field::list("items", 1),
            Field::text("items[].ref"),
            Field::text("items[].title"),
            Field::text("items[].event_id"),
            Field::text("items[].status"),
            Field::count("items[].market_count", 1),
            Field::boolean("items[].neg_risk"),
            Field::text("items[].published").in_some_items(),
            Field::text("items[].end_date").in_some_items(),
            Field::number("items[].volume", 0.0).in_some_items(),
            Field::number("items[].volume_24h", 0.0).in_some_items(),
            Field::number("items[].liquidity", 0.0).in_some_items(),
            Field::list("items[].top_markets", 1).in_some_items(),
            Field::text("items[].top_markets[].ref"),
            Field::text("items[].top_markets[].label").in_some_items(),
            Field::number("items[].top_markets[].probability", 0.0).in_some_items(),
        ]),
        "markets on the Fed's rate decisions are always open and come in multi-market events, \
         so the results carry live markets ranked by probability",
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
        LiveContract::fields(&[
            Field::text("title"),
            Field::text("event_id"),
            Field::text("status"),
            Field::text("published"),
            Field::text("end_date"),
            Field::text("prices_as_of"),
            Field::number("volume", 0.0),
            Field::boolean("neg_risk"),
            Field::list("markets", 2),
            Field::text("markets[].ref"),
            Field::text("markets[].market_id"),
            Field::text("markets[].status"),
            Field::text("markets[].question").in_some_items(),
            Field::text("markets[].label").in_some_items(),
            Field::text("markets[].condition_id").in_some_items(),
            Field::text("markets[].end_date").in_some_items(),
            Field::number("markets[].volume", 0.0).in_some_items(),
            Field::list("markets[].outcomes", 2),
            Field::text("markets[].outcomes[].name"),
            Field::text("markets[].outcomes[].basis"),
            Field::number("markets[].outcomes[].price", 0.0).in_some_items(),
        ])
        .with_sample(&[SampleRule::ResolvedByPayout {
            winner: POLYMARKET_CANARY_WINNER,
        }]),
        "a resolved event of many markets, priced by their payouts, so the fetch must not ask \
         the CLOB for quotes that no longer exist; a closed event has no trading volume or \
         liquidity left to report",
    ),
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
        LiveContract::fields(&[
            Field::text("title"),
            Field::text("event_id"),
            Field::text("status"),
            Field::text("published"),
            Field::text("end_date"),
            Field::number("volume", 0.0),
            Field::number("volume_24h", 0.0),
            Field::number("liquidity", 0.0),
            Field::boolean("neg_risk"),
            Field::list("markets", 1),
            Field::text("markets[].ref"),
            Field::text("markets[].market_id"),
            Field::text("markets[].status"),
            Field::text("markets[].question").in_some_items(),
            Field::text("markets[].label").in_some_items(),
            Field::text("markets[].end_date").in_some_items(),
            Field::number("markets[].volume", 0.0).in_some_items(),
            Field::number("markets[].liquidity", 0.0).in_some_items(),
            Field::number("markets[].last_trade_price", 0.0).in_some_items(),
            Field::list("markets[].outcomes", 1),
            Field::text("markets[].outcomes[].name"),
            Field::text("markets[].outcomes[].basis"),
            Field::number("markets[].outcomes[].price", 0.0).in_some_items(),
            Field::number("markets[].outcomes[].best_bid", 0.0).in_some_items(),
            Field::number("markets[].outcomes[].best_ask", 0.0).in_some_items(),
        ])
        .with_sample(&[SampleRule::LiveQuotes]),
        "the open event the search just found, so its prices must come from the CLOB order \
         book, not from Gamma's cache",
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
        LiveContract::fields(&[
            Field::text("market.ref"),
            Field::text("market.question"),
            Field::text("market.status"),
            Field::text("outcome"),
            Field::list("points", 2),
            Field::text("points[].t"),
            Field::number("points[].price", 0.0),
            Field::text("settlement.t"),
            Field::number("settlement.price", 1.0),
        ]),
        "a market that resolved Yes, so its whole history ends in a payout of 1 that must be \
         kept out of the price series",
    ),
];

// The fetch reads a post the search found, never one chosen from memory, and never one not
// marked as safe.
const REDDIT_BROWSER_LIVE: &[LiveCheck] = &[
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
        LiveContract::fields(&[
            Field::list("items", 1),
            Field::text("items[].ref"),
            Field::text("items[].title"),
            Field::text("items[].subreddit"),
            Field::text("items[].permalink"),
            Field::text("items[].published").in_some_items(),
            Field::list("items[].authors", 1).in_some_items(),
            Field::number("items[].score", f64::MIN).in_some_items(),
            Field::count("items[].comments", 0).in_some_items(),
            Field::boolean("items[].nsfw").in_some_items(),
        ]),
        "the word names both a language and a game, so results come from several subreddits \
         and the fetch must pick one marked safe",
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
        LiveContract::fields(&[
            Field::text("title"),
            Field::text("permalink"),
            Field::text("subreddit"),
            Field::number("score", f64::MIN),
            Field::count("comments", 0),
            Field::count("comments_delivered", 0),
            Field::boolean("has_more_comments"),
            Field::text("order"),
            Field::count("content_len", 1),
        ]),
        "a post the search found today rather than a fixed one, so the thread page is whatever \
         Reddit serves now, with its comment tree",
    )
    .depends_on("reddit_browser:search")
    .time_limit(BROWSER_LIMIT),
];

const YOUTUBE_API_LIVE: &[LiveCheck] = &[
    // A search costs 100 units of the daily quota of 10,000.
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
        LiveContract::fields(&[
            Field::list("items", 1),
            Field::text("items[].ref"),
            Field::text("items[].title"),
            Field::list("items[].authors", 1),
            Field::text("items[].published"),
            Field::text("items[].channel_ref"),
            Field::text("items[].live_status"),
            Field::count("items[].duration_seconds", 1),
            Field::count("items[].views", 0),
            Field::boolean("items[].has_captions"),
            Field::text("items[].snippet").in_some_items(),
            Field::count("items[].likes", 0).in_some_items(),
            Field::count("items[].comments", 0).in_some_items(),
        ]),
        "many channels lecture on the topic, so every result needs the second request that adds \
         its duration and statistics",
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
        LiveContract::fields(&[
            Field::text("title"),
            Field::list("authors", 1),
            Field::text("published"),
            Field::text("channel_ref"),
            Field::text("live_status"),
            Field::count("duration_seconds", 1),
            Field::count("views", 1),
            Field::count("likes", 1),
            Field::count("comments", 1),
            Field::boolean("has_captions"),
        ]),
        "the first YouTube upload, whose statistics run to hundreds of millions and whose \
         metadata never changes",
    ),
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
        LiveContract::fields(&[
            Field::list("comments", 1),
            Field::text("comments[].id"),
            Field::text("comments[].published"),
            Field::count("comments[].reply_count", 0),
            Field::text("comments[].text").in_some_items(),
            Field::text("comments[].author").in_some_items(),
            Field::count("comments[].likes", 0).in_some_items(),
            Field::list("comments[].replies", 0),
            Field::text("comments[].replies[].id"),
        ]),
        "the canary video has millions of comments, so an empty page means a broken read",
    ),
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
        LiveContract::fields(&[
            Field::list("items", 5),
            Field::text("items[].ref"),
            Field::text("items[].title"),
            Field::count("items[].position", 0),
            Field::text("items[].published"),
            Field::text("items[].channel_ref"),
            Field::list("items[].authors", 1),
            Field::text("items[].snippet").in_some_items(),
        ]),
        "a finished public course of 16 videos, so a page of five is full and every item has \
         its place in the playlist",
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
    LiveContract::fields(&[
        Field::text("title"),
        Field::list("authors", 1),
        Field::text("channel_ref"),
        Field::count("duration_seconds", 1),
        Field::count("views", 1),
        Field::list("caption_tracks", 1),
        Field::text("caption_tracks[].language"),
        Field::text("caption_tracks[].source"),
        Field::text("transcript.language"),
        Field::text("transcript.source"),
        Field::count("transcript.cues", 1),
        Field::count("transcript.paragraphs", 1),
        Field::count("content_len", 1),
    ]),
    "a video with only an automatic caption track, so the read must pick the speech \
     recognition track and turn its cues into paragraphs",
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
        LiveContract::fields(&[
            Field::text("video.title"),
            Field::text("answer"),
            Field::text("model"),
            Field::count("usage.total_tokens", 1),
        ]),
        "the video's speech never names the animals' number or the speaker's clothes, so only \
         a model that watched the picture can answer",
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
        LiveContract::fields(&[
            Field::text("title"),
            Field::text("transcript.language"),
            Field::count("transcript.cues", 1),
            Field::count("transcript.paragraphs", 1),
            Field::text("model"),
            Field::count("usage.total_tokens", 1),
            Field::count("content_len", 1),
        ]),
        "the clip's only captions are automatic, so the model, not a caption track, supplies the \
         timed segments that must decode into cues and paragraphs",
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
