//! The runtime projection of each Capability Seam: its configured provider order as
//! `SeamEntry` values, and the runtime configuration of the providers the seams run.

use super::location::ConfigError;
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
    pub(super) fn new(id: ProviderId, config: C, configured: bool) -> Self {
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

/// An OpenAI Responses endpoint that runs main search with the `web_search` tool.
#[derive(Clone, Debug)]
pub(crate) struct OpenaiRuntimeConfig {
    pub(crate) url: String,
    pub(crate) keys: Vec<Secret>,
    pub(crate) model: String,
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
pub(crate) struct MainSearchRuntimeConfig {
    entries: Vec<SeamEntry<MainSearchProviderConfig>>,
    pub(crate) fallback: FallbackPolicy,
}

#[derive(Clone, Debug)]
pub(crate) enum MainSearchProviderConfig {
    Xai(XaiRuntimeConfig),
    Openai(OpenaiRuntimeConfig),
}

pub(super) fn provider_configured(id: ProviderId, keys: &[Secret]) -> bool {
    catalog::registration(id).is_configured(keys.len())
}

impl MainSearchProviderConfig {
    pub(crate) fn configured(&self) -> bool {
        match self {
            Self::Xai(config) => provider_configured(ProviderId::Xai, &config.keys),
            Self::Openai(config) => provider_configured(ProviderId::Openai, &config.keys),
        }
    }

    pub(crate) fn url(&self) -> &str {
        match self {
            Self::Xai(config) => &config.url,
            Self::Openai(config) => &config.url,
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
pub(crate) struct Context7RuntimeConfig {
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
            Self::Exa(config) => provider_configured(ProviderId::Exa, &config.keys),
            Self::Context7(config) => provider_configured(ProviderId::Context7, &config.keys),
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

pub(super) fn unknown_provider(name: &str, seam: &str) -> ConfigError {
    ConfigError::Message(format!("{seam} contains unknown provider `{name}`"))
}

pub(super) fn main_search_runtime(
    order: Vec<String>,
    fallback: FallbackPolicy,
    providers: &MainSearchProviders<'_>,
) -> Result<MainSearchRuntimeConfig, ConfigError> {
    let entries = order
        .into_iter()
        .map(|name| {
            let id = ProviderId::parse(&name)
                .ok_or_else(|| unknown_provider(&name, "search.backends"))?;
            let config = main_provider_config(id, providers)
                .ok_or_else(|| unknown_provider(&name, "search.backends"))?;
            let configured = config.configured();
            Ok(SeamEntry::new(id, config, configured))
        })
        .collect::<Result<_, ConfigError>>()?;
    Ok(MainSearchRuntimeConfig { entries, fallback })
}

pub(super) fn docs_search_runtime(
    order: Vec<String>,
    exa: &ExaRuntimeConfig,
    context7: &Context7RuntimeConfig,
) -> Result<DocsSearchRuntimeConfig, ConfigError> {
    let entries = order
        .into_iter()
        .map(|name| {
            let seam = "capabilities.docs_search.order";
            let id = ProviderId::parse(&name).ok_or_else(|| unknown_provider(&name, seam))?;
            let config = docs_provider_config(id, exa, context7)
                .ok_or_else(|| unknown_provider(&name, seam))?;
            let configured = config.configured();
            Ok(SeamEntry::new(id, config, configured))
        })
        .collect::<Result<_, ConfigError>>()?;
    Ok(DocsSearchRuntimeConfig { entries })
}

/// The providers of `web_search` and `web_fetch`, in the order of their keys.
pub(super) struct WebProviders<'a> {
    pub(super) tavily: &'a WebFetchProviderConfig,
    pub(super) firecrawl: &'a WebFetchProviderConfig,
    pub(super) jina: &'a WebFetchProviderConfig,
}

pub(super) fn web_search_runtime(
    order: Vec<String>,
    providers: &WebProviders<'_>,
) -> Result<WebSearchRuntimeConfig, ConfigError> {
    let entries = web_entries(order, "web_search", providers)?;
    Ok(WebSearchRuntimeConfig { entries })
}

pub(super) fn web_fetch_runtime(
    order: Vec<String>,
    providers: &WebProviders<'_>,
) -> Result<WebFetchRuntimeConfig, ConfigError> {
    let entries = web_entries(order, "web_fetch", providers)?;
    Ok(WebFetchRuntimeConfig { entries })
}

fn web_entries(
    order: Vec<String>,
    seam: &str,
    providers: &WebProviders<'_>,
) -> Result<Vec<SeamEntry<WebFetchProviderConfig>>, ConfigError> {
    let catalog = catalog::by_seam(seam).ok_or_else(|| unknown_provider("", seam))?;
    order
        .into_iter()
        .map(|name| {
            let id = ProviderId::parse(&name).ok_or_else(|| unknown_provider(&name, seam))?;
            let config = web_provider_config(
                catalog,
                id,
                providers.tavily,
                providers.firecrawl,
                providers.jina,
            )
            .ok_or_else(|| unknown_provider(&name, seam))?;
            let configured = provider_configured(id, &config.keys);
            Ok(SeamEntry::new(id, config, configured))
        })
        .collect()
}

/// The providers of `main_search`, so a new backend adds a field here and changes no signature.
pub(crate) struct MainSearchProviders<'a> {
    pub(crate) xai: &'a XaiRuntimeConfig,
    pub(crate) openai: &'a OpenaiRuntimeConfig,
}

pub(crate) fn main_provider_config(
    id: ProviderId,
    providers: &MainSearchProviders<'_>,
) -> Option<MainSearchProviderConfig> {
    match id {
        ProviderId::Xai => Some(MainSearchProviderConfig::Xai(providers.xai.clone())),
        ProviderId::Openai => Some(MainSearchProviderConfig::Openai(providers.openai.clone())),
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
