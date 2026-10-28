//! The `[providers.*]` sections of the configuration file: one section per provider, with the
//! defaults each key takes when the file omits it.

use std::marker::PhantomData;

use serde::{Deserialize, Serialize};

use super::deserialize_integer;
use crate::redact::Secret;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Providers {
    pub(crate) xai: Xai,
    pub(crate) openai: Openai,
    pub(crate) exa: Endpoint<ExaEndpoint>,
    pub(crate) context7: Endpoint<Context7Endpoint>,
    pub(crate) jina: Jina,
    pub(crate) tavily: Endpoint<TavilyEndpoint>,
    pub(crate) firecrawl: Endpoint<FirecrawlEndpoint>,
    pub(crate) arxiv_api: AnonymousEndpoint<ArxivApiEndpoint>,
    pub(crate) ssrn_crossref: AnonymousEndpoint<SsrnCrossrefEndpoint>,
    pub(crate) ssrn_browser: ProcessRoute<SsrnBrowserProcess>,
    pub(crate) serpapi: Endpoint<SerpapiEndpoint>,
    pub(crate) xiaohongshu_browser: ProcessRoute<XiaohongshuBrowserProcess>,
    pub(crate) hn_api: HnApi,
    pub(crate) youtube_api: Endpoint<YoutubeApiEndpoint>,
    pub(crate) youtube_web: AnonymousEndpoint<YoutubeWebEndpoint>,
    pub(crate) gemini_api: GeminiApi,
    pub(crate) gemini_browser: ProcessRoute<GeminiBrowserProcess>,
    pub(crate) chatgpt_browser: ProcessRoute<ChatgptBrowserProcess>,
    pub(crate) polymarket_api: PolymarketApi,
    pub(crate) reddit_web: RedditWeb,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Xai {
    pub(crate) url: String,
    pub(crate) keys: Vec<Secret>,
    pub(crate) model: String,
    pub(crate) tools: Vec<String>,
}

impl Default for Xai {
    fn default() -> Self {
        Self {
            url: "https://api.x.ai/v1".into(),
            keys: Vec::new(),
            model: "grok-4-fast".into(),
            tools: vec!["web_search".into(), "x_search".into()],
        }
    }
}

/// The `openai` main-search section: an OpenAI Responses endpoint, the official API or a gateway
/// that speaks its protocol.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Openai {
    pub(crate) url: String,
    pub(crate) keys: Vec<Secret>,
    pub(crate) model: String,
}

// The default model is the name CLIProxyAPI gives a Codex account's model; the official API
// names its models differently.
impl Default for Openai {
    fn default() -> Self {
        Self {
            url: "https://api.openai.com/v1".into(),
            keys: Vec::new(),
            model: "gpt-6.1-terra".into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Endpoint<D: EndpointDefaults> {
    pub(crate) url: String,
    pub(crate) keys: Vec<Secret>,
    #[serde(deserialize_with = "deserialize_integer")]
    pub(crate) timeout: u64,
    #[serde(skip)]
    pub(crate) defaults: PhantomData<D>,
}

impl<D: EndpointDefaults> Default for Endpoint<D> {
    fn default() -> Self {
        Self {
            url: D::URL.into(),
            keys: Vec::new(),
            timeout: D::TIMEOUT_SECONDS,
            defaults: PhantomData,
        }
    }
}

/// The endpoint section of a provider whose registration requires no credentials.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct AnonymousEndpoint<D: EndpointDefaults> {
    pub(crate) url: String,
    #[serde(deserialize_with = "deserialize_integer")]
    pub(crate) timeout: u64,
    #[serde(skip)]
    pub(crate) defaults: PhantomData<D>,
}

impl<D: EndpointDefaults> Default for AnonymousEndpoint<D> {
    fn default() -> Self {
        Self {
            url: D::URL.into(),
            timeout: D::TIMEOUT_SECONDS,
            defaults: PhantomData,
        }
    }
}

/// The section of a route that runs a local OpenCLI command instead of sending HTTP requests.
/// The configuration selects only the executable; the route owns every argument (ADR 0019).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct ProcessRoute<D: ProcessDefaults> {
    pub(crate) command: String,
    #[serde(deserialize_with = "deserialize_integer")]
    pub(crate) timeout: u64,
    #[serde(skip)]
    pub(crate) defaults: PhantomData<D>,
}

impl<D: ProcessDefaults> Default for ProcessRoute<D> {
    fn default() -> Self {
        Self {
            command: "opencli".into(),
            timeout: D::TIMEOUT_SECONDS,
            defaults: PhantomData,
        }
    }
}

pub(crate) trait ProcessDefaults {
    const TIMEOUT_SECONDS: u64;
}

#[derive(Clone, Debug)]
pub(crate) struct SsrnBrowserProcess;

impl ProcessDefaults for SsrnBrowserProcess {
    const TIMEOUT_SECONDS: u64 = 90;
}

#[derive(Clone, Debug)]
pub(crate) struct XiaohongshuBrowserProcess;

// One command opens the results page in about 20 seconds and then reads up to four more pages.
impl ProcessDefaults for XiaohongshuBrowserProcess {
    const TIMEOUT_SECONDS: u64 = 120;
}

#[derive(Clone, Debug)]
pub(crate) struct GeminiBrowserProcess;

// Starting a research waits for the plan and then for the confirmed turn, about a minute live.
impl ProcessDefaults for GeminiBrowserProcess {
    const TIMEOUT_SECONDS: u64 = 240;
}

#[derive(Clone, Debug)]
pub(crate) struct ChatgptBrowserProcess;

// The same budget as Gemini: starting a research sends once and then waits for the plan card.
impl ProcessDefaults for ChatgptBrowserProcess {
    const TIMEOUT_SECONDS: u64 = 240;
}

pub(crate) trait EndpointDefaults {
    const URL: &'static str;
    const TIMEOUT_SECONDS: u64;
}

macro_rules! endpoint_defaults {
    ($name:ident, $url:literal) => {
        endpoint_defaults!($name, $url, 30);
    };
    ($name:ident, $url:literal, $timeout:literal) => {
        #[derive(Clone, Debug)]
        pub(crate) struct $name;

        impl EndpointDefaults for $name {
            const URL: &'static str = $url;
            const TIMEOUT_SECONDS: u64 = $timeout;
        }
    };
}

endpoint_defaults!(ExaEndpoint, "https://api.exa.ai");
endpoint_defaults!(Context7Endpoint, "https://mcp.context7.com/mcp");
endpoint_defaults!(TavilyEndpoint, "https://api.tavily.com");
// Matches the 60 s server-side timeout that Web Fetch requests from Firecrawl (ADR 0018).
endpoint_defaults!(FirecrawlEndpoint, "https://api.firecrawl.dev/v2", 60);
endpoint_defaults!(ArxivApiEndpoint, "https://export.arxiv.org/api/query");
endpoint_defaults!(SsrnCrossrefEndpoint, "https://api.crossref.org");
endpoint_defaults!(SerpapiEndpoint, "https://serpapi.com/search.json");
endpoint_defaults!(YoutubeApiEndpoint, "https://www.googleapis.com/youtube/v3");
endpoint_defaults!(YoutubeWebEndpoint, "https://www.youtube.com");

/// The `hn_api` route section: one URL per official Hacker News endpoint the route combines.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct HnApi {
    pub(crate) url: String,
    pub(crate) firebase_url: String,
    #[serde(deserialize_with = "deserialize_integer")]
    pub(crate) timeout: u64,
}

impl Default for HnApi {
    fn default() -> Self {
        Self {
            url: "https://hn.algolia.com/api/v1".into(),
            firebase_url: "https://hacker-news.firebaseio.com/v0".into(),
            timeout: 30,
        }
    }
}

/// The `polymarket_api` route section: one URL per official Polymarket endpoint the route
/// combines.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct PolymarketApi {
    pub(crate) url: String,
    pub(crate) clob_url: String,
    pub(crate) data_url: String,
    #[serde(deserialize_with = "deserialize_integer")]
    pub(crate) timeout: u64,
}

impl Default for PolymarketApi {
    fn default() -> Self {
        Self {
            url: "https://gamma-api.polymarket.com".into(),
            clob_url: "https://clob.polymarket.com".into(),
            data_url: "https://data-api.polymarket.com".into(),
            timeout: 30,
        }
    }
}

/// The `reddit_web` route section: the site origin that serves search and comment pages, and the
/// origin of its embed pages.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct RedditWeb {
    pub(crate) url: String,
    pub(crate) embed_url: String,
    #[serde(deserialize_with = "deserialize_integer")]
    pub(crate) timeout: u64,
}

impl Default for RedditWeb {
    fn default() -> Self {
        Self {
            url: "https://www.reddit.com".into(),
            embed_url: "https://embed.reddit.com".into(),
            timeout: 30,
        }
    }
}

/// The `gemini_api` route section: a Gemini `generateContent` endpoint, the official API or a
/// gateway that speaks its protocol.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct GeminiApi {
    pub(crate) url: String,
    pub(crate) keys: Vec<Secret>,
    pub(crate) model: String,
    #[serde(deserialize_with = "deserialize_integer")]
    pub(crate) timeout: u64,
}

// A model reads a long video for minutes before it answers.
impl Default for GeminiApi {
    fn default() -> Self {
        Self {
            url: "https://generativelanguage.googleapis.com/v1beta".into(),
            keys: Vec::new(),
            model: "gemini-flash-latest".into(),
            timeout: 300,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Jina {
    pub(crate) url: String,
    pub(crate) keys: Vec<Secret>,
    pub(crate) respond_with: String,
    #[serde(deserialize_with = "deserialize_integer")]
    pub(crate) timeout: u64,
}

impl Default for Jina {
    fn default() -> Self {
        Self {
            url: "https://r.jina.ai".into(),
            keys: Vec::new(),
            respond_with: String::new(),
            timeout: 30,
        }
    }
}
