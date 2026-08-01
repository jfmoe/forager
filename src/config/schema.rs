use std::marker::PhantomData;

use serde::{Deserialize, Serialize};

use crate::redact::Secret;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Config {
    pub(super) search: Search,
    pub(super) classifier: Classifier,
    pub(super) providers: Providers,
    pub(super) capabilities: Capabilities,
    pub(super) log: Log,
    pub(super) journal: Journal,
    pub(super) retry: Retry,
    pub(super) http: Http,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Search {
    pub(super) backends: Vec<String>,
    pub(super) validation: String,
    pub(super) fallback: String,
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
pub(super) struct Classifier {
    pub(super) url: String,
    pub(super) keys: Vec<Secret>,
    pub(super) model: String,
    pub(super) fallback_models: Vec<String>,
    pub(super) timeout: i64,
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
pub(super) struct Providers {
    pub(super) xai: Xai,
    pub(super) openai_compatible: OpenAiCompatible,
    pub(super) exa: Endpoint<ExaEndpoint>,
    pub(super) context7: Endpoint<Context7Endpoint>,
    pub(super) jina: Jina,
    pub(super) tavily: Endpoint<TavilyEndpoint>,
    pub(super) firecrawl: Endpoint<FirecrawlEndpoint>,
    pub(super) anysearch: Endpoint<AnysearchEndpoint>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Xai {
    pub(super) url: String,
    pub(super) keys: Vec<Secret>,
    pub(super) model: String,
    pub(super) tools: Vec<String>,
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
pub(super) struct OpenAiCompatible {
    pub(super) url: String,
    pub(super) keys: Vec<Secret>,
    pub(super) model: String,
    pub(super) fallback_models: Vec<String>,
    pub(super) stream: bool,
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
pub(super) struct Endpoint<D: EndpointDefaults> {
    pub(super) url: String,
    pub(super) keys: Vec<Secret>,
    pub(super) timeout: i64,
    #[serde(skip)]
    pub(super) defaults: PhantomData<D>,
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

pub(super) trait EndpointDefaults {
    const URL: &'static str;
}

macro_rules! endpoint_defaults {
    ($name:ident, $url:literal) => {
        #[derive(Clone, Debug)]
        pub(super) struct $name;

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
pub(super) struct Jina {
    pub(super) url: String,
    pub(super) keys: Vec<Secret>,
    pub(super) respond_with: String,
    pub(super) timeout: i64,
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
pub(super) struct Capabilities {
    pub(super) web_search: Order,
    pub(super) web_fetch: Order,
    pub(super) docs_search: Order,
    pub(super) vertical_search: Order,
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
pub(super) struct Order {
    pub(super) order: Vec<String>,
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
pub(super) struct Log {
    pub(super) level: String,
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
pub(super) struct Journal {
    pub(super) enabled: bool,
    pub(super) dir: String,
    pub(super) retention_days: i64,
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
pub(super) struct Retry {
    pub(super) max_attempts: i64,
    pub(super) multiplier: f64,
    pub(super) max_wait: i64,
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
pub(super) struct Http {
    pub(super) ssl_verify: bool,
}

impl Default for Http {
    fn default() -> Self {
        Self { ssl_verify: true }
    }
}

#[derive(Clone, Copy)]
pub(super) enum ValueKind {
    String,
    Boolean,
    Integer,
    Float,
    Array,
}

pub(super) fn path_kind(path: &str) -> Option<ValueKind> {
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

pub(super) fn is_leaf(path: &str) -> bool {
    LEAVES.contains(&path)
}

pub(super) fn env_path(name: &str) -> Option<&'static str> {
    LEAVES.iter().copied().find(|path| env_name(path) == name)
}

pub(super) fn env_name(path: &str) -> String {
    format!("FORAGER_{}", path.replace('.', "__").to_ascii_uppercase())
}

pub(super) const LEAVES: &[&str] = &[
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

#[cfg(test)]
mod tests {
    use super::Config;

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
}
