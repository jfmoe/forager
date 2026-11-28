//! The runtime configuration assembled from the validated file and environment: each seam's
//! provider order, each platform's route order, and the configuration of every route.
//!
//! The no-wildcard rule covers every match in this module, so a new provider identity or route
//! configuration is a compile error at each projection that must place it.
#![deny(clippy::wildcard_enum_match_arm)]

use std::env;
use std::path::{Path, PathBuf};

use super::load::load_effective_config;
use super::location::{ConfigError, ConfigLocation};
use super::route_config::{
    BrowserRouteRuntimeConfig, BrowserRuntimeConfig, HnRouteRuntimeConfig, HttpRouteRuntimeConfig,
    KeyedHttpRouteRuntimeConfig, ModelRouteRuntimeConfig, PolymarketRouteRuntimeConfig,
};
use super::schema::{Config, FieldRef, Providers, SCHEMA};
use super::seam_config::{
    DocsSearchRuntimeConfig, ExaRuntimeConfig, MainSearchProviders, MainSearchRuntimeConfig,
    OpenaiRuntimeConfig, SeamEntry, WebFetchProviderConfig, WebFetchRuntimeConfig, WebProviders,
    WebSearchRuntimeConfig, XaiRuntimeConfig, docs_search_runtime, main_search_runtime,
    provider_configured, unknown_provider, web_fetch_runtime, web_search_runtime,
};
use crate::catalog::{self, ProviderId};
use crate::redact::Secret;
use crate::state_file;
use crate::types::Platform;

#[derive(Clone, Debug)]
pub(crate) struct JournalRuntimeConfig {
    pub(crate) enabled: bool,
    pub(crate) dir: PathBuf,
    pub(crate) retention_days: u64,
    pub(crate) credentials: Vec<Secret>,
}

/// The configuration of every platform route. Route lookups take it whole, so a new route adds
/// a field here and changes no signature.
#[derive(Clone, Debug)]
pub(crate) struct PlatformRoutesRuntimeConfig {
    pub(crate) arxiv_api: HttpRouteRuntimeConfig,
    pub(crate) ssrn_crossref: HttpRouteRuntimeConfig,
    pub(crate) serpapi: KeyedHttpRouteRuntimeConfig,
    pub(crate) xiaohongshu_browser: BrowserRouteRuntimeConfig,
    /// The main search provider, which also reads X.
    pub(crate) xai: XaiRuntimeConfig,
    pub(crate) hn_api: HnRouteRuntimeConfig,
    pub(crate) youtube_api: KeyedHttpRouteRuntimeConfig,
    pub(crate) youtube_web: HttpRouteRuntimeConfig,
    pub(crate) gemini_api: ModelRouteRuntimeConfig,
    pub(crate) polymarket_api: PolymarketRouteRuntimeConfig,
    pub(crate) reddit_browser: BrowserRouteRuntimeConfig,
}

/// The configuration of one platform route.
#[derive(Clone, Debug)]
pub(crate) enum PlatformRouteConfig {
    ArxivApi(HttpRouteRuntimeConfig),
    SsrnCrossref(HttpRouteRuntimeConfig),
    Serpapi(KeyedHttpRouteRuntimeConfig),
    XiaohongshuBrowser(BrowserRouteRuntimeConfig),
    Xai(XaiRuntimeConfig),
    HnApi(HnRouteRuntimeConfig),
    YoutubeApi(KeyedHttpRouteRuntimeConfig),
    YoutubeWeb(HttpRouteRuntimeConfig),
    GeminiApi(ModelRouteRuntimeConfig),
    PolymarketApi(PolymarketRouteRuntimeConfig),
    RedditBrowser(BrowserRouteRuntimeConfig),
}

impl PlatformRouteConfig {
    pub(crate) fn route(&self) -> ProviderId {
        match self {
            Self::ArxivApi(_) => ProviderId::ArxivApi,
            Self::SsrnCrossref(_) => ProviderId::SsrnCrossref,
            Self::Serpapi(_) => ProviderId::Serpapi,
            Self::XiaohongshuBrowser(_) => ProviderId::XiaohongshuBrowser,
            Self::Xai(_) => ProviderId::Xai,
            Self::HnApi(_) => ProviderId::HnApi,
            Self::YoutubeApi(_) => ProviderId::YoutubeApi,
            Self::YoutubeWeb(_) => ProviderId::YoutubeWeb,
            Self::GeminiApi(_) => ProviderId::GeminiApi,
            Self::PolymarketApi(_) => ProviderId::PolymarketApi,
            Self::RedditBrowser(_) => ProviderId::RedditBrowser,
        }
    }

    pub(crate) fn configured(&self) -> bool {
        match self {
            Self::ArxivApi(_)
            | Self::SsrnCrossref(_)
            | Self::XiaohongshuBrowser(_)
            | Self::HnApi(_)
            | Self::YoutubeWeb(_)
            | Self::PolymarketApi(_)
            | Self::RedditBrowser(_) => provider_configured(self.route(), &[]),
            Self::Serpapi(config) | Self::YoutubeApi(config) => {
                provider_configured(self.route(), &config.keys)
            }
            Self::Xai(config) => provider_configured(self.route(), &config.keys),
            Self::GeminiApi(config) => provider_configured(self.route(), &config.keys),
        }
    }
}

/// The configured route order of one platform.
#[derive(Clone, Debug)]
pub(crate) struct PlatformRuntimeConfig {
    platform: Platform,
    entries: Vec<SeamEntry<PlatformRouteConfig>>,
}

impl PlatformRuntimeConfig {
    pub(crate) fn platform(&self) -> Platform {
        self.platform
    }

    pub(crate) fn entries(&self) -> &[SeamEntry<PlatformRouteConfig>] {
        &self.entries
    }

    /// Returns the configuration key that orders this platform's routes.
    pub(crate) fn order_key(&self) -> String {
        platform_order_key(self.platform)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PlatformsRuntimeConfig {
    arxiv: PlatformRuntimeConfig,
    ssrn: PlatformRuntimeConfig,
    scholar: PlatformRuntimeConfig,
    xiaohongshu: PlatformRuntimeConfig,
    x: PlatformRuntimeConfig,
    hn: PlatformRuntimeConfig,
    youtube: PlatformRuntimeConfig,
    polymarket: PlatformRuntimeConfig,
    reddit: PlatformRuntimeConfig,
}

impl PlatformsRuntimeConfig {
    pub(crate) fn get(&self, platform: Platform) -> &PlatformRuntimeConfig {
        match platform {
            Platform::Arxiv => &self.arxiv,
            Platform::Ssrn => &self.ssrn,
            Platform::Scholar => &self.scholar,
            Platform::Xiaohongshu => &self.xiaohongshu,
            Platform::X => &self.x,
            Platform::Hn => &self.hn,
            Platform::Youtube => &self.youtube,
            Platform::Polymarket => &self.polymarket,
            Platform::Reddit => &self.reddit,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RetryRuntimeConfig {
    pub(crate) max_attempts: usize,
    pub(crate) multiplier: f64,
    pub(crate) max_wait_seconds: u64,
}

/// What the stderr attempt log prints for each retrieval command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LogLevel {
    /// Nothing.
    Off,
    /// One summary line for the invocation.
    Summary,
    /// The summary line, then one line for each attempt.
    Attempts,
}

impl LogLevel {
    fn from_validated(value: &str) -> Self {
        match value {
            "off" => Self::Off,
            "summary" => Self::Summary,
            "attempts" => Self::Attempts,
            _ => unreachable!("log.level is validated before runtime assembly"),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RuntimeConfig {
    pub(crate) main_search: MainSearchRuntimeConfig,
    pub(crate) xai: XaiRuntimeConfig,
    pub(crate) openai: OpenaiRuntimeConfig,
    pub(crate) exa: ExaRuntimeConfig,
    pub(crate) platform_routes: PlatformRoutesRuntimeConfig,
    pub(crate) tavily: WebFetchProviderConfig,
    pub(crate) firecrawl: WebFetchProviderConfig,
    pub(crate) jina: WebFetchProviderConfig,
    pub(crate) docs_search: DocsSearchRuntimeConfig,
    pub(crate) web_search: WebSearchRuntimeConfig,
    pub(crate) web_fetch: WebFetchRuntimeConfig,
    pub(crate) platforms: PlatformsRuntimeConfig,
    pub(crate) journal: JournalRuntimeConfig,
    pub(crate) retry: RetryRuntimeConfig,
    pub(crate) log_level: LogLevel,
    pub(crate) ssl_verify: bool,
    pub(crate) browser: BrowserRuntimeConfig,
}

pub(crate) struct ProviderRuntime<'a> {
    /// The HTTP endpoint; empty for a browser route, which has none.
    pub(crate) endpoint: &'a str,
    pub(crate) keys: &'a [Secret],
}

impl RuntimeConfig {
    /// Applies the shared configured rule: credentials are needed only when the registration
    /// requires them.
    pub(crate) fn provider_configured(&self, id: ProviderId) -> bool {
        provider_configured(id, self.provider_runtime(id).keys)
    }

    pub(crate) fn provider_runtime(&self, id: ProviderId) -> ProviderRuntime<'_> {
        match id {
            ProviderId::Xai => ProviderRuntime {
                endpoint: &self.xai.url,
                keys: &self.xai.keys,
            },
            ProviderId::Openai => ProviderRuntime {
                endpoint: &self.openai.url,
                keys: &self.openai.keys,
            },
            ProviderId::Exa => ProviderRuntime {
                endpoint: &self.exa.url,
                keys: &self.exa.keys,
            },
            ProviderId::Tavily => ProviderRuntime {
                endpoint: &self.tavily.url,
                keys: &self.tavily.keys,
            },
            ProviderId::Firecrawl => ProviderRuntime {
                endpoint: &self.firecrawl.url,
                keys: &self.firecrawl.keys,
            },
            ProviderId::Jina => ProviderRuntime {
                endpoint: &self.jina.url,
                keys: &self.jina.keys,
            },
            ProviderId::ArxivApi => ProviderRuntime {
                endpoint: &self.platform_routes.arxiv_api.url,
                keys: &[],
            },
            ProviderId::SsrnCrossref => ProviderRuntime {
                endpoint: &self.platform_routes.ssrn_crossref.url,
                keys: &[],
            },
            ProviderId::Serpapi => ProviderRuntime {
                endpoint: &self.platform_routes.serpapi.url,
                keys: &self.platform_routes.serpapi.keys,
            },
            ProviderId::XiaohongshuBrowser | ProviderId::RedditBrowser => ProviderRuntime {
                endpoint: "",
                keys: &[],
            },
            ProviderId::HnApi => ProviderRuntime {
                endpoint: &self.platform_routes.hn_api.url,
                keys: &[],
            },
            ProviderId::YoutubeApi => ProviderRuntime {
                endpoint: &self.platform_routes.youtube_api.url,
                keys: &self.platform_routes.youtube_api.keys,
            },
            ProviderId::YoutubeWeb => ProviderRuntime {
                endpoint: &self.platform_routes.youtube_web.url,
                keys: &[],
            },
            ProviderId::GeminiApi => ProviderRuntime {
                endpoint: &self.platform_routes.gemini_api.url,
                keys: &self.platform_routes.gemini_api.keys,
            },
            ProviderId::PolymarketApi => ProviderRuntime {
                endpoint: &self.platform_routes.polymarket_api.url,
                keys: &[],
            },
        }
    }
}

pub(crate) fn runtime_config() -> Result<RuntimeConfig, ConfigError> {
    let loaded = load_effective_config()?;
    let config_dir = ConfigLocation::discover()?.config_dir;
    assemble(loaded.config, &config_dir)
}

/// The runtime configuration of a TOML document, as the loader would read it from a file in
/// the test process's state directory, without the environment's overrides.
#[cfg(test)]
pub(crate) fn runtime_config_from_toml(document: &str) -> Result<RuntimeConfig, ConfigError> {
    let config_dir = crate::state_file::state_directory().expect("the test state directory");
    let config = super::load::parse_document(document, &config_dir.join("config.toml"))?;
    assemble(config, &config_dir)
}

// Runtime assembly mirrors the complete validated configuration surface in one place.
#[expect(clippy::too_many_lines)]
fn assemble(config: Config, config_dir: &Path) -> Result<RuntimeConfig, ConfigError> {
    let fallback = config
        .search
        .fallback
        .parse()
        .map_err(ConfigError::Message)?;
    let journal = JournalRuntimeConfig {
        enabled: config.journal.enabled,
        dir: resolve_journal_dir(&config.journal.dir, config_dir)?,
        retention_days: config.journal.retention_days,
        credentials: configured_credentials(&config),
    };
    let browser = BrowserRuntimeConfig {
        path: (!config.browser.path.is_empty())
            .then(|| resolve_path("browser.path", &config.browser.path, config_dir))
            .transpose()?,
    };
    let platform_routes = platform_routes(&config.providers, &browser);
    let xai = platform_routes.xai.clone();
    let tavily = WebFetchProviderConfig {
        url: config.providers.tavily.url,
        keys: config.providers.tavily.keys,
        timeout_seconds: config.providers.tavily.timeout,
        respond_with: String::new(),
    };
    let firecrawl = WebFetchProviderConfig {
        url: config.providers.firecrawl.url,
        keys: config.providers.firecrawl.keys,
        timeout_seconds: config.providers.firecrawl.timeout,
        respond_with: String::new(),
    };
    let jina = WebFetchProviderConfig {
        url: config.providers.jina.url,
        keys: config.providers.jina.keys,
        timeout_seconds: config.providers.jina.timeout,
        respond_with: config.providers.jina.respond_with,
    };
    let exa = ExaRuntimeConfig {
        url: config.providers.exa.url,
        keys: config.providers.exa.keys,
        timeout_seconds: config.providers.exa.timeout,
    };
    let openai = OpenaiRuntimeConfig {
        url: config.providers.openai.url,
        keys: config.providers.openai.keys,
        model: config.providers.openai.model,
    };
    let main_search = main_search_runtime(
        config.search.backends,
        fallback,
        &MainSearchProviders {
            xai: &xai,
            openai: &openai,
        },
    )?;
    let docs_search = docs_search_runtime(config.capabilities.docs_search.order, &exa)?;
    let web_providers = WebProviders {
        tavily: &tavily,
        firecrawl: &firecrawl,
        jina: &jina,
    };
    let web_search = web_search_runtime(config.capabilities.web_search.order, &web_providers)?;
    let web_fetch = web_fetch_runtime(config.capabilities.web_fetch.order, &web_providers)?;
    let platforms = PlatformsRuntimeConfig {
        arxiv: platform_entries(
            Platform::Arxiv,
            config.platforms.arxiv.order,
            &platform_routes,
        )?,
        ssrn: platform_entries(
            Platform::Ssrn,
            config.platforms.ssrn.order,
            &platform_routes,
        )?,
        scholar: platform_entries(
            Platform::Scholar,
            config.platforms.scholar.order,
            &platform_routes,
        )?,
        xiaohongshu: platform_entries(
            Platform::Xiaohongshu,
            config.platforms.xiaohongshu.order,
            &platform_routes,
        )?,
        x: platform_entries(Platform::X, config.platforms.x.order, &platform_routes)?,
        hn: platform_entries(Platform::Hn, config.platforms.hn.order, &platform_routes)?,
        youtube: platform_entries(
            Platform::Youtube,
            config.platforms.youtube.order,
            &platform_routes,
        )?,
        polymarket: platform_entries(
            Platform::Polymarket,
            config.platforms.polymarket.order,
            &platform_routes,
        )?,
        reddit: platform_entries(
            Platform::Reddit,
            config.platforms.reddit.order,
            &platform_routes,
        )?,
    };
    Ok(RuntimeConfig {
        main_search,
        xai,
        openai,
        exa,
        platform_routes,
        tavily,
        firecrawl,
        jina,
        docs_search,
        web_search,
        web_fetch,
        platforms,
        journal,
        retry: RetryRuntimeConfig {
            max_attempts: usize::try_from(config.retry.max_attempts).map_err(|_| {
                ConfigError::Message("retry.max_attempts exceeds this platform's limit".into())
            })?,
            multiplier: config.retry.multiplier,
            max_wait_seconds: config.retry.max_wait,
        },
        log_level: LogLevel::from_validated(&config.log.level),
        ssl_verify: config.http.ssl_verify,
        browser,
    })
}

/// Projects the sections of every platform route, including the main-search backend that also
/// reads X, onto their runtime configuration.
fn platform_routes(
    providers: &Providers,
    browser: &BrowserRuntimeConfig,
) -> PlatformRoutesRuntimeConfig {
    PlatformRoutesRuntimeConfig {
        arxiv_api: HttpRouteRuntimeConfig {
            url: providers.arxiv_api.url.clone(),
            timeout_seconds: providers.arxiv_api.timeout,
        },
        ssrn_crossref: HttpRouteRuntimeConfig {
            url: providers.ssrn_crossref.url.clone(),
            timeout_seconds: providers.ssrn_crossref.timeout,
        },
        serpapi: KeyedHttpRouteRuntimeConfig {
            url: providers.serpapi.url.clone(),
            keys: providers.serpapi.keys.clone(),
            timeout_seconds: providers.serpapi.timeout,
        },
        xiaohongshu_browser: BrowserRouteRuntimeConfig {
            timeout_seconds: providers.xiaohongshu_browser.timeout,
            browser: browser.clone(),
        },
        xai: XaiRuntimeConfig {
            url: providers.xai.url.clone(),
            keys: providers.xai.keys.clone(),
            model: providers.xai.model.clone(),
            tools: providers.xai.tools.clone(),
        },
        hn_api: HnRouteRuntimeConfig {
            url: providers.hn_api.url.clone(),
            firebase_url: providers.hn_api.firebase_url.clone(),
            timeout_seconds: providers.hn_api.timeout,
        },
        youtube_api: KeyedHttpRouteRuntimeConfig {
            url: providers.youtube_api.url.clone(),
            keys: providers.youtube_api.keys.clone(),
            timeout_seconds: providers.youtube_api.timeout,
        },
        youtube_web: HttpRouteRuntimeConfig {
            url: providers.youtube_web.url.clone(),
            timeout_seconds: providers.youtube_web.timeout,
        },
        gemini_api: ModelRouteRuntimeConfig {
            url: providers.gemini_api.url.clone(),
            keys: providers.gemini_api.keys.clone(),
            model: providers.gemini_api.model.clone(),
            timeout_seconds: providers.gemini_api.timeout,
        },
        polymarket_api: PolymarketRouteRuntimeConfig {
            url: providers.polymarket_api.url.clone(),
            clob_url: providers.polymarket_api.clob_url.clone(),
            data_url: providers.polymarket_api.data_url.clone(),
            timeout_seconds: providers.polymarket_api.timeout,
        },
        reddit_browser: BrowserRouteRuntimeConfig {
            timeout_seconds: providers.reddit_browser.timeout,
            browser: browser.clone(),
        },
    }
}

/// The runtime configuration of every platform route under the default configuration.
#[cfg(test)]
pub(crate) fn default_platform_routes() -> PlatformRoutesRuntimeConfig {
    platform_routes(
        &Config::default().providers,
        &BrowserRuntimeConfig { path: None },
    )
}

fn platform_entries(
    platform: Platform,
    order: Vec<String>,
    routes: &PlatformRoutesRuntimeConfig,
) -> Result<PlatformRuntimeConfig, ConfigError> {
    let key = platform_order_key(platform);
    let catalog = catalog::platform(platform);
    let entries = order
        .into_iter()
        .map(|name| {
            let config = ProviderId::parse(&name)
                .filter(|id| catalog.contains(*id))
                .and_then(|id| Some((id, platform_route_config(id, routes)?)))
                .ok_or_else(|| unknown_provider(&name, &key))?;
            let configured = config.1.configured();
            Ok(SeamEntry::new(config.0, config.1, configured))
        })
        .collect::<Result<_, ConfigError>>()?;
    Ok(PlatformRuntimeConfig { platform, entries })
}

/// Returns the configuration key that orders the routes of `platform`.
pub(crate) fn platform_order_key(platform: Platform) -> String {
    format!("platforms.{platform}.order")
}

/// Returns the configuration key that holds the credential pool of `provider`.
pub(crate) fn provider_keys_key(provider: &str) -> String {
    format!("providers.{provider}.keys")
}

/// Returns the configuration of a platform route, or `None` for a provider that is no route.
pub(crate) fn platform_route_config(
    id: ProviderId,
    routes: &PlatformRoutesRuntimeConfig,
) -> Option<PlatformRouteConfig> {
    match id {
        ProviderId::ArxivApi => Some(PlatformRouteConfig::ArxivApi(routes.arxiv_api.clone())),
        ProviderId::SsrnCrossref => Some(PlatformRouteConfig::SsrnCrossref(
            routes.ssrn_crossref.clone(),
        )),
        ProviderId::Serpapi => Some(PlatformRouteConfig::Serpapi(routes.serpapi.clone())),
        ProviderId::XiaohongshuBrowser => Some(PlatformRouteConfig::XiaohongshuBrowser(
            routes.xiaohongshu_browser.clone(),
        )),
        ProviderId::Xai => Some(PlatformRouteConfig::Xai(routes.xai.clone())),
        ProviderId::HnApi => Some(PlatformRouteConfig::HnApi(routes.hn_api.clone())),
        ProviderId::YoutubeApi => Some(PlatformRouteConfig::YoutubeApi(routes.youtube_api.clone())),
        ProviderId::YoutubeWeb => Some(PlatformRouteConfig::YoutubeWeb(routes.youtube_web.clone())),
        ProviderId::GeminiApi => Some(PlatformRouteConfig::GeminiApi(routes.gemini_api.clone())),
        ProviderId::PolymarketApi => Some(PlatformRouteConfig::PolymarketApi(
            routes.polymarket_api.clone(),
        )),
        ProviderId::RedditBrowser => Some(PlatformRouteConfig::RedditBrowser(
            routes.reddit_browser.clone(),
        )),
        ProviderId::Openai
        | ProviderId::Exa
        | ProviderId::Tavily
        | ProviderId::Firecrawl
        | ProviderId::Jina => None,
    }
}

fn resolve_journal_dir(value: &str, config_dir: &Path) -> Result<PathBuf, ConfigError> {
    if value == "~/.local/state/forager/journal"
        && let Some(state_directory) = state_file::state_directory()
    {
        return Ok(state_directory.join("journal"));
    }
    resolve_path("journal.dir", value, config_dir)
}

/// Resolves a configured path: a leading `~/` expands to `HOME`, and a relative path is relative
/// to the configuration directory, never the working directory.
fn resolve_path(key: &str, value: &str, config_dir: &Path) -> Result<PathBuf, ConfigError> {
    if let Some(relative) = value.strip_prefix("~/") {
        return env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .map(|home| home.join(relative))
            .ok_or_else(|| {
                ConfigError::Message(format!(
                    "{key} uses `~` but HOME is unavailable or not absolute"
                ))
            });
    }
    let path = PathBuf::from(value);
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(config_dir.join(path))
    }
}

fn configured_credentials(config: &Config) -> Vec<Secret> {
    SCHEMA
        .iter()
        .filter_map(|leaf| match (leaf.get)(config) {
            FieldRef::Secrets(keys) => Some(keys),
            FieldRef::String(_)
            | FieldRef::Bool(_)
            | FieldRef::U64(_)
            | FieldRef::F64(_)
            | FieldRef::Strings(_) => None,
        })
        .flatten()
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::path::{Path, PathBuf};

    use crate::redact::{CREDENTIAL_MASK, Secret};

    use super::{configured_credentials, resolve_journal_dir};
    use crate::config::schema::Config;
    use crate::state_file;

    #[test]
    fn configured_credentials_collects_every_pool_without_debug_leakage() {
        let mut config = Config::default();
        config.providers.openai.keys = vec![Secret::from("openai-canary")];
        config.providers.xai.keys = vec![Secret::from("xai-canary")];
        config.providers.exa.keys = vec![Secret::from("exa-canary")];
        config.providers.youtube_api.keys = vec![Secret::from("youtube-canary")];
        config.providers.jina.keys = vec![Secret::from("jina-canary")];
        config.providers.tavily.keys = vec![Secret::from("tavily-canary")];
        config.providers.firecrawl.keys = vec![Secret::from("firecrawl-canary")];
        config.providers.serpapi.keys = vec![Secret::from("serpapi-canary")];

        let credentials = configured_credentials(&config);
        let debug = format!("{credentials:?}");

        assert_eq!(credentials.len(), 8);
        assert_eq!(debug.matches(CREDENTIAL_MASK).count(), 8);
        assert!(!debug.contains("canary"));
    }

    #[test]
    fn in_crate_tests_resolve_state_and_the_default_journal_outside_home() {
        let home = env::var_os("HOME").map(PathBuf::from);
        let state = state_file::state_directory().expect("test state directory");
        let journal = resolve_journal_dir(&Config::default().journal.dir, Path::new("/unused"))
            .expect("resolve default journal directory");

        let under_home = |path: &Path| home.as_deref().is_some_and(|home| path.starts_with(home));
        assert_eq!(
            (
                state.starts_with(env::temp_dir()),
                journal == state.join("journal"),
                under_home(&state),
            ),
            (true, true, false),
            "state {} and journal {} must be in the test process's temporary directory, outside HOME",
            state.display(),
            journal.display()
        );
    }

    #[test]
    fn journal_directory_resolves_relative_to_the_configuration_directory() {
        let directory = resolve_journal_dir("journal", Path::new("/tmp/forager-config"))
            .expect("resolve relative journal directory");

        assert_eq!(directory, PathBuf::from("/tmp/forager-config/journal"));
    }
}
