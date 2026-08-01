use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};

use super::load::load_effective_config;
use super::location::{ConfigError, ConfigLocation};
use super::schema::Config;
use crate::redact::Secret;

#[derive(Clone, Debug)]
pub(crate) struct ExaRuntimeConfig {
    pub(crate) url: String,
    pub(crate) keys: Vec<Secret>,
    pub(crate) timeout_seconds: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct XaiRuntimeConfig {
    pub(crate) url: String,
    pub(crate) keys: Vec<Secret>,
    pub(crate) model: String,
    pub(crate) tools: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct OpenAiCompatibleRuntimeConfig {
    pub(crate) url: String,
    pub(crate) keys: Vec<Secret>,
    pub(crate) model: String,
    pub(crate) fallback_models: Vec<String>,
    pub(crate) stream: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct ClassifierRuntimeConfig {
    pub(crate) url: String,
    pub(crate) keys: Vec<Secret>,
    pub(crate) model: String,
    pub(crate) fallback_models: Vec<String>,
    pub(crate) timeout_seconds: u64,
}

impl ClassifierRuntimeConfig {
    pub(crate) fn configured(&self) -> bool {
        !self.url.is_empty() && !self.keys.is_empty() && !self.model.is_empty()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct MainSearchRuntimeConfig {
    pub(crate) backends: Vec<String>,
    pub(crate) fallback: String,
    providers: BTreeMap<String, MainSearchProviderConfig>,
}

#[derive(Clone, Debug)]
pub(crate) enum MainSearchProviderConfig {
    Xai(XaiRuntimeConfig),
    OpenAiCompatible(OpenAiCompatibleRuntimeConfig),
}

impl MainSearchProviderConfig {
    pub(crate) fn configured(&self) -> bool {
        match self {
            Self::Xai(config) => !config.keys.is_empty(),
            Self::OpenAiCompatible(config) => !config.keys.is_empty(),
        }
    }

    fn model(&self) -> &str {
        match self {
            Self::Xai(config) => &config.model,
            Self::OpenAiCompatible(config) => &config.model,
        }
    }

    pub(crate) fn url(&self) -> &str {
        match self {
            Self::Xai(config) => &config.url,
            Self::OpenAiCompatible(config) => &config.url,
        }
    }

    pub(crate) fn keys(&self) -> &[Secret] {
        match self {
            Self::Xai(config) => &config.keys,
            Self::OpenAiCompatible(config) => &config.keys,
        }
    }
}

impl MainSearchRuntimeConfig {
    pub(crate) fn provider(&self, provider: &str) -> Option<&MainSearchProviderConfig> {
        self.providers.get(provider)
    }

    pub(crate) fn configured_provider_count(&self) -> usize {
        self.backends
            .iter()
            .filter(|provider| {
                self.provider(provider)
                    .is_some_and(MainSearchProviderConfig::configured)
            })
            .count()
    }

    pub(crate) fn default_model(&self) -> &str {
        self.backends
            .iter()
            .find_map(|provider| self.provider(provider))
            .map(MainSearchProviderConfig::model)
            .unwrap_or_default()
    }

    pub(crate) fn default_endpoint_host(&self) -> String {
        self.backends
            .iter()
            .find_map(|provider| self.provider(provider))
            .and_then(|provider| reqwest::Url::parse(provider.url()).ok())
            .and_then(|url| url.host_str().map(ToOwned::to_owned))
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct JournalRuntimeConfig {
    pub(crate) enabled: bool,
    pub(crate) dir: PathBuf,
    pub(crate) retention_days: u64,
    pub(crate) credentials: Vec<Secret>,
}

#[derive(Clone, Debug)]
pub(crate) struct Context7RuntimeConfig {
    pub(crate) url: String,
    pub(crate) keys: Vec<Secret>,
    pub(crate) timeout_seconds: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct AnysearchRuntimeConfig {
    pub(crate) url: String,
    pub(crate) keys: Vec<Secret>,
    pub(crate) timeout_seconds: u64,
}

#[derive(Clone, Debug)]
pub(crate) enum DocsSearchProviderConfig {
    Exa(ExaRuntimeConfig),
    Context7(Context7RuntimeConfig),
}

impl DocsSearchProviderConfig {
    pub(crate) fn configured(&self) -> bool {
        match self {
            Self::Exa(config) => !config.keys.is_empty(),
            Self::Context7(config) => !config.keys.is_empty(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct DocsSearchRuntimeConfig {
    pub(crate) order: Vec<String>,
    providers: BTreeMap<String, DocsSearchProviderConfig>,
}

impl DocsSearchRuntimeConfig {
    pub(crate) fn configured_provider_count(&self) -> usize {
        self.order
            .iter()
            .filter(|provider| {
                self.provider(provider)
                    .is_some_and(DocsSearchProviderConfig::configured)
            })
            .count()
    }

    pub(crate) fn provider(&self, provider: &str) -> Option<&DocsSearchProviderConfig> {
        self.providers.get(provider)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct VerticalSearchRuntimeConfig {
    pub(crate) order: Vec<String>,
    providers: BTreeMap<String, AnysearchRuntimeConfig>,
}

impl VerticalSearchRuntimeConfig {
    pub(crate) fn configured_provider_count(&self) -> usize {
        self.order
            .iter()
            .filter(|provider| {
                self.provider(provider)
                    .is_some_and(|config| !config.keys.is_empty())
            })
            .count()
    }

    pub(crate) fn provider(&self, provider: &str) -> Option<&AnysearchRuntimeConfig> {
        self.providers.get(provider)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct WebFetchProviderConfig {
    pub(crate) url: String,
    pub(crate) keys: Vec<Secret>,
    pub(crate) timeout_seconds: u64,
    pub(crate) respond_with: String,
}

#[derive(Clone, Debug)]
pub(crate) struct WebFetchRuntimeConfig {
    pub(crate) order: Vec<String>,
    providers: BTreeMap<String, WebFetchProviderConfig>,
}

#[derive(Clone, Debug)]
pub(crate) struct WebSearchRuntimeConfig {
    pub(crate) order: Vec<String>,
    providers: BTreeMap<String, WebFetchProviderConfig>,
}

impl WebSearchRuntimeConfig {
    pub(crate) fn configured_provider_count(&self) -> usize {
        self.order
            .iter()
            .filter(|provider| {
                self.provider(provider)
                    .is_some_and(|config| !config.keys.is_empty())
            })
            .count()
    }

    pub(crate) fn provider(&self, provider: &str) -> Option<&WebFetchProviderConfig> {
        self.providers.get(provider)
    }
}

impl WebFetchRuntimeConfig {
    pub(crate) fn configured_provider_count(&self) -> usize {
        self.order
            .iter()
            .filter(|provider| {
                self.provider(provider)
                    .is_some_and(|config| !config.keys.is_empty())
            })
            .count()
    }

    pub(crate) fn provider(&self, provider: &str) -> Option<&WebFetchProviderConfig> {
        self.providers.get(provider)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RetryRuntimeConfig {
    pub(crate) max_attempts: usize,
    pub(crate) multiplier: f64,
    pub(crate) max_wait_seconds: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct RuntimeConfig {
    pub(crate) main_search: MainSearchRuntimeConfig,
    pub(crate) classifier: ClassifierRuntimeConfig,
    pub(crate) exa: ExaRuntimeConfig,
    pub(crate) context7: Context7RuntimeConfig,
    pub(crate) anysearch: AnysearchRuntimeConfig,
    pub(crate) tavily: WebFetchProviderConfig,
    pub(crate) docs_search: DocsSearchRuntimeConfig,
    pub(crate) vertical_search: VerticalSearchRuntimeConfig,
    pub(crate) web_search: WebSearchRuntimeConfig,
    pub(crate) web_fetch: WebFetchRuntimeConfig,
    pub(crate) journal: JournalRuntimeConfig,
    pub(crate) retry: RetryRuntimeConfig,
    pub(crate) ssl_verify: bool,
}

// Runtime assembly mirrors the complete validated configuration surface in one place.
#[expect(clippy::too_many_lines)]
pub(crate) fn runtime_config() -> Result<RuntimeConfig, ConfigError> {
    let loaded = load_effective_config()?;
    let config = loaded.config;
    let config_dir = ConfigLocation::discover()?.config_dir;
    let journal = JournalRuntimeConfig {
        enabled: config.journal.enabled,
        dir: resolve_journal_dir(&config.journal.dir, &config_dir)?,
        retention_days: config.journal.retention_days.cast_unsigned(),
        credentials: configured_credentials(&config),
    };
    let tavily = WebFetchProviderConfig {
        url: config.providers.tavily.url,
        keys: config.providers.tavily.keys,
        timeout_seconds: config.providers.tavily.timeout.cast_unsigned(),
        respond_with: String::new(),
    };
    let firecrawl = WebFetchProviderConfig {
        url: config.providers.firecrawl.url,
        keys: config.providers.firecrawl.keys,
        timeout_seconds: config.providers.firecrawl.timeout.cast_unsigned(),
        respond_with: String::new(),
    };
    let exa = ExaRuntimeConfig {
        url: config.providers.exa.url,
        keys: config.providers.exa.keys,
        timeout_seconds: config.providers.exa.timeout.cast_unsigned(),
    };
    let context7 = Context7RuntimeConfig {
        url: config.providers.context7.url,
        keys: config.providers.context7.keys,
        timeout_seconds: config.providers.context7.timeout.cast_unsigned(),
    };
    let anysearch = AnysearchRuntimeConfig {
        url: config.providers.anysearch.url,
        keys: config.providers.anysearch.keys,
        timeout_seconds: config.providers.anysearch.timeout.cast_unsigned(),
    };
    let classifier = ClassifierRuntimeConfig {
        url: config.classifier.url,
        keys: config.classifier.keys,
        model: config.classifier.model,
        fallback_models: config.classifier.fallback_models,
        timeout_seconds: config.classifier.timeout.cast_unsigned(),
    };
    Ok(RuntimeConfig {
        main_search: MainSearchRuntimeConfig {
            backends: config.search.backends,
            fallback: config.search.fallback,
            providers: BTreeMap::from([
                (
                    "xai".into(),
                    MainSearchProviderConfig::Xai(XaiRuntimeConfig {
                        url: config.providers.xai.url,
                        keys: config.providers.xai.keys,
                        model: config.providers.xai.model,
                        tools: config.providers.xai.tools,
                    }),
                ),
                (
                    "openai_compatible".into(),
                    MainSearchProviderConfig::OpenAiCompatible(OpenAiCompatibleRuntimeConfig {
                        url: config.providers.openai_compatible.url,
                        keys: config.providers.openai_compatible.keys,
                        model: config.providers.openai_compatible.model,
                        fallback_models: config.providers.openai_compatible.fallback_models,
                        stream: config.providers.openai_compatible.stream,
                    }),
                ),
            ]),
        },
        classifier,
        exa: exa.clone(),
        context7: context7.clone(),
        anysearch: anysearch.clone(),
        tavily: tavily.clone(),
        docs_search: DocsSearchRuntimeConfig {
            order: config.capabilities.docs_search.order,
            providers: BTreeMap::from([
                ("exa".into(), DocsSearchProviderConfig::Exa(exa)),
                (
                    "context7".into(),
                    DocsSearchProviderConfig::Context7(context7),
                ),
            ]),
        },
        vertical_search: VerticalSearchRuntimeConfig {
            order: config.capabilities.vertical_search.order,
            providers: BTreeMap::from([("anysearch".into(), anysearch)]),
        },
        web_search: WebSearchRuntimeConfig {
            order: config.capabilities.web_search.order,
            providers: BTreeMap::from([
                ("tavily".into(), tavily.clone()),
                ("firecrawl".into(), firecrawl.clone()),
            ]),
        },
        web_fetch: WebFetchRuntimeConfig {
            order: config.capabilities.web_fetch.order,
            providers: BTreeMap::from([
                (
                    "jina".into(),
                    WebFetchProviderConfig {
                        url: config.providers.jina.url,
                        keys: config.providers.jina.keys,
                        timeout_seconds: config.providers.jina.timeout.cast_unsigned(),
                        respond_with: config.providers.jina.respond_with,
                    },
                ),
                ("tavily".into(), tavily),
                ("firecrawl".into(), firecrawl),
            ]),
        },
        journal,
        retry: RetryRuntimeConfig {
            max_attempts: usize::try_from(config.retry.max_attempts).map_err(|_| {
                ConfigError::Message("retry.max_attempts exceeds this platform's limit".into())
            })?,
            multiplier: config.retry.multiplier,
            max_wait_seconds: config.retry.max_wait.cast_unsigned(),
        },
        ssl_verify: config.http.ssl_verify,
    })
}

fn resolve_journal_dir(value: &str, config_dir: &Path) -> Result<PathBuf, ConfigError> {
    if value == "~/.local/state/forager/journal"
        && let Some(state_home) = env::var_os("XDG_STATE_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    {
        return Ok(state_home.join("forager/journal"));
    }
    if let Some(relative) = value.strip_prefix("~/") {
        return env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .map(|home| home.join(relative))
            .ok_or_else(|| {
                ConfigError::Message(
                    "journal.dir uses `~` but HOME is unavailable or not absolute".into(),
                )
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
    [
        &config.classifier.keys,
        &config.providers.xai.keys,
        &config.providers.openai_compatible.keys,
        &config.providers.exa.keys,
        &config.providers.context7.keys,
        &config.providers.jina.keys,
        &config.providers.tavily.keys,
        &config.providers.firecrawl.keys,
        &config.providers.anysearch.keys,
    ]
    .into_iter()
    .flatten()
    .cloned()
    .collect()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::redact::{CREDENTIAL_MASK, Secret};

    use super::{configured_credentials, resolve_journal_dir};
    use crate::config::schema::Config;

    #[test]
    fn configured_credentials_collects_all_nine_pools_without_debug_leakage() {
        let mut config = Config::default();
        config.classifier.keys = vec![Secret::from("classifier-canary")];
        config.providers.xai.keys = vec![Secret::from("xai-canary")];
        config.providers.openai_compatible.keys = vec![Secret::from("openai-canary")];
        config.providers.exa.keys = vec![Secret::from("exa-canary")];
        config.providers.context7.keys = vec![Secret::from("context7-canary")];
        config.providers.jina.keys = vec![Secret::from("jina-canary")];
        config.providers.tavily.keys = vec![Secret::from("tavily-canary")];
        config.providers.firecrawl.keys = vec![Secret::from("firecrawl-canary")];
        config.providers.anysearch.keys = vec![Secret::from("anysearch-canary")];

        let credentials = configured_credentials(&config);
        let debug = format!("{credentials:?}");

        assert_eq!(credentials.len(), 9);
        assert_eq!(debug.matches(CREDENTIAL_MASK).count(), 9);
        assert!(!debug.contains("canary"));
    }

    #[test]
    fn journal_directory_resolves_relative_to_the_configuration_directory() {
        let directory = resolve_journal_dir("journal", Path::new("/tmp/forager-config"))
            .expect("resolve relative journal directory");

        assert_eq!(directory, PathBuf::from("/tmp/forager-config/journal"));
    }
}
