use std::sync::LazyLock;
use std::time::Duration;

use crate::rate_limit::AccessPolicy;
use crate::types::{
    ASK, CHATGPT_RESEARCH_RESULT, CHATGPT_RESEARCH_START, CITED_BY, COMMENTS,
    GEMINI_RESEARCH_RESULT, GEMINI_RESEARCH_START, Platform, STORIES, TRANSCRIBE, VIDEOS,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CapabilityCatalog {
    pub(crate) seam: &'static str,
    pub(crate) providers: &'static [ProviderId],
}

pub(crate) const MAIN_SEARCH: CapabilityCatalog = CapabilityCatalog {
    seam: "main_search",
    providers: &[ProviderId::Xai],
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
    providers: &[ProviderId::Exa, ProviderId::Context7],
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
    ARXIV, HN, PLATFORMS, PlatformCatalog, PlatformOperation, SCHOLAR, SSRN, X, XIAOHONGSHU,
    YOUTUBE, http_opt_in, platform,
};

pub(crate) fn supports(capability: &str, provider: &str) -> bool {
    let Some(id) = ProviderId::parse(provider) else {
        return false;
    };
    by_seam(capability).is_some_and(|catalog| catalog.contains(id))
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum ProviderId {
    Xai,
    Exa,
    Tavily,
    Firecrawl,
    Jina,
    Context7,
    ArxivApi,
    SsrnCrossref,
    SsrnBrowser,
    Serpapi,
    XiaohongshuBrowser,
    HnApi,
    YoutubeApi,
    YoutubeWeb,
    GeminiApi,
    GeminiBrowser,
    ChatgptBrowser,
}

impl ProviderId {
    pub(crate) const ALL: [Self; 17] = [
        Self::Xai,
        Self::Exa,
        Self::Tavily,
        Self::Firecrawl,
        Self::Jina,
        Self::Context7,
        Self::ArxivApi,
        Self::SsrnCrossref,
        Self::SsrnBrowser,
        Self::Serpapi,
        Self::XiaohongshuBrowser,
        Self::HnApi,
        Self::YoutubeApi,
        Self::YoutubeWeb,
        Self::GeminiApi,
        Self::GeminiBrowser,
        Self::ChatgptBrowser,
    ];

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "xai" => Some(Self::Xai),
            "exa" => Some(Self::Exa),
            "tavily" => Some(Self::Tavily),
            "firecrawl" => Some(Self::Firecrawl),
            "jina" => Some(Self::Jina),
            "context7" => Some(Self::Context7),
            "arxiv_api" => Some(Self::ArxivApi),
            "ssrn_crossref" => Some(Self::SsrnCrossref),
            "ssrn_browser" => Some(Self::SsrnBrowser),
            "serpapi" => Some(Self::Serpapi),
            "xiaohongshu_browser" => Some(Self::XiaohongshuBrowser),
            "hn_api" => Some(Self::HnApi),
            "youtube_api" => Some(Self::YoutubeApi),
            "youtube_web" => Some(Self::YoutubeWeb),
            "gemini_api" => Some(Self::GeminiApi),
            "gemini_browser" => Some(Self::GeminiBrowser),
            "chatgpt_browser" => Some(Self::ChatgptBrowser),
            _ => None,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Xai => "xai",
            Self::Exa => "exa",
            Self::Tavily => "tavily",
            Self::Firecrawl => "firecrawl",
            Self::Jina => "jina",
            Self::Context7 => "context7",
            Self::ArxivApi => "arxiv_api",
            Self::SsrnCrossref => "ssrn_crossref",
            Self::SsrnBrowser => "ssrn_browser",
            Self::Serpapi => "serpapi",
            Self::XiaohongshuBrowser => "xiaohongshu_browser",
            Self::HnApi => "hn_api",
            Self::YoutubeApi => "youtube_api",
            Self::YoutubeWeb => "youtube_web",
            Self::GeminiApi => "gemini_api",
            Self::GeminiBrowser => "gemini_browser",
            Self::ChatgptBrowser => "chatgpt_browser",
        }
    }
}

/// How forager reaches a provider. Configuration checks and doctor branch on it, never on a
/// provider ID.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProviderTransport {
    /// HTTP requests to the configured `url`.
    Http,
    /// Commands of a forager-owned OpenCLI adapter, run through the configured `command`.
    OpenCli(OpenCliAdapter),
}

/// A forager-owned OpenCLI adapter: the site whose commands it installs, and the contract
/// version of the output envelope every command answers with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OpenCliAdapter {
    pub(crate) site: &'static str,
    pub(crate) contract: &'static str,
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
    /// Runs the OpenCLI adapter's read-only `status` command: it proves OpenCLI, the adapter
    /// contract, and the site login, never an operation.
    AdapterStatus {
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
    pub(crate) operations: &'static [&'static str],
    pub(crate) credentials: Credentials,
    pub(crate) transport: ProviderTransport,
    /// The pacing every send to the provider must follow; a process route paces each OpenCLI
    /// command, not each HTTP request the browser sends.
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
const CONTEXT7_SMOKE: &[ProviderSmokeCase] = &[
    ProviderSmokeCase {
        id: "C10",
        platform: None,
        operation: "library_resolve",
        transport: "mcp",
    },
    ProviderSmokeCase {
        id: "C11",
        platform: None,
        operation: "docs",
        transport: "mcp",
    },
];
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
        transport: "process",
    },
    ProviderSmokeCase {
        id: "C28",
        platform: Some(Platform::Xiaohongshu),
        operation: "fetch",
        transport: "process",
    },
    ProviderSmokeCase {
        id: "C29",
        platform: Some(Platform::Xiaohongshu),
        operation: COMMENTS,
        transport: "process",
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

// One browser operation every five seconds keeps the pace close to a person's (ADR 0020).
const SSRN_BROWSER_ACCESS: AccessPolicy = AccessPolicy {
    min_interval: Duration::from_secs(5),
    max_concurrency: 1,
};

// About 55 read-only commands 5 to 10 seconds apart raised no risk control on 2026-10-08; ten
// seconds is the slowest pace measured, not a limit the site states (ADR 0020).
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

// The Data API meters quota, not pace, and failed requests still cost quota; one request per
// second keeps a runaway loop from draining the daily search budget in moments.
const YOUTUBE_API_ACCESS: AccessPolicy = AccessPolicy {
    min_interval: Duration::from_secs(1),
    max_concurrency: 1,
};

// The yt-dlp wiki's anonymous pace is about 300 videos an hour, 5 to 10 seconds apart; one
// request every four seconds keeps a fetch's three requests near it. Going faster risks the
// user's egress IP (ADR 0028).
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

// The Gemini page itself polls its conversation about every ten seconds while a research runs
// (2026-10-09), so one command every ten seconds stays at a person's pace (ADR 0020).
const GEMINI_BROWSER_ACCESS: AccessPolicy = AccessPolicy {
    min_interval: Duration::from_secs(10),
    max_concurrency: 1,
};

// One command every ten seconds keeps reading a ChatGPT conversation at a person's pace, a
// condition of accepting the OpenAI terms risk (ADR 0024).
const CHATGPT_BROWSER_ACCESS: AccessPolicy = AccessPolicy {
    min_interval: Duration::from_secs(10),
    max_concurrency: 1,
};

const REGISTRY: &[ProviderRegistration] = &[
    ProviderRegistration {
        id: ProviderId::Xai,
        operations: &[],
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
        id: ProviderId::Tavily,
        operations: &["site_map"],
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
        operations: &[],
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
        operations: &[],
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
        id: ProviderId::Context7,
        operations: &[],
        credentials: Credentials::Required,
        transport: ProviderTransport::Http,
        access_policy: None,
        probe: DoctorProbe::DocsSearch {
            name: "library",
            transport: "mcp",
        },
        smoke_cases: CONTEXT7_SMOKE,
        native_full_text: false,
    },
    ProviderRegistration {
        id: ProviderId::Exa,
        operations: &["similar"],
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
        operations: &[],
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
        operations: &[],
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
    // No live smoke case: SSRN's Cloudflare check fails too often for an unattended run.
    ProviderRegistration {
        id: ProviderId::SsrnBrowser,
        operations: &[],
        credentials: Credentials::Unused,
        transport: ProviderTransport::OpenCli(OpenCliAdapter {
            site: "ssrn",
            contract: "forager-ssrn/4",
        }),
        access_policy: Some(SSRN_BROWSER_ACCESS),
        probe: DoctorProbe::PlatformSearch {
            platform: Platform::Ssrn,
            name: "search",
            transport: "process",
        },
        smoke_cases: &[],
        native_full_text: false,
    },
    // SerpApi limits throughput per account; a 429 rotates to the next key instead of pacing.
    ProviderRegistration {
        id: ProviderId::Serpapi,
        operations: &[],
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
        operations: &[],
        credentials: Credentials::Unused,
        transport: ProviderTransport::OpenCli(OpenCliAdapter {
            site: "forager-xhs",
            contract: "forager-xhs/2",
        }),
        access_policy: Some(XIAOHONGSHU_BROWSER_ACCESS),
        probe: DoctorProbe::PlatformSearch {
            platform: Platform::Xiaohongshu,
            name: "search",
            transport: "process",
        },
        smoke_cases: XIAOHONGSHU_BROWSER_SMOKE,
        native_full_text: true,
    },
    // A full-text fetch of a link story reads the linked page through Web Fetch.
    ProviderRegistration {
        id: ProviderId::HnApi,
        operations: &[],
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
    // A deep doctor spends one search of the separate daily search quota.
    ProviderRegistration {
        id: ProviderId::YoutubeApi,
        operations: &[],
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
        operations: &[],
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
        operations: &[],
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
    // Only `forager gemini` commands run it, never a chain. No live smoke case exists: starting
    // a research spends the user's quota, and reading one needs a conversation of the user.
    ProviderRegistration {
        id: ProviderId::GeminiBrowser,
        operations: &[GEMINI_RESEARCH_START, GEMINI_RESEARCH_RESULT],
        credentials: Credentials::Unused,
        transport: ProviderTransport::OpenCli(OpenCliAdapter {
            site: "forager-gemini",
            contract: "forager-gemini/2",
        }),
        access_policy: Some(GEMINI_BROWSER_ACCESS),
        probe: DoctorProbe::AdapterStatus {
            name: "status",
            transport: "process",
        },
        smoke_cases: &[],
        native_full_text: false,
    },
    // Like `gemini_browser`: only `forager chatgpt` commands run it, and it has no live smoke
    // case.
    ProviderRegistration {
        id: ProviderId::ChatgptBrowser,
        operations: &[CHATGPT_RESEARCH_START, CHATGPT_RESEARCH_RESULT],
        credentials: Credentials::Unused,
        transport: ProviderTransport::OpenCli(OpenCliAdapter {
            site: "forager-chatgpt",
            contract: "forager-chatgpt/1",
        }),
        access_policy: Some(CHATGPT_BROWSER_ACCESS),
        probe: DoctorProbe::AdapterStatus {
            name: "status",
            transport: "process",
        },
        smoke_cases: &[],
        native_full_text: false,
    },
];

/// Returns whether a registration is legitimate: it belongs to a capability catalog or a
/// platform catalog, or it owns an operation.
pub(crate) fn has_owner(registration: &ProviderRegistration) -> bool {
    CATALOGS
        .iter()
        .any(|catalog| catalog.contains(registration.id))
        || PLATFORMS
            .iter()
            .any(|platform| platform.contains(registration.id))
        || !registration.operations.is_empty()
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
                "{} belongs to no capability or platform and owns no operation",
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
    use std::collections::BTreeSet;

    use serde::Deserialize;

    use super::{
        CATALOGS, Credentials, DOCS_SEARCH, DoctorProbe, MAIN_SEARCH, PLATFORMS, PlatformOperation,
        ProviderId, ProviderRegistration, ProviderTransport, REGISTRY, WEB_FETCH, WEB_SEARCH,
        has_owner, platform, registration, registrations, validate_registrations,
    };

    /// Providers without a live smoke case. `gemini_browser` and `chatgpt_browser` get only the
    /// offline registration check: a live case would spend the user's Deep Research quota or read
    /// their conversations.
    const WITHOUT_LIVE_SMOKE: &[ProviderId] = &[
        ProviderId::SsrnBrowser,
        ProviderId::GeminiBrowser,
        ProviderId::ChatgptBrowser,
    ];

    #[derive(Deserialize)]
    struct AcceptanceManifest {
        transport_fixtures: Vec<TransportFixture>,
    }

    #[derive(Deserialize)]
    struct TransportFixture {
        provider: String,
        seam: String,
        test: String,
    }

    /// Returns whether a seam names a platform operation that no platform catalog lists.
    fn is_single_route_operation_seam(seam: &str) -> bool {
        seam.strip_prefix("platform:")
            .and_then(|rest| rest.split_once(':'))
            .is_some_and(|(_, operation)| {
                !PlatformOperation::ALL
                    .iter()
                    .any(|catalog_operation| catalog_operation.as_str() == operation)
            })
    }

    #[test]
    fn provider_fixture_projection_matches_transport_manifest() {
        let manifest: AcceptanceManifest = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/acceptance-manifest.json"
        )))
        .expect("acceptance manifest");
        let capability_projection = CATALOGS.iter().flat_map(|catalog| {
            catalog
                .providers
                .iter()
                .map(move |provider| (provider.name().to_owned(), catalog.seam.to_owned()))
        });
        let platform_projection = PLATFORMS.iter().flat_map(|catalog| {
            PlatformOperation::ALL
                .into_iter()
                .flat_map(move |operation| {
                    catalog.routes(operation).iter().map(move |route| {
                        (
                            route.name().to_owned(),
                            format!("platform:{}:{}", catalog.platform, operation.as_str()),
                        )
                    })
                })
        });
        let registry = capability_projection
            .chain(platform_projection)
            .collect::<BTreeSet<_>>();
        // Single-route platform operations live outside the catalog; the platform checklist
        // checks their fixtures.
        let fixture_projection = manifest
            .transport_fixtures
            .iter()
            .filter(|fixture| !is_single_route_operation_seam(&fixture.seam))
            .map(|fixture| (fixture.provider.clone(), fixture.seam.clone()))
            .collect::<BTreeSet<_>>();

        assert_eq!(fixture_projection, registry);
        for fixture in manifest.transport_fixtures {
            assert!(
                !fixture.test.trim().is_empty(),
                "{} / {} lacks a fixture test",
                fixture.provider,
                fixture.seam
            );
        }
    }

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
                DoctorProbe::AdapterStatus { .. } => {
                    matches!(registration.transport, ProviderTransport::OpenCli(_))
                }
            };
            assert!(probe_is_supported, "{} probe", registration.id.name());
            assert_eq!(
                registration.smoke_cases.is_empty(),
                WITHOUT_LIVE_SMOKE.contains(&registration.id),
                "{} smoke",
                registration.id.name()
            );
        }
    }

    #[test]
    fn a_provider_outside_every_catalog_is_registered_through_its_operations() {
        let registered = [ProviderId::GeminiBrowser, ProviderId::ChatgptBrowser].map(|id| {
            let registration = registration(id);
            (
                CATALOGS.iter().any(|catalog| catalog.contains(id)),
                PLATFORMS.iter().any(|catalog| catalog.contains(id)),
                registration.operations,
                has_owner(registration),
            )
        });

        assert_eq!(
            registered,
            [
                (
                    false,
                    false,
                    &["gemini_research_start", "gemini_research_result"][..],
                    true
                ),
                (
                    false,
                    false,
                    &["chatgpt_research_start", "chatgpt_research_result"][..],
                    true
                ),
            ]
        );
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
        let arxiv_api = registration(ProviderId::ArxivApi);

        assert_eq!(
            (
                CATALOGS
                    .iter()
                    .any(|catalog| catalog.contains(ProviderId::ArxivApi)),
                arxiv_api.operations.is_empty(),
                validate_registrations(REGISTRY),
            ),
            (false, true, Ok(()))
        );
    }

    #[test]
    fn registry_validation_accepts_registrations_without_credentials() {
        let mut registry = REGISTRY.to_vec();
        registry[0].credentials = Credentials::Unused;

        assert!(validate_registrations(&registry).is_ok());
    }
}
