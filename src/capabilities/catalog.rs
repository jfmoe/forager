use std::sync::LazyLock;
use std::time::Duration;

use crate::rate_limit::AccessPolicy;
use crate::types::{ASK, CITED_BY, COMMENTS, HISTORY, Platform, STORIES, TRANSCRIBE, VIDEOS};

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

#[path = "platform_catalog.rs"]
mod platform_catalog;

pub(crate) use platform_catalog::{
    ARXIV, HN, PLATFORMS, POLYMARKET, PlatformCatalog, PlatformOperation, REDDIT, SCHOLAR, SSRN, X,
    XIAOHONGSHU, YOUTUBE, config_set_hint, opt_in_risk, platform, route_platform,
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DoctorProbe {
    MainSearch {
        name: &'static str,
        transport: &'static str,
    },
    WebSearch {
        name: &'static str,
        transport: &'static str,
    },
    WebFetch {
        name: &'static str,
        transport: &'static str,
    },
    DocsSearch {
        name: &'static str,
        transport: &'static str,
    },
    PlatformSearch {
        platform: Platform,
        name: &'static str,
        transport: &'static str,
    },
    /// Fetches a fixed canary item of the platform at `metadata` depth, for a route that
    /// serves fetch only.
    PlatformFetch {
        platform: Platform,
        canary: &'static str,
        name: &'static str,
        transport: &'static str,
    },
    /// Asks the service's account endpoint about every configured key; it proves the endpoint
    /// and keys, not any operation's request or decoding.
    ServiceAccount {
        name: &'static str,
        transport: &'static str,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ProviderSmokeCase {
    pub(crate) id: &'static str,
    /// The platform whose operation the case exercises, for a platform route.
    pub(crate) platform: Option<Platform>,
    pub(crate) operation: &'static str,
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
    pub(crate) probe: DoctorProbe,
    pub(crate) smoke_cases: &'static [ProviderSmokeCase],
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

const XAI_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C01",
        platform: None,
        operation: "main_search",
        transport: "sse",
    },
    ProviderSmokeCase {
        id: "C30",
        platform: Some(Platform::X),
        operation: "search",
        transport: "sse",
    },
    ProviderSmokeCase {
        id: "C31",
        platform: Some(Platform::X),
        operation: "fetch",
        transport: "sse",
    },
    ProviderSmokeCase {
        id: "C32",
        platform: Some(Platform::X),
        operation: ASK,
        transport: "sse",
    },
];
const OPENAI_SMOKE: &[ProviderSmokeCase] = &[ProviderSmokeCase {
    id: "C49",
    platform: None,
    operation: "main_search",
    transport: "sse",
}];

const TAVILY_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C05",
        platform: None,
        operation: "web_search",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C08",
        platform: None,
        operation: "web_fetch",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C17",
        platform: None,
        operation: "site_map",
        transport: "http",
    },
];
const FIRECRAWL_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C06",
        platform: None,
        operation: "web_search",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C09",
        platform: None,
        operation: "web_fetch",
        transport: "http",
    },
];
const JINA_SMOKE: &[ProviderSmokeCase] = &[ProviderSmokeCase {
    id: "C07",
    platform: None,
    operation: "web_fetch",
    transport: "http",
}];
const EXA_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C12",
        platform: None,
        operation: "docs_search",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C13",
        platform: None,
        operation: "similar",
        transport: "http",
    },
];

const ARXIV_API_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C18",
        platform: Some(Platform::Arxiv),
        operation: "search",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C19",
        platform: Some(Platform::Arxiv),
        operation: "fetch",
        transport: "http",
    },
];

const SSRN_CROSSREF_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C20",
        platform: Some(Platform::Ssrn),
        operation: "search",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C21",
        platform: Some(Platform::Ssrn),
        operation: "fetch",
        transport: "http",
    },
];

const SERPAPI_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C24",
        platform: Some(Platform::Scholar),
        operation: "search",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C25",
        platform: Some(Platform::Scholar),
        operation: "fetch",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C26",
        platform: Some(Platform::Scholar),
        operation: CITED_BY,
        transport: "http",
    },
];

const XIAOHONGSHU_BROWSER_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C27",
        platform: Some(Platform::Xiaohongshu),
        operation: "search",
        transport: "browser",
    },
    ProviderSmokeCase {
        id: "C28",
        platform: Some(Platform::Xiaohongshu),
        operation: "fetch",
        transport: "browser",
    },
    ProviderSmokeCase {
        id: "C29",
        platform: Some(Platform::Xiaohongshu),
        operation: COMMENTS,
        transport: "browser",
    },
];

const HN_API_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C33",
        platform: Some(Platform::Hn),
        operation: "search",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C34",
        platform: Some(Platform::Hn),
        operation: "fetch",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C35",
        platform: Some(Platform::Hn),
        operation: STORIES,
        transport: "http",
    },
];

const POLYMARKET_API_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C43",
        platform: Some(Platform::Polymarket),
        operation: "search",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C44",
        platform: Some(Platform::Polymarket),
        operation: "fetch",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C45",
        platform: Some(Platform::Polymarket),
        operation: "fetch",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C46",
        platform: Some(Platform::Polymarket),
        operation: HISTORY,
        transport: "http",
    },
];

const REDDIT_BROWSER_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C50",
        platform: Some(Platform::Reddit),
        operation: "search",
        transport: "browser",
    },
    ProviderSmokeCase {
        id: "C51",
        platform: Some(Platform::Reddit),
        operation: "fetch",
        transport: "browser",
    },
];

const YOUTUBE_API_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C36",
        platform: Some(Platform::Youtube),
        operation: "search",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C37",
        platform: Some(Platform::Youtube),
        operation: "fetch",
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C39",
        platform: Some(Platform::Youtube),
        operation: COMMENTS,
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C40",
        platform: Some(Platform::Youtube),
        operation: VIDEOS,
        transport: "http",
    },
];

const YOUTUBE_WEB_SMOKE: &[ProviderSmokeCase] = &[ProviderSmokeCase {
    id: "C38",
    platform: Some(Platform::Youtube),
    operation: "fetch",
    transport: "http",
}];

const GEMINI_API_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C41",
        platform: Some(Platform::Youtube),
        operation: ASK,
        transport: "http",
    },
    ProviderSmokeCase {
        id: "C42",
        platform: Some(Platform::Youtube),
        operation: TRANSCRIBE,
        transport: "http",
    },
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
        probe: DoctorProbe::MainSearch {
            name: "responses",
            transport: "sse",
        },
        smoke_cases: XAI_SMOKE,
        native_full_text: false,
    },
    ProviderRegistration {
        id: ProviderId::Openai,
        credentials: Credentials::Required,
        transport: ProviderTransport::Http,
        access_policy: None,
        probe: DoctorProbe::MainSearch {
            name: "responses",
            transport: "sse",
        },
        smoke_cases: OPENAI_SMOKE,
        native_full_text: false,
    },
    ProviderRegistration {
        id: ProviderId::Tavily,
        credentials: Credentials::Required,
        transport: ProviderTransport::Http,
        access_policy: None,
        probe: DoctorProbe::WebSearch {
            name: "search",
            transport: "http",
        },
        smoke_cases: TAVILY_SMOKE,
        native_full_text: false,
    },
    ProviderRegistration {
        id: ProviderId::Firecrawl,
        credentials: Credentials::Required,
        transport: ProviderTransport::Http,
        access_policy: None,
        probe: DoctorProbe::WebSearch {
            name: "search",
            transport: "http",
        },
        smoke_cases: FIRECRAWL_SMOKE,
        native_full_text: false,
    },
    ProviderRegistration {
        id: ProviderId::Jina,
        credentials: Credentials::Optional,
        transport: ProviderTransport::Http,
        access_policy: None,
        probe: DoctorProbe::WebFetch {
            name: "fetch",
            transport: "http",
        },
        smoke_cases: JINA_SMOKE,
        native_full_text: false,
    },
    ProviderRegistration {
        id: ProviderId::Exa,
        credentials: Credentials::Required,
        transport: ProviderTransport::Http,
        access_policy: None,
        probe: DoctorProbe::DocsSearch {
            name: "search",
            transport: "http",
        },
        smoke_cases: EXA_SMOKE,
        native_full_text: false,
    },
    ProviderRegistration {
        id: ProviderId::ArxivApi,
        credentials: Credentials::Unused,
        transport: ProviderTransport::Http,
        access_policy: Some(ARXIV_API_ACCESS),
        probe: DoctorProbe::PlatformSearch {
            platform: Platform::Arxiv,
            name: "search",
            transport: "http",
        },
        smoke_cases: ARXIV_API_SMOKE,
        native_full_text: false,
    },
    ProviderRegistration {
        id: ProviderId::SsrnCrossref,
        credentials: Credentials::Unused,
        transport: ProviderTransport::Http,
        access_policy: Some(SSRN_CROSSREF_ACCESS),
        probe: DoctorProbe::PlatformSearch {
            platform: Platform::Ssrn,
            name: "search",
            transport: "http",
        },
        smoke_cases: SSRN_CROSSREF_SMOKE,
        native_full_text: false,
    },
    // SerpApi limits throughput per account; a 429 rotates to the next key instead of pacing.
    ProviderRegistration {
        id: ProviderId::Serpapi,
        credentials: Credentials::Required,
        transport: ProviderTransport::Http,
        access_policy: None,
        probe: DoctorProbe::ServiceAccount {
            name: "account",
            transport: "http",
        },
        smoke_cases: SERPAPI_SMOKE,
        native_full_text: false,
    },
    ProviderRegistration {
        id: ProviderId::XiaohongshuBrowser,
        credentials: Credentials::Unused,
        transport: ProviderTransport::Browser(BrowserEnvironment { language: "zh-CN" }),
        access_policy: Some(XIAOHONGSHU_BROWSER_ACCESS),
        probe: DoctorProbe::PlatformSearch {
            platform: Platform::Xiaohongshu,
            name: "search",
            transport: "browser",
        },
        smoke_cases: XIAOHONGSHU_BROWSER_SMOKE,
        native_full_text: true,
    },
    // A full-text fetch of a link story reads the linked page through Web Fetch.
    ProviderRegistration {
        id: ProviderId::HnApi,
        credentials: Credentials::Unused,
        transport: ProviderTransport::Http,
        access_policy: Some(HN_API_ACCESS),
        probe: DoctorProbe::PlatformSearch {
            platform: Platform::Hn,
            name: "search",
            transport: "http",
        },
        smoke_cases: HN_API_SMOKE,
        native_full_text: false,
    },
    // Search and fetch read only official keyless endpoints; a fetch never needs Web Fetch.
    ProviderRegistration {
        id: ProviderId::PolymarketApi,
        credentials: Credentials::Unused,
        transport: ProviderTransport::Http,
        access_policy: Some(POLYMARKET_API_ACCESS),
        probe: DoctorProbe::PlatformSearch {
            platform: Platform::Polymarket,
            name: "search",
            transport: "http",
        },
        smoke_cases: POLYMARKET_API_SMOKE,
        native_full_text: false,
    },
    // A full-text fetch of a link post reads the linked page through Web Fetch.
    ProviderRegistration {
        id: ProviderId::RedditBrowser,
        credentials: Credentials::Unused,
        transport: ProviderTransport::Browser(BrowserEnvironment { language: "en-US" }),
        access_policy: Some(REDDIT_BROWSER_ACCESS),
        probe: DoctorProbe::PlatformSearch {
            platform: Platform::Reddit,
            name: "search",
            transport: "browser",
        },
        smoke_cases: REDDIT_BROWSER_SMOKE,
        native_full_text: false,
    },
    // A deep doctor spends one search of the separate daily search quota.
    ProviderRegistration {
        id: ProviderId::YoutubeApi,
        credentials: Credentials::Required,
        transport: ProviderTransport::Http,
        access_policy: Some(YOUTUBE_API_ACCESS),
        probe: DoctorProbe::PlatformSearch {
            platform: Platform::Youtube,
            name: "search",
            transport: "http",
        },
        smoke_cases: YOUTUBE_API_SMOKE,
        native_full_text: false,
    },
    // It serves fetch only, so doctor probes it with a fetch of the canary video. Its full
    // text is the transcript it renders itself.
    ProviderRegistration {
        id: ProviderId::YoutubeWeb,
        credentials: Credentials::Unused,
        transport: ProviderTransport::Http,
        access_policy: Some(YOUTUBE_WEB_ACCESS),
        // The first YouTube upload (2005), whose metadata no longer changes.
        probe: DoctorProbe::PlatformFetch {
            platform: Platform::Youtube,
            canary: "youtube:jNQXAC9IVRw",
            name: "fetch",
            transport: "http",
        },
        smoke_cases: YOUTUBE_WEB_SMOKE,
        native_full_text: true,
    },
    // It serves Ask and Transcribe only, so doctor asks the model list about every key, which
    // spends no model quota.
    ProviderRegistration {
        id: ProviderId::GeminiApi,
        credentials: Credentials::Required,
        transport: ProviderTransport::Http,
        access_policy: Some(GEMINI_API_ACCESS),
        probe: DoctorProbe::ServiceAccount {
            name: "models",
            transport: "http",
        },
        smoke_cases: GEMINI_API_SMOKE,
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
    let smoke_ids = registry
        .iter()
        .flat_map(|registration| registration.smoke_cases.iter().map(|case| case.id))
        .collect::<Vec<_>>();
    let unique_smoke_ids = smoke_ids
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    if smoke_ids.len() != unique_smoke_ids.len() {
        return Err("provider smoke case IDs must be unique".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        CATALOGS, Credentials, DOCS_SEARCH, DoctorProbe, MAIN_SEARCH, ProviderId,
        ProviderRegistration, REGISTRY, WEB_FETCH, WEB_SEARCH, has_owner, platform, registration,
        registrations, validate_registrations,
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
    fn catalogs_project_every_registration_probe_and_smoke_case_consistently() {
        for registration in registrations() {
            assert!(has_owner(registration), "{} owner", registration.id.name());
            let probe_is_supported = match registration.probe {
                DoctorProbe::MainSearch { .. } => MAIN_SEARCH.contains(registration.id),
                DoctorProbe::WebSearch { .. } => WEB_SEARCH.contains(registration.id),
                DoctorProbe::WebFetch { .. } => WEB_FETCH.contains(registration.id),
                DoctorProbe::DocsSearch { .. } => DOCS_SEARCH.contains(registration.id),
                DoctorProbe::PlatformSearch {
                    platform: probed, ..
                } => platform(probed).search.contains(&registration.id),
                DoctorProbe::PlatformFetch {
                    platform: probed, ..
                } => platform(probed).fetch.contains(&registration.id),
                DoctorProbe::ServiceAccount { .. } => {
                    registration.credentials == Credentials::Required
                }
            };
            assert!(probe_is_supported, "{} probe", registration.id.name());
            assert!(
                !registration.smoke_cases.is_empty(),
                "{} smoke",
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

        assert!(validate_registrations(&registry).is_ok());
    }
}
