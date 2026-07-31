//! Configuration schema, layered loading, effective views, and private writes.

use std::collections::{BTreeMap, HashSet};
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use thiserror::Error;
use toml_edit::{Array, Document, DocumentMut, Item, Table, TableLike, Value};

use crate::providers;

static FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static WRITE_PROBE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
const LOCK_WAIT: Duration = Duration::from_millis(100);
pub(crate) const CREDENTIAL_MASK: &str = "********";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Config {
    search: Search,
    classifier: Classifier,
    providers: Providers,
    capabilities: Capabilities,
    log: Log,
    journal: Journal,
    retry: Retry,
    http: Http,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Search {
    backends: Vec<String>,
    validation: String,
    fallback: String,
}

impl Default for Search {
    fn default() -> Self {
        Self {
            backends: vec!["xai".into(), "openai_compatible".into()],
            validation: "balanced".into(),
            fallback: "auto".into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Classifier {
    url: String,
    keys: Vec<String>,
    model: String,
    fallback_models: Vec<String>,
    timeout: i64,
}

impl Default for Classifier {
    fn default() -> Self {
        Self {
            url: String::new(),
            keys: Vec::new(),
            model: String::new(),
            fallback_models: Vec::new(),
            timeout: 30,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Providers {
    xai: Xai,
    openai_compatible: OpenAiCompatible,
    exa: Endpoint<ExaEndpoint>,
    context7: Endpoint<Context7Endpoint>,
    jina: Jina,
    tavily: Endpoint<TavilyEndpoint>,
    firecrawl: Endpoint<FirecrawlEndpoint>,
    anysearch: Endpoint<AnysearchEndpoint>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Xai {
    url: String,
    keys: Vec<String>,
    model: String,
    tools: Vec<String>,
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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct OpenAiCompatible {
    url: String,
    keys: Vec<String>,
    model: String,
    fallback_models: Vec<String>,
    stream: bool,
}

impl Default for OpenAiCompatible {
    fn default() -> Self {
        Self {
            url: String::new(),
            keys: Vec::new(),
            model: "grok-4-fast".into(),
            fallback_models: Vec::new(),
            stream: false,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Endpoint<D: EndpointDefaults> {
    url: String,
    keys: Vec<String>,
    timeout: i64,
    #[serde(skip)]
    defaults: PhantomData<D>,
}

impl<D: EndpointDefaults> Default for Endpoint<D> {
    fn default() -> Self {
        Self {
            url: D::URL.into(),
            keys: Vec::new(),
            timeout: 30,
            defaults: PhantomData,
        }
    }
}

trait EndpointDefaults {
    const URL: &'static str;
}

macro_rules! endpoint_defaults {
    ($name:ident, $url:literal) => {
        #[derive(Clone, Debug)]
        struct $name;

        impl EndpointDefaults for $name {
            const URL: &'static str = $url;
        }
    };
}

endpoint_defaults!(ExaEndpoint, "https://api.exa.ai");
endpoint_defaults!(Context7Endpoint, "https://mcp.context7.com/mcp");
endpoint_defaults!(TavilyEndpoint, "https://api.tavily.com");
endpoint_defaults!(FirecrawlEndpoint, "https://api.firecrawl.dev/v2");
endpoint_defaults!(AnysearchEndpoint, "https://api.anysearch.com/mcp");

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Jina {
    url: String,
    keys: Vec<String>,
    respond_with: String,
    timeout: i64,
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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Capabilities {
    web_search: Order,
    web_fetch: Order,
    docs_search: Order,
    vertical_search: Order,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            web_search: Order::new(&["tavily", "firecrawl"]),
            web_fetch: Order::new(&["jina", "tavily", "firecrawl"]),
            docs_search: Order::new(&["context7", "exa"]),
            vertical_search: Order::new(&["anysearch"]),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Order {
    order: Vec<String>,
}

impl Order {
    fn new(values: &[&str]) -> Self {
        Self {
            order: values.iter().map(ToString::to_string).collect(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Log {
    level: String,
}

impl Default for Log {
    fn default() -> Self {
        Self {
            level: "info".into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Journal {
    enabled: bool,
    dir: String,
    retention_days: i64,
}

impl Default for Journal {
    fn default() -> Self {
        Self {
            enabled: true,
            dir: "~/.local/state/forager/journal".into(),
            retention_days: 30,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Retry {
    max_attempts: i64,
    multiplier: f64,
    max_wait: i64,
}

impl Default for Retry {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            multiplier: 1.0,
            max_wait: 10,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Http {
    ssl_verify: bool,
}

impl Default for Http {
    fn default() -> Self {
        Self { ssl_verify: true }
    }
}

/// A serialized, credential-safe view of the effective configuration.
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct EffectiveConfigView(JsonValue);

#[derive(Clone, Debug)]
pub(crate) struct ExaRuntimeConfig {
    pub(crate) url: String,
    pub(crate) keys: Vec<String>,
    pub(crate) timeout_seconds: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct XaiRuntimeConfig {
    pub(crate) url: String,
    pub(crate) keys: Vec<String>,
    pub(crate) model: String,
    pub(crate) tools: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct OpenAiCompatibleRuntimeConfig {
    pub(crate) url: String,
    pub(crate) keys: Vec<String>,
    pub(crate) model: String,
    pub(crate) fallback_models: Vec<String>,
    pub(crate) stream: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct ClassifierRuntimeConfig {
    pub(crate) url: String,
    pub(crate) keys: Vec<String>,
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

    pub(crate) fn keys(&self) -> &[String] {
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
    pub(crate) credentials: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct Context7RuntimeConfig {
    pub(crate) url: String,
    pub(crate) keys: Vec<String>,
    pub(crate) timeout_seconds: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct AnysearchRuntimeConfig {
    pub(crate) url: String,
    pub(crate) keys: Vec<String>,
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
    pub(crate) keys: Vec<String>,
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

struct LoadedConfig {
    config: Config,
    file_value: toml::Value,
    env_paths: HashSet<String>,
}

/// Loads and validates the complete effective configuration.
///
/// # Errors
///
/// Returns a configuration error for malformed files, unknown keys, invalid
/// values, or unknown `FORAGER_*` variables.
pub fn effective_view() -> Result<EffectiveConfigView, ConfigError> {
    let loaded = load_effective_config()?;
    Ok(EffectiveConfigView(build_view(
        &loaded.config,
        &loaded.file_value,
        &loaded.env_paths,
    )))
}

pub(crate) fn runtime_config() -> Result<RuntimeConfig, ConfigError> {
    let loaded = load_effective_config()?;
    let config = loaded.config;
    let config_dir = ConfigLocation::discover()?.config_dir;
    let journal = JournalRuntimeConfig {
        enabled: config.journal.enabled,
        dir: resolve_journal_dir(&config.journal.dir, &config_dir)?,
        retention_days: config.journal.retention_days as u64,
        credentials: configured_credentials(&config),
    };
    let tavily = WebFetchProviderConfig {
        url: config.providers.tavily.url,
        keys: config.providers.tavily.keys,
        timeout_seconds: config.providers.tavily.timeout as u64,
        respond_with: String::new(),
    };
    let firecrawl = WebFetchProviderConfig {
        url: config.providers.firecrawl.url,
        keys: config.providers.firecrawl.keys,
        timeout_seconds: config.providers.firecrawl.timeout as u64,
        respond_with: String::new(),
    };
    let exa = ExaRuntimeConfig {
        url: config.providers.exa.url,
        keys: config.providers.exa.keys,
        timeout_seconds: config.providers.exa.timeout as u64,
    };
    let context7 = Context7RuntimeConfig {
        url: config.providers.context7.url,
        keys: config.providers.context7.keys,
        timeout_seconds: config.providers.context7.timeout as u64,
    };
    let anysearch = AnysearchRuntimeConfig {
        url: config.providers.anysearch.url,
        keys: config.providers.anysearch.keys,
        timeout_seconds: config.providers.anysearch.timeout as u64,
    };
    let classifier = ClassifierRuntimeConfig {
        url: config.classifier.url,
        keys: config.classifier.keys,
        model: config.classifier.model,
        fallback_models: config.classifier.fallback_models,
        timeout_seconds: config.classifier.timeout as u64,
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
                        timeout_seconds: config.providers.jina.timeout as u64,
                        respond_with: config.providers.jina.respond_with,
                    },
                ),
                ("tavily".into(), tavily),
                ("firecrawl".into(), firecrawl),
            ]),
        },
        journal,
        retry: RetryRuntimeConfig {
            max_attempts: config.retry.max_attempts as usize,
            multiplier: config.retry.multiplier,
            max_wait_seconds: config.retry.max_wait as u64,
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

fn configured_credentials(config: &Config) -> Vec<String> {
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

fn load_effective_config() -> Result<LoadedConfig, ConfigError> {
    let location = ConfigLocation::discover()?;
    let path = location.config_file();
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(ConfigError::io(&path, error)),
    };
    let mut config = if content.is_empty() {
        Config::default()
    } else {
        let deserializer =
            toml::Deserializer::parse(&content).map_err(|error| ConfigError::Document {
                path: path.clone(),
                detail: diagnostic_without_source(&error.to_string()),
            })?;
        serde_path_to_error::deserialize(deserializer).map_err(|error| {
            let key_path = error.path().to_string();
            let detail = if key_path.ends_with(".keys") || key_path == "classifier.keys" {
                let location = error
                    .inner()
                    .to_string()
                    .lines()
                    .find(|line| line.contains("TOML parse error"))
                    .unwrap_or("invalid credential array")
                    .to_owned();
                format!("{location}\ninvalid credential array")
            } else {
                diagnostic_without_source(&error.inner().to_string())
            };
            ConfigError::Document {
                path: path.clone(),
                detail: if key_path.is_empty() {
                    detail
                } else {
                    format!("key `{key_path}`: {detail}")
                },
            }
        })?
    };
    let file_value = if content.is_empty() {
        toml::Value::Table(toml::map::Map::new())
    } else {
        toml::from_str::<toml::Value>(&content).map_err(|error| ConfigError::Document {
            path: path.clone(),
            detail: diagnostic_without_source(&error.to_string()),
        })?
    };
    let mut env_paths = HashSet::new();
    apply_environment(&mut config, &mut env_paths)?;
    normalize_credentials(&mut config);
    validate(&config, &path, &content)?;

    Ok(LoadedConfig {
        config,
        file_value,
        env_paths,
    })
}

/// Serializes the shared effective configuration view.
///
/// # Errors
///
/// Returns a configuration or JSON serialization error.
pub fn effective_view_json() -> Result<String, ConfigError> {
    serde_json::to_string(&effective_view()?)
        .map_err(|error| ConfigError::Message(error.to_string()))
}

/// Sets one schema leaf in the file layer without strictly loading other keys.
///
/// # Errors
///
/// Returns an argument error for an invalid target or value, and a
/// configuration error when the document cannot be parsed or written.
pub fn set_file_value(path: &str, raw: &str) -> Result<(), EditError> {
    let value = parse_edit_value(path, raw)?;
    let location = ConfigLocation::discover().map_err(EditError::Config)?;
    let file = location.config_file();
    let _lock = acquire_location_lock(&location).map_err(EditError::Config)?;
    let content = read_edit_document(&file)?;
    let mut document = parse_edit_document(&file, &content)?;
    set_document_path(&mut document, path, value)?;
    atomic_write(&location.config_dir, &file, document.to_string().as_bytes())
        .map_err(|error| EditError::Config(ConfigError::io(&file, error)))
}

/// Removes one schema leaf from the file layer without strictly loading it.
///
/// Returns whether an environment value for the same leaf remains effective.
///
/// # Errors
///
/// Returns an argument error for an invalid target and a configuration error
/// when the document cannot be parsed or written.
pub fn unset_file_value(path: &str) -> Result<bool, EditError> {
    if !is_leaf(path) {
        return Err(EditError::Argument(format!(
            "unknown configuration key `{path}`"
        )));
    }
    let location = ConfigLocation::discover().map_err(EditError::Config)?;
    let file = location.config_file();
    let _lock = acquire_location_lock(&location).map_err(EditError::Config)?;
    let content = read_edit_document(&file)?;
    let mut document = parse_edit_document(&file, &content)?;
    remove_document_path(&mut document, path);
    atomic_write(&location.config_dir, &file, document.to_string().as_bytes())
        .map_err(|error| EditError::Config(ConfigError::io(&file, error)))?;
    Ok(env::var_os(env_name(path)).is_some())
}

/// A parseable configuration document being updated by the setup wizard.
pub struct SetupDocument {
    _lock: File,
    location: ConfigLocation,
    file: PathBuf,
    document: DocumentMut,
    defaults: DocumentMut,
}

impl SetupDocument {
    /// Loads the existing document without validating unrelated schema values.
    ///
    /// # Errors
    ///
    /// Returns a configuration error when the file cannot be read or parsed.
    pub fn load() -> Result<Self, EditError> {
        let location = ConfigLocation::discover().map_err(EditError::Config)?;
        let file = location.config_file();
        let lock = acquire_location_lock(&location).map_err(EditError::Config)?;
        let content = read_edit_document(&file)?;
        let document = parse_edit_document(&file, &content)?;
        let defaults = default_document().map_err(EditError::Config)?;
        Ok(Self {
            _lock: lock,
            location,
            file,
            document,
            defaults,
        })
    }

    /// Returns the first configured search backend, falling back to `xai`.
    pub fn primary_backend(&self) -> &str {
        document_array(&self.document, "search.backends")
            .and_then(|array| array.iter().find_map(Value::as_str))
            .filter(|backend| matches!(*backend, "xai" | "openai_compatible"))
            .unwrap_or("xai")
    }

    /// Returns a string leaf from the file or built-in defaults.
    pub fn string(&self, path: &str) -> &str {
        document_string(&self.document, path)
            .or_else(|| document_string(&self.defaults, path))
            .unwrap_or_default()
    }

    /// Returns whether the existing file contains classifier model configuration.
    pub fn classifier_is_configured(&self) -> bool {
        ["classifier.url", "classifier.model"].iter().any(|path| {
            document_string(&self.document, path).is_some_and(|value| !value.is_empty())
        }) || document_array(&self.document, "classifier.keys").is_some_and(|keys| {
            keys.iter()
                .any(|key| key.as_str().is_some_and(|key| !key.is_empty()))
        })
    }

    /// Updates a string leaf in memory.
    ///
    /// # Errors
    ///
    /// Returns an argument error when the path or value is invalid.
    pub fn set_string(&mut self, path: &str, value: &str) -> Result<(), EditError> {
        let value = parse_edit_value(path, value)?;
        set_document_path(&mut self.document, path, value)
    }

    /// Updates a string-array leaf in memory.
    ///
    /// # Errors
    ///
    /// Returns an argument error when the path or values are invalid.
    pub fn set_strings(&mut self, path: &str, values: &[String]) -> Result<(), EditError> {
        if !matches!(path_kind(path), Some(ValueKind::Array)) {
            return Err(invalid_value(path));
        }
        let mut array = Array::new();
        for value in values {
            array.push(value.as_str());
        }
        let value = if path.ends_with(".keys") || path == "classifier.keys" {
            Value::Array(normalize_array(array))
        } else {
            Value::Array(array)
        };
        validate_edit_value(path, &value)?;
        set_document_path(&mut self.document, path, value)
    }

    /// Persists all wizard changes with one atomic replacement.
    ///
    /// # Errors
    ///
    /// Returns a configuration error when the document cannot be written.
    pub fn save(self) -> Result<(), EditError> {
        atomic_write(
            &self.location.config_dir,
            &self.file,
            self.document.to_string().as_bytes(),
        )
        .map_err(|error| EditError::Config(ConfigError::io(&self.file, error)))
    }
}

/// Creates the complete commented configuration template without overwriting.
///
/// # Errors
///
/// Returns a configuration error when the target exists or cannot be created.
pub fn create_setup_template() -> Result<PathBuf, ConfigError> {
    let location = ConfigLocation::discover()?;
    let file = location.config_file();
    let _lock = acquire_location_lock(&location)?;
    if file
        .try_exists()
        .map_err(|error| ConfigError::io(&file, error))?
    {
        return Err(ConfigError::Message(format!(
            "{} already exists; refusing to overwrite",
            file.display()
        )));
    }

    let document = default_document()?;
    let template = commented_template(&document)?;
    atomic_create(&location.config_dir, &file, template.as_bytes())
        .map_err(|error| ConfigError::io(&file, error))?;
    Ok(file)
}

fn default_document() -> Result<DocumentMut, ConfigError> {
    toml::to_string(&Config::default())
        .map_err(|error| ConfigError::Message(error.to_string()))?
        .parse::<DocumentMut>()
        .map_err(|error| ConfigError::Message(error.to_string()))
}

fn document_item<'a>(document: &'a DocumentMut, path: &str) -> Option<&'a Item> {
    let mut table: &dyn TableLike = document.as_table();
    let mut segments = path.split('.').peekable();
    while let Some(segment) = segments.next() {
        let item = table.get(segment)?;
        if segments.peek().is_none() {
            return Some(item);
        }
        table = item.as_table_like()?;
    }
    None
}

fn document_string<'a>(document: &'a DocumentMut, path: &str) -> Option<&'a str> {
    document_item(document, path)?.as_str()
}

fn document_array<'a>(document: &'a DocumentMut, path: &str) -> Option<&'a Array> {
    document_item(document, path)?.as_array()
}

fn commented_template(document: &DocumentMut) -> Result<String, ConfigError> {
    let mut table = String::new();
    let mut template = String::new();
    let mut annotated = 0;
    for line in document.to_string().lines() {
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            table.clear();
            table.push_str(name);
        } else if let Some((key, _)) = line.split_once(" = ") {
            let path = if table.is_empty() {
                key.to_owned()
            } else {
                format!("{table}.{key}")
            };
            if !is_leaf(&path) {
                return Err(ConfigError::Message(format!(
                    "built-in configuration contains unknown `{path}`"
                )));
            }
            template.push_str("# ");
            template.push_str(&template_comment(&path));
            template.push('\n');
            annotated += 1;
        }
        template.push_str(line);
        template.push('\n');
    }
    if annotated != LEAVES.len() {
        return Err(ConfigError::Message(
            "built-in configuration does not cover the complete key surface".into(),
        ));
    }
    Ok(template)
}

fn template_comment(path: &str) -> String {
    let purpose = if path.ends_with(".keys") || path == "classifier.keys" {
        "credential pool; keep empty until credentials are available"
    } else if path.ends_with(".url") || path == "classifier.url" {
        "service endpoint URL"
    } else if path.ends_with(".timeout") || path == "classifier.timeout" {
        "shared timeout in seconds; must be greater than zero"
    } else if path.ends_with(".model") || path == "classifier.model" {
        "model identifier"
    } else if path.ends_with(".fallback_models") || path == "classifier.fallback_models" {
        "ordered fallback model identifiers"
    } else if path.ends_with(".order") {
        "authoritative provider order for this capability"
    } else {
        match path {
            "search.backends" => "ordered main-model backends",
            "search.validation" => "result validation level: fast, balanced, or strict",
            "search.fallback" => "fallback policy: auto or off",
            "providers.xai.tools" => "xAI tools enabled for the main model",
            "providers.openai_compatible.stream" => "enable streaming transport",
            "providers.jina.respond_with" => "optional X-Respond-With header value",
            "log.level" => "stderr log level",
            "journal.enabled" => "record search result journals",
            "journal.dir" => "journal storage directory",
            "journal.retention_days" => {
                "journal retention in days; zero keeps entries indefinitely"
            }
            "retry.max_attempts" => "maximum attempts per request",
            "retry.multiplier" => "retry backoff multiplier",
            "retry.max_wait" => "maximum retry wait in seconds",
            "http.ssl_verify" => "verify TLS certificates",
            _ => "configuration value",
        }
    };
    format!("{path}: {purpose}")
}

fn read_edit_document(path: &Path) -> Result<String, EditError> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(content),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(EditError::Config(ConfigError::io(path, error))),
    }
}

fn parse_edit_document(path: &Path, content: &str) -> Result<DocumentMut, EditError> {
    content.parse::<DocumentMut>().map_err(|error| {
        EditError::Config(ConfigError::Document {
            path: path.to_path_buf(),
            detail: diagnostic_without_source(&error.to_string()),
        })
    })
}

fn diagnostic_without_source(detail: &str) -> String {
    detail
        .lines()
        .filter(|line| !line.contains(" |") && line.trim() != "|")
        .collect::<Vec<_>>()
        .join("\n")
}

fn set_document_path(
    document: &mut DocumentMut,
    path: &str,
    value: Value,
) -> Result<(), EditError> {
    let mut segments = path.split('.').peekable();
    let mut table = document.as_table_mut();
    while let Some(segment) = segments.next() {
        if segments.peek().is_none() {
            table.insert(segment, Item::Value(value));
            return Ok(());
        }
        let item = table
            .entry(segment)
            .or_insert_with(|| Item::Table(Table::new()));
        table = item.as_table_mut().ok_or_else(|| {
            EditError::Config(ConfigError::Message(format!(
                "cannot set `{path}` because `{segment}` is not a table"
            )))
        })?;
    }
    Err(EditError::Argument(format!(
        "unknown configuration key `{path}`"
    )))
}

fn remove_document_path(document: &mut DocumentMut, path: &str) {
    let segments: Vec<_> = path.split('.').collect();
    let Some((leaf, parents)) = segments.split_last() else {
        return;
    };
    let mut table = document.as_table_mut();
    for segment in parents {
        let Some(next) = table.get_mut(segment).and_then(Item::as_table_mut) else {
            return;
        };
        table = next;
    }
    table.remove(leaf);
}

fn parse_edit_value(path: &str, raw: &str) -> Result<Value, EditError> {
    if !is_leaf(path) {
        return Err(EditError::Argument(format!(
            "unknown configuration key `{path}`"
        )));
    }
    let value = match path_kind(path) {
        Some(ValueKind::String) => Value::from(raw),
        Some(ValueKind::Boolean) => raw
            .parse::<bool>()
            .map(Value::from)
            .map_err(|_| invalid_value(path))?,
        Some(ValueKind::Integer) => raw
            .parse::<i64>()
            .map(Value::from)
            .map_err(|_| invalid_value(path))?,
        Some(ValueKind::Float) => raw
            .parse::<f64>()
            .map(Value::from)
            .map_err(|_| invalid_value(path))?,
        Some(ValueKind::Array) => {
            let document = format!("value = {raw}")
                .parse::<DocumentMut>()
                .map_err(|_| invalid_value(path))?;
            let array = document["value"]
                .as_array()
                .cloned()
                .ok_or_else(|| invalid_value(path))?;
            if array.iter().any(|item| item.as_str().is_none()) {
                return Err(invalid_value(path));
            }
            if path.ends_with(".keys") || path == "classifier.keys" {
                Value::Array(normalize_array(array))
            } else {
                Value::Array(array)
            }
        }
        None => return Err(invalid_value(path)),
    };
    validate_edit_value(path, &value)?;
    Ok(value)
}

fn normalize_array(array: Array) -> Array {
    let mut seen = HashSet::new();
    array
        .iter()
        .filter_map(Value::as_str)
        .filter(|value| !value.is_empty())
        .filter(|value| seen.insert((*value).to_owned()))
        .fold(Array::new(), |mut normalized, value| {
            normalized.push(value);
            normalized
        })
}

fn validate_edit_value(path: &str, value: &Value) -> Result<(), EditError> {
    let valid = match path {
        "search.backends" => valid_unique_array(value, &["xai", "openai_compatible"], false),
        "search.validation" => string_in(value, &["fast", "balanced", "strict"]),
        "search.fallback" => string_in(value, &["auto", "off"]),
        "providers.xai.tools" => valid_array(value, &["web_search", "x_search"], true),
        "classifier.timeout"
        | "providers.exa.timeout"
        | "providers.context7.timeout"
        | "providers.jina.timeout"
        | "providers.tavily.timeout"
        | "providers.firecrawl.timeout"
        | "providers.anysearch.timeout" => value.as_integer().is_some_and(|number| number > 0),
        "retry.max_attempts" => value.as_integer().is_some_and(|number| number >= 1),
        "retry.multiplier" => value
            .as_float()
            .is_some_and(|number| number.is_finite() && number > 0.0),
        "retry.max_wait" | "journal.retention_days" => {
            value.as_integer().is_some_and(|number| number >= 0)
        }
        "log.level" => string_in(value, &["error", "warn", "info", "debug", "trace"]),
        "capabilities.web_search.order"
        | "capabilities.web_fetch.order"
        | "capabilities.docs_search.order"
        | "capabilities.vertical_search.order" => {
            let capability = path.split('.').nth(1).unwrap_or_default();
            let non_empty = path != "capabilities.web_fetch.order"
                || value.as_array().is_some_and(|array| !array.is_empty());
            non_empty
                && value.as_array().is_some_and(|array| {
                    array.iter().all(|provider| {
                        provider
                            .as_str()
                            .is_some_and(|provider| providers::supports(capability, provider))
                    })
                })
        }
        _ => true,
    };
    if valid {
        Ok(())
    } else {
        Err(invalid_value(path))
    }
}

fn valid_unique_array(value: &Value, allowed: &[&str], allow_empty: bool) -> bool {
    let Some(array) = value.as_array() else {
        return false;
    };
    let mut seen = HashSet::new();
    (allow_empty || !array.is_empty())
        && array.iter().all(|entry| {
            entry
                .as_str()
                .is_some_and(|entry| allowed.contains(&entry) && seen.insert(entry))
        })
}

fn valid_array(value: &Value, allowed: &[&str], allow_empty: bool) -> bool {
    value.as_array().is_some_and(|array| {
        (allow_empty || !array.is_empty())
            && array
                .iter()
                .all(|entry| entry.as_str().is_some_and(|entry| allowed.contains(&entry)))
    })
}

fn string_in(value: &Value, allowed: &[&str]) -> bool {
    value.as_str().is_some_and(|value| allowed.contains(&value))
}

fn invalid_value(path: &str) -> EditError {
    EditError::Argument(format!("invalid value for `{path}`"))
}

fn apply_environment(
    config: &mut Config,
    env_paths: &mut HashSet<String>,
) -> Result<(), ConfigError> {
    for (name, value) in env::vars_os() {
        let visible_name = name.to_string_lossy();
        if !visible_name.starts_with("FORAGER_") || visible_name == "FORAGER_CONFIG_DIR" {
            continue;
        }
        let name = name.into_string().map_err(|_| {
            ConfigError::Message("unknown non-Unicode FORAGER_* environment variable".into())
        })?;
        let path = env_path(&name).ok_or_else(|| {
            ConfigError::Message(format!(
                "unknown configuration environment variable `{name}`"
            ))
        })?;
        let value = value.into_string().map_err(|_| {
            ConfigError::Message(format!(
                "invalid value for configuration environment variable `{name}`"
            ))
        })?;
        parse_edit_value(path, &value).map_err(|_| {
            ConfigError::Message(format!(
                "invalid value for configuration environment variable `{name}`"
            ))
        })?;
        apply_env_value(config, path, &value).map_err(|_| {
            ConfigError::Message(format!(
                "invalid value for configuration environment variable `{name}`"
            ))
        })?;
        env_paths.insert(path.to_owned());
    }
    Ok(())
}

fn apply_env_value(config: &mut Config, path: &str, raw: &str) -> Result<(), ()> {
    macro_rules! string {
        ($target:expr) => {{
            $target = raw.to_owned();
            Ok(())
        }};
    }
    macro_rules! integer {
        ($target:expr) => {{
            $target = raw.parse().map_err(|_| ())?;
            Ok(())
        }};
    }
    macro_rules! boolean {
        ($target:expr) => {{
            $target = raw.parse().map_err(|_| ())?;
            Ok(())
        }};
    }
    macro_rules! array {
        ($target:expr) => {{
            $target = parse_string_array(raw)?;
            Ok(())
        }};
    }
    match path {
        "search.backends" => array!(config.search.backends),
        "search.validation" => string!(config.search.validation),
        "search.fallback" => string!(config.search.fallback),
        "classifier.url" => string!(config.classifier.url),
        "classifier.keys" => array!(config.classifier.keys),
        "classifier.model" => string!(config.classifier.model),
        "classifier.fallback_models" => array!(config.classifier.fallback_models),
        "classifier.timeout" => integer!(config.classifier.timeout),
        "providers.xai.url" => string!(config.providers.xai.url),
        "providers.xai.keys" => array!(config.providers.xai.keys),
        "providers.xai.model" => string!(config.providers.xai.model),
        "providers.xai.tools" => array!(config.providers.xai.tools),
        "providers.openai_compatible.url" => string!(config.providers.openai_compatible.url),
        "providers.openai_compatible.keys" => array!(config.providers.openai_compatible.keys),
        "providers.openai_compatible.model" => string!(config.providers.openai_compatible.model),
        "providers.openai_compatible.fallback_models" => {
            array!(config.providers.openai_compatible.fallback_models)
        }
        "providers.openai_compatible.stream" => boolean!(config.providers.openai_compatible.stream),
        "providers.exa.url" => string!(config.providers.exa.url),
        "providers.exa.keys" => array!(config.providers.exa.keys),
        "providers.exa.timeout" => integer!(config.providers.exa.timeout),
        "providers.context7.url" => string!(config.providers.context7.url),
        "providers.context7.keys" => array!(config.providers.context7.keys),
        "providers.context7.timeout" => integer!(config.providers.context7.timeout),
        "providers.jina.url" => string!(config.providers.jina.url),
        "providers.jina.keys" => array!(config.providers.jina.keys),
        "providers.jina.respond_with" => string!(config.providers.jina.respond_with),
        "providers.jina.timeout" => integer!(config.providers.jina.timeout),
        "providers.tavily.url" => string!(config.providers.tavily.url),
        "providers.tavily.keys" => array!(config.providers.tavily.keys),
        "providers.tavily.timeout" => integer!(config.providers.tavily.timeout),
        "providers.firecrawl.url" => string!(config.providers.firecrawl.url),
        "providers.firecrawl.keys" => array!(config.providers.firecrawl.keys),
        "providers.firecrawl.timeout" => integer!(config.providers.firecrawl.timeout),
        "providers.anysearch.url" => string!(config.providers.anysearch.url),
        "providers.anysearch.keys" => array!(config.providers.anysearch.keys),
        "providers.anysearch.timeout" => integer!(config.providers.anysearch.timeout),
        "capabilities.web_search.order" => array!(config.capabilities.web_search.order),
        "capabilities.web_fetch.order" => array!(config.capabilities.web_fetch.order),
        "capabilities.docs_search.order" => array!(config.capabilities.docs_search.order),
        "capabilities.vertical_search.order" => array!(config.capabilities.vertical_search.order),
        "log.level" => string!(config.log.level),
        "journal.enabled" => boolean!(config.journal.enabled),
        "journal.dir" => string!(config.journal.dir),
        "journal.retention_days" => integer!(config.journal.retention_days),
        "retry.max_attempts" => integer!(config.retry.max_attempts),
        "retry.multiplier" => integer!(config.retry.multiplier),
        "retry.max_wait" => integer!(config.retry.max_wait),
        "http.ssl_verify" => boolean!(config.http.ssl_verify),
        _ => Err(()),
    }
}

fn parse_string_array(raw: &str) -> Result<Vec<String>, ()> {
    #[derive(Deserialize)]
    struct Wrapped {
        value: Vec<String>,
    }
    toml::from_str::<Wrapped>(&format!("value = {raw}"))
        .map(|wrapped| wrapped.value)
        .map_err(|_| ())
}

fn normalize_credentials(config: &mut Config) {
    normalize_strings(&mut config.classifier.keys);
    normalize_strings(&mut config.providers.xai.keys);
    normalize_strings(&mut config.providers.openai_compatible.keys);
    normalize_strings(&mut config.providers.exa.keys);
    normalize_strings(&mut config.providers.context7.keys);
    normalize_strings(&mut config.providers.jina.keys);
    normalize_strings(&mut config.providers.tavily.keys);
    normalize_strings(&mut config.providers.firecrawl.keys);
    normalize_strings(&mut config.providers.anysearch.keys);
}

fn normalize_strings(values: &mut Vec<String>) {
    for value in values.iter_mut() {
        *value = value.trim().to_owned();
    }
    let mut seen = HashSet::new();
    values.retain(|value| !value.is_empty() && seen.insert(value.clone()));
}

fn validate(config: &Config, path: &Path, content: &str) -> Result<(), ConfigError> {
    validate_list(
        "search.backends",
        &config.search.backends,
        &["xai", "openai_compatible"],
        false,
        path,
        content,
    )?;
    validate_enum(
        "search.validation",
        &config.search.validation,
        &["fast", "balanced", "strict"],
        path,
        content,
    )?;
    validate_enum(
        "search.fallback",
        &config.search.fallback,
        &["auto", "off"],
        path,
        content,
    )?;
    if config
        .providers
        .xai
        .tools
        .iter()
        .any(|tool| !["web_search", "x_search"].contains(&tool.as_str()))
    {
        return Err(value_error(path, content, "providers.xai.tools"));
    }
    validate_enum(
        "log.level",
        &config.log.level,
        &["error", "warn", "info", "debug", "trace"],
        path,
        content,
    )?;
    for (key, timeout) in [
        ("classifier.timeout", config.classifier.timeout),
        ("providers.exa.timeout", config.providers.exa.timeout),
        (
            "providers.context7.timeout",
            config.providers.context7.timeout,
        ),
        ("providers.jina.timeout", config.providers.jina.timeout),
        ("providers.tavily.timeout", config.providers.tavily.timeout),
        (
            "providers.firecrawl.timeout",
            config.providers.firecrawl.timeout,
        ),
        (
            "providers.anysearch.timeout",
            config.providers.anysearch.timeout,
        ),
    ] {
        if timeout <= 0 {
            return Err(value_error(path, content, key));
        }
    }
    if config.retry.max_attempts < 1 {
        return Err(value_error(path, content, "retry.max_attempts"));
    }
    if !config.retry.multiplier.is_finite() || config.retry.multiplier <= 0.0 {
        return Err(value_error(path, content, "retry.multiplier"));
    }
    if config.retry.max_wait < 0 {
        return Err(value_error(path, content, "retry.max_wait"));
    }
    if config.journal.retention_days < 0 {
        return Err(value_error(path, content, "journal.retention_days"));
    }
    validate_order(
        "web_search",
        &config.capabilities.web_search.order,
        false,
        path,
        content,
    )?;
    validate_order(
        "web_fetch",
        &config.capabilities.web_fetch.order,
        true,
        path,
        content,
    )?;
    validate_order(
        "docs_search",
        &config.capabilities.docs_search.order,
        false,
        path,
        content,
    )?;
    validate_order(
        "vertical_search",
        &config.capabilities.vertical_search.order,
        false,
        path,
        content,
    )
}

fn validate_enum(
    key: &str,
    value: &str,
    allowed: &[&str],
    path: &Path,
    content: &str,
) -> Result<(), ConfigError> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(value_error(path, content, key))
    }
}

fn validate_list(
    key: &str,
    values: &[String],
    allowed: &[&str],
    allow_empty: bool,
    path: &Path,
    content: &str,
) -> Result<(), ConfigError> {
    let mut seen = HashSet::new();
    if (!allow_empty && values.is_empty())
        || values
            .iter()
            .any(|value| !allowed.contains(&value.as_str()) || !seen.insert(value))
    {
        Err(value_error(path, content, key))
    } else {
        Ok(())
    }
}

fn validate_order(
    capability: &str,
    order: &[String],
    required: bool,
    path: &Path,
    content: &str,
) -> Result<(), ConfigError> {
    let key = format!("capabilities.{capability}.order");
    if (required && order.is_empty())
        || order
            .iter()
            .any(|provider| !providers::supports(capability, provider))
    {
        Err(value_error(path, content, &key))
    } else {
        Ok(())
    }
}

fn value_error(path: &Path, content: &str, key: &str) -> ConfigError {
    let position = source_position(content, key)
        .map(|(line, column)| format!(" at line {line}, column {column}"))
        .unwrap_or_default();
    ConfigError::Document {
        path: path.to_path_buf(),
        detail: format!("invalid value for `{key}`{position}"),
    }
}

fn source_position(content: &str, path: &str) -> Option<(usize, usize)> {
    let document = Document::parse(content).ok()?;
    let mut table: &dyn TableLike = document.as_table();
    let mut item = None;
    let mut segments = path.split('.').peekable();
    while let Some(segment) = segments.next() {
        let next = table.get(segment)?;
        item = Some(next);
        if segments.peek().is_some() {
            table = next.as_table_like()?;
        }
    }
    let offset = item?.span()?.start;
    let preceding = content.get(..offset)?;
    let line = preceding.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = preceding
        .rsplit_once('\n')
        .map_or(preceding, |(_, line)| line)
        .chars()
        .count()
        + 1;
    Some((line, column))
}

fn build_view(config: &Config, file: &toml::Value, environment: &HashSet<String>) -> JsonValue {
    macro_rules! leaf {
        ($path:literal, $value:expr) => {
            leaf_value(json!($value), source($path, file, environment))
        };
    }
    macro_rules! keys {
        ($path:literal, $value:expr) => {
            key_value($value, source($path, file, environment))
        };
    }
    macro_rules! url {
        ($path:literal, $value:expr) => {
            leaf_value(json!(redact_url($value)), source($path, file, environment))
        };
    }
    json!({
        "search": {
            "backends": leaf!("search.backends", config.search.backends),
            "validation": leaf!("search.validation", config.search.validation),
            "fallback": leaf!("search.fallback", config.search.fallback),
        },
        "classifier": {
            "url": url!("classifier.url", &config.classifier.url),
            "keys": keys!("classifier.keys", &config.classifier.keys),
            "model": leaf!("classifier.model", config.classifier.model),
            "fallback_models": leaf!("classifier.fallback_models", config.classifier.fallback_models),
            "timeout": leaf!("classifier.timeout", config.classifier.timeout),
        },
        "providers": {
            "xai": {
                "url": url!("providers.xai.url", &config.providers.xai.url),
                "keys": keys!("providers.xai.keys", &config.providers.xai.keys),
                "model": leaf!("providers.xai.model", config.providers.xai.model),
                "tools": leaf!("providers.xai.tools", config.providers.xai.tools),
            },
            "openai_compatible": {
                "url": url!("providers.openai_compatible.url", &config.providers.openai_compatible.url),
                "keys": keys!("providers.openai_compatible.keys", &config.providers.openai_compatible.keys),
                "model": leaf!("providers.openai_compatible.model", config.providers.openai_compatible.model),
                "fallback_models": leaf!("providers.openai_compatible.fallback_models", config.providers.openai_compatible.fallback_models),
                "stream": leaf!("providers.openai_compatible.stream", config.providers.openai_compatible.stream),
            },
            "exa": endpoint_view("providers.exa", &config.providers.exa, file, environment),
            "context7": endpoint_view("providers.context7", &config.providers.context7, file, environment),
            "jina": {
                "url": url!("providers.jina.url", &config.providers.jina.url),
                "keys": keys!("providers.jina.keys", &config.providers.jina.keys),
                "respond_with": leaf!("providers.jina.respond_with", config.providers.jina.respond_with),
                "timeout": leaf!("providers.jina.timeout", config.providers.jina.timeout),
            },
            "tavily": endpoint_view("providers.tavily", &config.providers.tavily, file, environment),
            "firecrawl": endpoint_view("providers.firecrawl", &config.providers.firecrawl, file, environment),
            "anysearch": endpoint_view("providers.anysearch", &config.providers.anysearch, file, environment),
        },
        "capabilities": {
            "web_search": {"order": leaf!("capabilities.web_search.order", config.capabilities.web_search.order)},
            "web_fetch": {"order": leaf!("capabilities.web_fetch.order", config.capabilities.web_fetch.order)},
            "docs_search": {"order": leaf!("capabilities.docs_search.order", config.capabilities.docs_search.order)},
            "vertical_search": {"order": leaf!("capabilities.vertical_search.order", config.capabilities.vertical_search.order)},
        },
        "log": {"level": leaf!("log.level", config.log.level)},
        "journal": {
            "enabled": leaf!("journal.enabled", config.journal.enabled),
            "dir": leaf!("journal.dir", config.journal.dir),
            "retention_days": leaf!("journal.retention_days", config.journal.retention_days),
        },
        "retry": {
            "max_attempts": leaf!("retry.max_attempts", config.retry.max_attempts),
            "multiplier": leaf!("retry.multiplier", config.retry.multiplier),
            "max_wait": leaf!("retry.max_wait", config.retry.max_wait),
        },
        "http": {"ssl_verify": leaf!("http.ssl_verify", config.http.ssl_verify)},
    })
}

fn endpoint_view<D: EndpointDefaults>(
    prefix: &str,
    endpoint: &Endpoint<D>,
    file: &toml::Value,
    environment: &HashSet<String>,
) -> JsonValue {
    let url_path = format!("{prefix}.url");
    let keys_path = format!("{prefix}.keys");
    let timeout_path = format!("{prefix}.timeout");
    json!({
        "url": leaf_value(json!(redact_url(&endpoint.url)), source(&url_path, file, environment)),
        "keys": key_value(&endpoint.keys, source(&keys_path, file, environment)),
        "timeout": leaf_value(json!(endpoint.timeout), source(&timeout_path, file, environment)),
    })
}

pub(crate) fn redact_url(value: &str) -> String {
    let without_fragment = value.split_once('#').map_or(value, |(url, _)| url);
    let mut redacted = without_fragment.to_owned();

    if let Some(authority_start) = redacted.find("://").map(|index| index + 3) {
        let authority_end = redacted[authority_start..]
            .find(['/', '?'])
            .map_or(redacted.len(), |index| authority_start + index);
        if let Some(userinfo_end) = redacted[authority_start..authority_end].rfind('@') {
            redacted.replace_range(authority_start..=authority_start + userinfo_end, "");
        }
    }

    let Some(query_start) = redacted.find('?') else {
        return redacted;
    };
    let query = redacted[query_start + 1..]
        .split('&')
        .map(|pair| {
            let Some((name, value)) = pair.split_once('=') else {
                return pair.to_owned();
            };
            if is_sensitive_query_name(name) {
                format!("{name}={CREDENTIAL_MASK}")
            } else {
                format!("{name}={value}")
            }
        })
        .collect::<Vec<_>>()
        .join("&");
    redacted.truncate(query_start + 1);
    redacted.push_str(&query);
    redacted
}

pub(crate) fn redact_urls(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut remaining = value;
    while let Some(start) = match (remaining.find("https://"), remaining.find("http://")) {
        (Some(https), Some(http)) => Some(https.min(http)),
        (Some(https), None) => Some(https),
        (None, Some(http)) => Some(http),
        (None, None) => None,
    } {
        output.push_str(&remaining[..start]);
        remaining = &remaining[start..];
        let end = remaining
            .find(|character: char| {
                character.is_whitespace()
                    || matches!(character, '<' | '>' | '"' | '\'' | ')' | ']' | '}')
            })
            .unwrap_or(remaining.len());
        output.push_str(&redact_url(&remaining[..end]));
        remaining = &remaining[end..];
    }
    output.push_str(remaining);
    output
}

pub(crate) fn redact_credentials(value: &str, credentials: &[String]) -> String {
    credentials
        .iter()
        .fold(redact_urls(value), |redacted, credential| {
            redacted.replace(credential, CREDENTIAL_MASK)
        })
}

fn is_sensitive_query_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ["token", "key", "secret", "signature", "authorization"]
        .iter()
        .any(|sensitive| name.contains(sensitive))
}

fn leaf_value(value: JsonValue, source: &'static str) -> JsonValue {
    json!({"value": value, "source": source})
}

fn key_value(keys: &[String], source: &'static str) -> JsonValue {
    json!({
        "value": vec![CREDENTIAL_MASK; keys.len()],
        "source": source,
        "configured": !keys.is_empty(),
        "key_count": keys.len(),
    })
}

fn source(path: &str, file: &toml::Value, environment: &HashSet<String>) -> &'static str {
    if environment.contains(path) {
        "env"
    } else if toml_contains(file, path) {
        "file"
    } else {
        "default"
    }
}

fn toml_contains(value: &toml::Value, path: &str) -> bool {
    path.split('.')
        .try_fold(value, |value, segment| value.get(segment))
        .is_some()
}

#[derive(Clone, Copy)]
enum ValueKind {
    String,
    Boolean,
    Integer,
    Float,
    Array,
}

fn path_kind(path: &str) -> Option<ValueKind> {
    if [
        "search.backends",
        "classifier.keys",
        "classifier.fallback_models",
        "providers.xai.keys",
        "providers.xai.tools",
        "providers.openai_compatible.keys",
        "providers.openai_compatible.fallback_models",
        "providers.exa.keys",
        "providers.context7.keys",
        "providers.jina.keys",
        "providers.tavily.keys",
        "providers.firecrawl.keys",
        "providers.anysearch.keys",
        "capabilities.web_search.order",
        "capabilities.web_fetch.order",
        "capabilities.docs_search.order",
        "capabilities.vertical_search.order",
    ]
    .contains(&path)
    {
        Some(ValueKind::Array)
    } else if [
        "providers.openai_compatible.stream",
        "journal.enabled",
        "http.ssl_verify",
    ]
    .contains(&path)
    {
        Some(ValueKind::Boolean)
    } else if path == "retry.multiplier" {
        Some(ValueKind::Float)
    } else if [
        "classifier.timeout",
        "providers.exa.timeout",
        "providers.context7.timeout",
        "providers.jina.timeout",
        "providers.tavily.timeout",
        "providers.firecrawl.timeout",
        "providers.anysearch.timeout",
        "journal.retention_days",
        "retry.max_attempts",
        "retry.max_wait",
    ]
    .contains(&path)
    {
        Some(ValueKind::Integer)
    } else if is_leaf(path) {
        Some(ValueKind::String)
    } else {
        None
    }
}

fn is_leaf(path: &str) -> bool {
    LEAVES.contains(&path)
}

fn env_path(name: &str) -> Option<&'static str> {
    LEAVES.iter().copied().find(|path| env_name(path) == name)
}

fn env_name(path: &str) -> String {
    format!("FORAGER_{}", path.replace('.', "__").to_ascii_uppercase())
}

const LEAVES: &[&str] = &[
    "search.backends",
    "search.validation",
    "search.fallback",
    "classifier.url",
    "classifier.keys",
    "classifier.model",
    "classifier.fallback_models",
    "classifier.timeout",
    "providers.xai.url",
    "providers.xai.keys",
    "providers.xai.model",
    "providers.xai.tools",
    "providers.openai_compatible.url",
    "providers.openai_compatible.keys",
    "providers.openai_compatible.model",
    "providers.openai_compatible.fallback_models",
    "providers.openai_compatible.stream",
    "providers.exa.url",
    "providers.exa.keys",
    "providers.exa.timeout",
    "providers.context7.url",
    "providers.context7.keys",
    "providers.context7.timeout",
    "providers.jina.url",
    "providers.jina.keys",
    "providers.jina.respond_with",
    "providers.jina.timeout",
    "providers.tavily.url",
    "providers.tavily.keys",
    "providers.tavily.timeout",
    "providers.firecrawl.url",
    "providers.firecrawl.keys",
    "providers.firecrawl.timeout",
    "providers.anysearch.url",
    "providers.anysearch.keys",
    "providers.anysearch.timeout",
    "capabilities.web_search.order",
    "capabilities.web_fetch.order",
    "capabilities.docs_search.order",
    "capabilities.vertical_search.order",
    "log.level",
    "journal.enabled",
    "journal.dir",
    "journal.retention_days",
    "retry.max_attempts",
    "retry.multiplier",
    "retry.max_wait",
    "http.ssl_verify",
];

/// The directory and file used by forager configuration.
#[derive(Debug, Eq, PartialEq)]
pub struct ConfigLocation {
    config_dir: PathBuf,
}

impl ConfigLocation {
    /// Resolves the configuration location from the process environment.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::DefaultDirectoryUnavailable`] when no writable
    /// absolute XDG default can be resolved.
    pub fn discover() -> Result<Self, ConfigError> {
        if let Some(config_dir) = env::var_os("FORAGER_CONFIG_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
        {
            return Ok(Self { config_dir });
        }

        let config_dir = env::var_os("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .map(|path| path.join("forager"))
            .or_else(|| {
                env::var_os("HOME")
                    .filter(|value| !value.is_empty())
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute())
                    .map(|path| path.join(".config/forager"))
            })
            .ok_or(ConfigError::DefaultDirectoryUnavailable)?;

        verify_default_directory(&config_dir)
            .map_err(|_| ConfigError::DefaultDirectoryUnavailable)?;
        Ok(Self { config_dir })
    }

    /// Returns the resolved `config.toml` path.
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }
}

/// Configuration loading and persistence errors.
#[derive(Debug, Eq, Error, PartialEq)]
pub enum ConfigError {
    /// The XDG configuration directory could not be resolved or written.
    #[error("default configuration directory is unavailable; set FORAGER_CONFIG_DIR")]
    DefaultDirectoryUnavailable,
    /// The configuration document is invalid.
    #[error("{}: {detail}", path.display())]
    Document { path: PathBuf, detail: String },
    /// A configuration operation failed.
    #[error("{0}")]
    Message(String),
}

impl ConfigError {
    fn io(path: &Path, error: io::Error) -> Self {
        Self::Message(format!("{}: {error}", path.display()))
    }
}

/// Errors from `config set` and `config unset`.
#[derive(Debug, Error)]
pub enum EditError {
    /// The requested key or value is invalid.
    #[error("{0}")]
    Argument(String),
    /// The configuration document could not be read or written.
    #[error(transparent)]
    Config(#[from] ConfigError),
}

fn atomic_write(config_dir: &Path, destination: &Path, bytes: &[u8]) -> io::Result<()> {
    ensure_private_directory(config_dir)?;
    let sequence = FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = config_dir.join(format!(
        ".config.toml.{}.{}.tmp",
        std::process::id(),
        sequence
    ));
    let result = (|| {
        let mut file = create_new_private_file(&temporary)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        fs::rename(&temporary, destination)?;
        restrict_private_file(destination)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn atomic_create(config_dir: &Path, destination: &Path, bytes: &[u8]) -> io::Result<()> {
    ensure_private_directory(config_dir)?;
    let sequence = FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = config_dir.join(format!(
        ".config.toml.{}.{}.tmp",
        std::process::id(),
        sequence
    ));
    let result = (|| {
        let mut file = create_new_private_file(&temporary)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        fs::hard_link(&temporary, destination)?;
        restrict_private_file(destination)?;
        fs::remove_file(&temporary)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn acquire_config_lock(config_dir: &Path) -> io::Result<File> {
    ensure_private_directory(config_dir)?;
    let lock = open_private_lock(&config_dir.join(".config.lock"))?;
    let deadline = Instant::now() + LOCK_WAIT;
    loop {
        match lock.try_lock_exclusive() {
            Ok(()) => return Ok(lock),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "config lock timed out",
                    ));
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(error),
        }
    }
}

fn acquire_location_lock(location: &ConfigLocation) -> Result<File, ConfigError> {
    let lock_path = location.config_dir.join(".config.lock");
    acquire_config_lock(&location.config_dir).map_err(|error| ConfigError::io(&lock_path, error))
}

fn open_private_lock(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    restrict_private_file(path)?;
    Ok(file)
}

/// Creates a configuration directory restricted to the current user.
#[cfg(unix)]
pub fn ensure_private_directory(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    set_mode(path, 0o700)
}

/// Creates a configuration directory restricted to the Windows owner.
#[cfg(windows)]
pub fn ensure_private_directory(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    restrict_windows_acl(path)
}

/// Opens or creates a private configuration file.
#[cfg(unix)]
pub fn create_private_file(path: &Path) -> io::Result<File> {
    let file = open_private_file(path)?;
    set_mode(path, 0o600)?;
    Ok(file)
}

/// Opens or creates a private configuration file on Windows.
#[cfg(windows)]
pub fn create_private_file(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    restrict_windows_acl(path)?;
    Ok(file)
}

#[cfg(unix)]
fn open_private_file(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .mode(0o600)
        .open(path)
}

#[cfg(unix)]
fn create_new_private_file(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
}

#[cfg(windows)]
fn create_new_private_file(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new().create_new(true).write(true).open(path)?;
    restrict_windows_acl(path)?;
    Ok(file)
}

#[cfg(unix)]
fn restrict_private_file(path: &Path) -> io::Result<()> {
    set_mode(path, 0o600)
}

#[cfg(windows)]
fn restrict_private_file(path: &Path) -> io::Result<()> {
    restrict_windows_acl(path)
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(unix)]
pub(crate) fn has_private_permissions(path: &Path, expected: u32) -> io::Result<bool> {
    use std::os::unix::fs::PermissionsExt;

    let actual = fs::metadata(path)?.permissions().mode() & 0o777;
    Ok(actual & !expected == 0)
}

#[cfg(windows)]
pub(crate) fn has_private_permissions(path: &Path, _expected: u32) -> io::Result<bool> {
    use winapi::um::winnt::PSID;
    use windows_acl::acl::{ACL, AceType};
    use windows_acl::helper::sid_to_string;

    let path = path.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "configuration path is not valid Unicode",
        )
    })?;
    let acl = ACL::from_file_path(path, false)
        .map_err(|code| io::Error::other(format!("cannot read Windows ACL: {code}")))?;
    let entries = acl
        .all()
        .map_err(|code| io::Error::other(format!("cannot enumerate Windows ACL: {code}")))?;
    Ok(!entries.is_empty()
        && entries.iter().all(|entry| {
            let is_owner = entry.sid.as_ref().is_some_and(|sid| {
                sid_to_string(sid.as_ptr() as PSID).is_ok_and(|sid| sid == "S-1-3-4")
            });
            let is_allow = matches!(
                entry.entry_type,
                AceType::AccessAllow
                    | AceType::AccessAllowCallback
                    | AceType::AccessAllowObject
                    | AceType::AccessAllowCallbackObject
            );
            is_owner && is_allow
        }))
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn has_private_permissions(_path: &Path, _expected: u32) -> io::Result<bool> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "private configuration permissions are unavailable",
    ))
}

#[cfg(not(any(unix, windows)))]
pub fn ensure_private_directory(_path: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "private configuration permissions are unavailable",
    ))
}

#[cfg(not(any(unix, windows)))]
pub fn create_private_file(_path: &Path) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "private configuration permissions are unavailable",
    ))
}

#[cfg(not(any(unix, windows)))]
fn create_new_private_file(_path: &Path) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "private configuration permissions are unavailable",
    ))
}

#[cfg(not(any(unix, windows)))]
fn restrict_private_file(_path: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "private configuration permissions are unavailable",
    ))
}

#[cfg(windows)]
fn restrict_windows_acl(path: &Path) -> io::Result<()> {
    use winapi::um::winnt::{FILE_ALL_ACCESS, PSID};
    use windows_acl::acl::{ACL, AceType};
    use windows_acl::helper::{sid_to_string, string_to_sid};

    const OWNER_SID: &str = "S-1-3-4";
    let path = path.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "configuration path is not valid Unicode",
        )
    })?;
    let mut acl = ACL::from_file_path(path, false)
        .map_err(|code| io::Error::other(format!("cannot read Windows ACL: {code}")))?;
    let entries = acl
        .all()
        .map_err(|code| io::Error::other(format!("cannot enumerate Windows ACL: {code}")))?;
    let owner = string_to_sid(OWNER_SID)
        .map_err(|code| io::Error::other(format!("cannot create owner SID: {code}")))?;
    for entry in entries {
        let sid = entry
            .sid
            .ok_or_else(|| io::Error::other("cannot verify a Windows ACL entry without a SID"))?;
        let sid = sid.as_ptr() as PSID;
        sid_to_string(sid)
            .map_err(|code| io::Error::other(format!("cannot read ACL SID: {code}")))?;
        acl.remove(sid, None, None).map_err(|code| {
            io::Error::other(format!("cannot remove inherited ACL entry: {code}"))
        })?;
    }
    acl.add_entry(
        owner.as_ptr() as PSID,
        AceType::AccessAllow,
        0,
        FILE_ALL_ACCESS,
    )
    .map_err(|code| io::Error::other(format!("cannot grant owner access: {code}")))
    .map(|_| ())
}

fn verify_default_directory(config_dir: &Path) -> io::Result<()> {
    let mut writable_ancestor = None;
    for ancestor in config_dir.ancestors() {
        match fs::metadata(ancestor) {
            Ok(metadata) if metadata.is_dir() => {
                writable_ancestor = Some(ancestor);
                break;
            }
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::NotADirectory,
                    "configuration path ancestor is not a directory",
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    let writable_ancestor = writable_ancestor.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "configuration path has no existing ancestor",
        )
    })?;
    let sequence = WRITE_PROBE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let probe = writable_ancestor.join(format!(
        ".forager-write-probe-{}-{sequence}",
        std::process::id()
    ));
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&probe)?;
    drop(file);
    fs::remove_file(probe)
}

#[cfg(test)]
mod tests {
    use super::{Config, redact_url};

    #[test]
    fn provider_endpoints_use_their_url_defaults_when_omitted() {
        let config: Config = toml::from_str(
            "[providers.exa]\ntimeout = 41\n\
             [providers.context7]\ntimeout = 42\n\
             [providers.tavily]\ntimeout = 43\n\
             [providers.firecrawl]\ntimeout = 44\n\
             [providers.anysearch]\ntimeout = 45\n",
        )
        .expect("deserialize partial provider endpoints");

        assert_eq!(
            (
                config.providers.exa.url.as_str(),
                config.providers.context7.url.as_str(),
                config.providers.tavily.url.as_str(),
                config.providers.firecrawl.url.as_str(),
                config.providers.anysearch.url.as_str(),
            ),
            (
                "https://api.exa.ai",
                "https://mcp.context7.com/mcp",
                "https://api.tavily.com",
                "https://api.firecrawl.dev/v2",
                "https://api.anysearch.com/mcp",
            )
        );
    }

    #[test]
    fn url_redaction_exhausts_credentials_fragments_and_safe_query_boundaries() {
        for (input, expected) in [
            (
                "https://user:password@example.test/path#private",
                "https://example.test/path",
            ),
            (
                "https://example.test/path?api_key=secret&safe=yes",
                "https://example.test/path?api_key=********&safe=yes",
            ),
            (
                "https://example.test/path?access_token=secret&signature=secret",
                "https://example.test/path?access_token=********&signature=********",
            ),
            (
                "https://example.test/path?monkey=value&author=alice",
                "https://example.test/path?monkey=********&author=alice",
            ),
            ("not a URL#fragment", "not a URL"),
        ] {
            assert_eq!(redact_url(input), expected, "input={input}");
        }
    }
}
