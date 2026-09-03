use std::env;
use std::path::{Path, PathBuf};

use super::load::load_effective_config;
use super::location::{ConfigError, ConfigLocation};
use super::schema::{Config, FieldRef, SCHEMA};
use crate::catalog::{self, ProviderId};
use crate::redact::Secret;
use crate::types::FallbackPolicy;

#[derive(Clone, Debug)]
pub(crate) struct SeamEntry<C> {
    id: ProviderId,
    config: C,
    configured: bool,
}

impl<C> SeamEntry<C> {
    fn new(id: ProviderId, config: C, configured: bool) -> Self {
        Self {
            id,
            config,
            configured,
        }
    }

    pub(crate) fn id(&self) -> ProviderId {
        self.id
    }

    pub(crate) fn name(&self) -> &'static str {
        self.id.name()
    }

    pub(crate) fn configured(&self) -> bool {
        self.configured
    }

    pub(crate) fn config(&self) -> &C {
        &self.config
    }

    pub(crate) fn into_parts(self) -> (ProviderId, C, bool) {
        (self.id, self.config, self.configured)
    }
}

fn entry_names<C>(entries: &[SeamEntry<C>]) -> Vec<String> {
    entries
        .iter()
        .map(|entry| entry.name().to_owned())
        .collect()
}

fn unconfigured_entry_names<C>(entries: &[SeamEntry<C>]) -> Vec<String> {
    entries
        .iter()
        .filter(|entry| !entry.configured())
        .map(|entry| entry.name().to_owned())
        .collect()
}

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
    entries: Vec<SeamEntry<MainSearchProviderConfig>>,
    pub(crate) fallback: FallbackPolicy,
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

    pub(crate) fn url(&self) -> &str {
        match self {
            Self::Xai(config) => &config.url,
            Self::OpenAiCompatible(config) => &config.url,
        }
    }
}

impl MainSearchRuntimeConfig {
    pub(crate) fn entries(&self) -> &[SeamEntry<MainSearchProviderConfig>] {
        &self.entries
    }

    pub(crate) fn into_entries(self) -> Vec<SeamEntry<MainSearchProviderConfig>> {
        self.entries
    }

    pub(crate) fn configured_provider_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.configured())
            .count()
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
    entries: Vec<SeamEntry<DocsSearchProviderConfig>>,
}

impl DocsSearchRuntimeConfig {
    pub(crate) fn configured_provider_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.configured())
            .count()
    }

    pub(crate) fn entries(&self) -> &[SeamEntry<DocsSearchProviderConfig>] {
        &self.entries
    }

    pub(crate) fn into_entries(self) -> Vec<SeamEntry<DocsSearchProviderConfig>> {
        self.entries
    }

    pub(crate) fn names(&self) -> Vec<String> {
        entry_names(&self.entries)
    }

    pub(crate) fn unconfigured_names(&self) -> Vec<String> {
        unconfigured_entry_names(&self.entries)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct VerticalSearchRuntimeConfig {
    entries: Vec<SeamEntry<AnysearchRuntimeConfig>>,
}

impl VerticalSearchRuntimeConfig {
    pub(crate) fn configured_provider_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.configured())
            .count()
    }

    pub(crate) fn entries(&self) -> &[SeamEntry<AnysearchRuntimeConfig>] {
        &self.entries
    }

    pub(crate) fn into_entries(self) -> Vec<SeamEntry<AnysearchRuntimeConfig>> {
        self.entries
    }

    pub(crate) fn names(&self) -> Vec<String> {
        entry_names(&self.entries)
    }

    pub(crate) fn unconfigured_names(&self) -> Vec<String> {
        unconfigured_entry_names(&self.entries)
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
    entries: Vec<SeamEntry<WebFetchProviderConfig>>,
}

#[derive(Clone, Debug)]
pub(crate) struct WebSearchRuntimeConfig {
    entries: Vec<SeamEntry<WebFetchProviderConfig>>,
}

impl WebSearchRuntimeConfig {
    pub(crate) fn configured_provider_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.configured())
            .count()
    }

    pub(crate) fn entries(&self) -> &[SeamEntry<WebFetchProviderConfig>] {
        &self.entries
    }

    pub(crate) fn into_entries(self) -> Vec<SeamEntry<WebFetchProviderConfig>> {
        self.entries
    }

    pub(crate) fn names(&self) -> Vec<String> {
        entry_names(&self.entries)
    }

    pub(crate) fn unconfigured_names(&self) -> Vec<String> {
        unconfigured_entry_names(&self.entries)
    }

    pub(crate) fn retain(&mut self, id: ProviderId) {
        self.entries.retain(|entry| entry.id() == id);
    }
}

impl WebFetchRuntimeConfig {
    pub(crate) fn configured_provider_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.configured())
            .count()
    }

    pub(crate) fn entries(&self) -> &[SeamEntry<WebFetchProviderConfig>] {
        &self.entries
    }

    pub(crate) fn into_entries(self) -> Vec<SeamEntry<WebFetchProviderConfig>> {
        self.entries
    }

    pub(crate) fn names(&self) -> Vec<String> {
        entry_names(&self.entries)
    }

    pub(crate) fn unconfigured_names(&self) -> Vec<String> {
        unconfigured_entry_names(&self.entries)
    }

    pub(crate) fn retain_first(&mut self) {
        self.entries.truncate(1);
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RetryRuntimeConfig {
    pub(crate) max_attempts: usize,
    pub(crate) multiplier: f64,
    pub(crate) max_wait_seconds: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    fn from_validated(value: &str) -> Self {
        match value {
            "error" => Self::Error,
            "warn" => Self::Warn,
            "info" => Self::Info,
            "debug" => Self::Debug,
            "trace" => Self::Trace,
            _ => unreachable!("log.level is validated before runtime assembly"),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RuntimeConfig {
    pub(crate) main_search: MainSearchRuntimeConfig,
    pub(crate) classifier: ClassifierRuntimeConfig,
    pub(crate) xai: XaiRuntimeConfig,
    pub(crate) openai_compatible: OpenAiCompatibleRuntimeConfig,
    pub(crate) exa: ExaRuntimeConfig,
    pub(crate) context7: Context7RuntimeConfig,
    pub(crate) anysearch: AnysearchRuntimeConfig,
    pub(crate) tavily: WebFetchProviderConfig,
    pub(crate) firecrawl: WebFetchProviderConfig,
    pub(crate) jina: WebFetchProviderConfig,
    pub(crate) docs_search: DocsSearchRuntimeConfig,
    pub(crate) vertical_search: VerticalSearchRuntimeConfig,
    pub(crate) web_search: WebSearchRuntimeConfig,
    pub(crate) web_fetch: WebFetchRuntimeConfig,
    pub(crate) journal: JournalRuntimeConfig,
    pub(crate) retry: RetryRuntimeConfig,
    pub(crate) log_level: LogLevel,
    pub(crate) ssl_verify: bool,
}

pub(crate) struct ProviderRuntime<'a> {
    pub(crate) endpoint: &'a str,
    pub(crate) keys: &'a [Secret],
}

impl RuntimeConfig {
    pub(crate) fn provider_runtime(&self, id: ProviderId) -> ProviderRuntime<'_> {
        match id {
            ProviderId::Xai => ProviderRuntime {
                endpoint: &self.xai.url,
                keys: &self.xai.keys,
            },
            ProviderId::OpenAiCompatible => ProviderRuntime {
                endpoint: &self.openai_compatible.url,
                keys: &self.openai_compatible.keys,
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
            ProviderId::Context7 => ProviderRuntime {
                endpoint: &self.context7.url,
                keys: &self.context7.keys,
            },
            ProviderId::Anysearch => ProviderRuntime {
                endpoint: &self.anysearch.url,
                keys: &self.anysearch.keys,
            },
        }
    }
}

// Runtime assembly mirrors the complete validated configuration surface in one place.
#[expect(clippy::too_many_lines)]
pub(crate) fn runtime_config() -> Result<RuntimeConfig, ConfigError> {
    let loaded = load_effective_config()?;
    let config = loaded.config;
    let fallback = config
        .search
        .fallback
        .parse()
        .map_err(ConfigError::Message)?;
    let config_dir = ConfigLocation::discover()?.config_dir;
    let journal = JournalRuntimeConfig {
        enabled: config.journal.enabled,
        dir: resolve_journal_dir(&config.journal.dir, &config_dir)?,
        retention_days: config.journal.retention_days,
        credentials: configured_credentials(&config),
    };
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
    let context7 = Context7RuntimeConfig {
        url: config.providers.context7.url,
        keys: config.providers.context7.keys,
        timeout_seconds: config.providers.context7.timeout,
    };
    let anysearch = AnysearchRuntimeConfig {
        url: config.providers.anysearch.url,
        keys: config.providers.anysearch.keys,
        timeout_seconds: config.providers.anysearch.timeout,
    };
    let classifier = ClassifierRuntimeConfig {
        url: config.classifier.url,
        keys: config.classifier.keys,
        model: config.classifier.model,
        fallback_models: config.classifier.fallback_models,
        timeout_seconds: config.classifier.timeout,
    };
    let xai = XaiRuntimeConfig {
        url: config.providers.xai.url,
        keys: config.providers.xai.keys,
        model: config.providers.xai.model,
        tools: config.providers.xai.tools,
    };
    let openai_compatible = OpenAiCompatibleRuntimeConfig {
        url: config.providers.openai_compatible.url,
        keys: config.providers.openai_compatible.keys,
        model: config.providers.openai_compatible.model,
        fallback_models: config.providers.openai_compatible.fallback_models,
        stream: config.providers.openai_compatible.stream,
    };
    let main_entries = main_search_entries(config.search.backends, &xai, &openai_compatible)?;
    let docs_entries = docs_search_entries(config.capabilities.docs_search.order, &exa, &context7)?;
    let vertical_entries =
        vertical_search_entries(config.capabilities.vertical_search.order, &anysearch)?;
    let web_search_entries = web_entries(
        config.capabilities.web_search.order,
        "web_search",
        &tavily,
        &firecrawl,
        &jina,
    )?;
    let web_fetch_entries = web_entries(
        config.capabilities.web_fetch.order,
        "web_fetch",
        &tavily,
        &firecrawl,
        &jina,
    )?;
    Ok(RuntimeConfig {
        main_search: MainSearchRuntimeConfig {
            entries: main_entries,
            fallback,
        },
        classifier,
        xai,
        openai_compatible,
        exa,
        context7,
        anysearch,
        tavily,
        firecrawl,
        jina,
        docs_search: DocsSearchRuntimeConfig {
            entries: docs_entries,
        },
        vertical_search: VerticalSearchRuntimeConfig {
            entries: vertical_entries,
        },
        web_search: WebSearchRuntimeConfig {
            entries: web_search_entries,
        },
        web_fetch: WebFetchRuntimeConfig {
            entries: web_fetch_entries,
        },
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
    })
}

fn unknown_provider(name: &str, seam: &str) -> ConfigError {
    ConfigError::Message(format!("{seam} contains unknown provider `{name}`"))
}

fn main_search_entries(
    order: Vec<String>,
    xai: &XaiRuntimeConfig,
    openai: &OpenAiCompatibleRuntimeConfig,
) -> Result<Vec<SeamEntry<MainSearchProviderConfig>>, ConfigError> {
    order
        .into_iter()
        .map(|name| {
            let id = ProviderId::parse(&name)
                .ok_or_else(|| unknown_provider(&name, "search.backends"))?;
            let config = main_provider_config(id, xai, openai)
                .ok_or_else(|| unknown_provider(&name, "search.backends"))?;
            let configured = config.configured();
            Ok(SeamEntry::new(id, config, configured))
        })
        .collect()
}

fn docs_search_entries(
    order: Vec<String>,
    exa: &ExaRuntimeConfig,
    context7: &Context7RuntimeConfig,
) -> Result<Vec<SeamEntry<DocsSearchProviderConfig>>, ConfigError> {
    order
        .into_iter()
        .map(|name| {
            let seam = "capabilities.docs_search.order";
            let id = ProviderId::parse(&name).ok_or_else(|| unknown_provider(&name, seam))?;
            let config = docs_provider_config(id, exa, context7)
                .ok_or_else(|| unknown_provider(&name, seam))?;
            let configured = config.configured();
            Ok(SeamEntry::new(id, config, configured))
        })
        .collect()
}

fn vertical_search_entries(
    order: Vec<String>,
    anysearch: &AnysearchRuntimeConfig,
) -> Result<Vec<SeamEntry<AnysearchRuntimeConfig>>, ConfigError> {
    order
        .into_iter()
        .map(|name| {
            let seam = "capabilities.vertical_search.order";
            let id = ProviderId::parse(&name).ok_or_else(|| unknown_provider(&name, seam))?;
            let config = vertical_provider_config(id, anysearch)
                .ok_or_else(|| unknown_provider(&name, seam))?;
            let configured = !config.keys.is_empty();
            Ok(SeamEntry::new(id, config, configured))
        })
        .collect()
}

fn web_entries(
    order: Vec<String>,
    seam: &str,
    tavily: &WebFetchProviderConfig,
    firecrawl: &WebFetchProviderConfig,
    jina: &WebFetchProviderConfig,
) -> Result<Vec<SeamEntry<WebFetchProviderConfig>>, ConfigError> {
    let catalog = catalog::by_seam(seam).ok_or_else(|| unknown_provider("", seam))?;
    order
        .into_iter()
        .map(|name| {
            let id = ProviderId::parse(&name).ok_or_else(|| unknown_provider(&name, seam))?;
            let config = web_provider_config(catalog, id, tavily, firecrawl, jina)
                .ok_or_else(|| unknown_provider(&name, seam))?;
            let configured = !config.keys.is_empty();
            Ok(SeamEntry::new(id, config, configured))
        })
        .collect()
}

pub(crate) fn main_provider_config(
    id: ProviderId,
    xai: &XaiRuntimeConfig,
    openai: &OpenAiCompatibleRuntimeConfig,
) -> Option<MainSearchProviderConfig> {
    match id {
        ProviderId::Xai => Some(MainSearchProviderConfig::Xai(xai.clone())),
        ProviderId::OpenAiCompatible => {
            Some(MainSearchProviderConfig::OpenAiCompatible(openai.clone()))
        }
        _ => None,
    }
}

pub(crate) fn docs_provider_config(
    id: ProviderId,
    exa: &ExaRuntimeConfig,
    context7: &Context7RuntimeConfig,
) -> Option<DocsSearchProviderConfig> {
    match id {
        ProviderId::Exa => Some(DocsSearchProviderConfig::Exa(exa.clone())),
        ProviderId::Context7 => Some(DocsSearchProviderConfig::Context7(context7.clone())),
        _ => None,
    }
}

pub(crate) fn web_provider_config(
    catalog: catalog::CapabilityCatalog,
    id: ProviderId,
    tavily: &WebFetchProviderConfig,
    firecrawl: &WebFetchProviderConfig,
    jina: &WebFetchProviderConfig,
) -> Option<WebFetchProviderConfig> {
    if !catalog.contains(id) {
        return None;
    }
    match id {
        ProviderId::Tavily => Some(tavily.clone()),
        ProviderId::Firecrawl => Some(firecrawl.clone()),
        ProviderId::Jina => Some(jina.clone()),
        _ => None,
    }
}

pub(crate) fn vertical_provider_config(
    id: ProviderId,
    anysearch: &AnysearchRuntimeConfig,
) -> Option<AnysearchRuntimeConfig> {
    catalog::VERTICAL_SEARCH
        .contains(id)
        .then(|| anysearch.clone())
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
    SCHEMA
        .iter()
        .filter_map(|leaf| match (leaf.get)(config) {
            FieldRef::Secrets(keys) => Some(keys),
            _ => None,
        })
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
