use std::marker::PhantomData;
use std::sync::LazyLock;

use serde::{Deserialize, Deserializer, Serialize, de};

use crate::catalog;
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
    pub(super) fallback: String,
}

impl Default for Search {
    fn default() -> Self {
        Self {
            backends: provider_names(catalog::MAIN_SEARCH),
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
    #[serde(deserialize_with = "deserialize_integer")]
    pub(super) timeout: u64,
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
    #[serde(deserialize_with = "deserialize_integer")]
    pub(super) timeout: u64,
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
    #[serde(deserialize_with = "deserialize_integer")]
    pub(super) timeout: u64,
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
            web_search: Order::new(catalog::WEB_SEARCH),
            web_fetch: Order::new(catalog::WEB_FETCH),
            docs_search: Order::new(catalog::DOCS_SEARCH),
            vertical_search: Order::new(catalog::VERTICAL_SEARCH),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Order {
    pub(super) order: Vec<String>,
}

impl Order {
    fn new(catalog: catalog::CapabilityCatalog) -> Self {
        Self {
            order: provider_names(catalog),
        }
    }
}

fn provider_names(catalog: catalog::CapabilityCatalog) -> Vec<String> {
    catalog
        .providers
        .iter()
        .map(|provider| provider.name().to_owned())
        .collect()
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
    #[serde(deserialize_with = "deserialize_integer")]
    pub(super) retention_days: u64,
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
    #[serde(deserialize_with = "deserialize_integer")]
    pub(super) max_attempts: u64,
    pub(super) multiplier: f64,
    #[serde(deserialize_with = "deserialize_integer")]
    pub(super) max_wait: u64,
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

pub(super) struct Leaf {
    pub(super) path: &'static str,
    pub(super) kind: ValueKind,
    pub(super) get: fn(&Config) -> FieldRef<'_>,
    pub(super) get_mut: fn(&mut Config) -> FieldMut<'_>,
    pub(super) rule: Rule,
    pub(super) view: View,
    pub(super) comment: &'static str,
}

pub(super) enum FieldRef<'a> {
    String(&'a str),
    Bool(bool),
    U64(u64),
    F64(f64),
    Strings(&'a [String]),
    Secrets(&'a [Secret]),
}

pub(super) enum FieldMut<'a> {
    String(&'a mut String),
    Bool(&'a mut bool),
    U64(&'a mut u64),
    F64(&'a mut f64),
    Strings(&'a mut Vec<String>),
    Secrets(&'a mut Vec<Secret>),
}

#[derive(Clone, Copy)]
pub(super) enum Rule {
    Any,
    OneOf(&'static [&'static str]),
    Subset {
        allowed: &'static [&'static str],
        unique: bool,
        allow_empty: bool,
    },
    Positive,
    PositiveFinite,
    CapabilityOrder {
        capability: &'static str,
        allow_empty: bool,
    },
}

#[derive(Clone, Copy)]
pub(super) enum View {
    Plain,
    Url,
    Keys,
}

macro_rules! kind_of {
    (String) => {
        ValueKind::String
    };
    (Bool) => {
        ValueKind::Boolean
    };
    (U64) => {
        ValueKind::Integer
    };
    (F64) => {
        ValueKind::Float
    };
    (Strings) => {
        ValueKind::Array
    };
    (Secrets) => {
        ValueKind::Array
    };
}

macro_rules! field_ref {
    (String, $value:expr) => {
        FieldRef::String($value.as_str())
    };
    (Bool, $value:expr) => {
        FieldRef::Bool(*$value)
    };
    (U64, $value:expr) => {
        FieldRef::U64(*$value)
    };
    (F64, $value:expr) => {
        FieldRef::F64(*$value)
    };
    (Strings, $value:expr) => {
        FieldRef::Strings($value.as_slice())
    };
    (Secrets, $value:expr) => {
        FieldRef::Secrets($value.as_slice())
    };
}

macro_rules! leaf {
    ($path:literal, $($field:ident).+ : $variant:ident, $rule:expr, $view:expr, $comment:literal) => {{
        fn get(config: &Config) -> FieldRef<'_> {
            field_ref!($variant, &config.$($field).+)
        }
        fn get_mut(config: &mut Config) -> FieldMut<'_> {
            FieldMut::$variant(&mut config.$($field).+)
        }
        Leaf {
            path: $path,
            kind: kind_of!($variant),
            get,
            get_mut,
            rule: $rule,
            view: $view,
            comment: $comment,
        }
    }};
}

const XAI_TOOLS: &[&str] = &["web_search", "x_search"];
const FALLBACK: &[&str] = &["auto", "off"];
const LOG_LEVELS: &[&str] = &["error", "warn", "info", "debug", "trace"];

pub(super) static SCHEMA: &[Leaf] = &[
    leaf!("search.backends", search.backends: Strings, Rule::CapabilityOrder { capability: "main_search", allow_empty: false }, View::Plain, "ordered main-model backends"),
    leaf!("search.fallback", search.fallback: String, Rule::OneOf(FALLBACK), View::Plain, "fallback policy: auto or off"),
    leaf!("classifier.url", classifier.url: String, Rule::Any, View::Url, "service endpoint URL"),
    leaf!("classifier.keys", classifier.keys: Secrets, Rule::Any, View::Keys, "credential pool; keep empty until credentials are available"),
    leaf!("classifier.model", classifier.model: String, Rule::Any, View::Plain, "model identifier"),
    leaf!("classifier.fallback_models", classifier.fallback_models: Strings, Rule::Any, View::Plain, "ordered fallback model identifiers"),
    leaf!("classifier.timeout", classifier.timeout: U64, Rule::Positive, View::Plain, "shared timeout in seconds; must be greater than zero"),
    leaf!("providers.xai.url", providers.xai.url: String, Rule::Any, View::Url, "service endpoint URL"),
    leaf!("providers.xai.keys", providers.xai.keys: Secrets, Rule::Any, View::Keys, "credential pool; keep empty until credentials are available"),
    leaf!("providers.xai.model", providers.xai.model: String, Rule::Any, View::Plain, "model identifier"),
    leaf!("providers.xai.tools", providers.xai.tools: Strings, Rule::Subset { allowed: XAI_TOOLS, unique: false, allow_empty: true }, View::Plain, "xAI tools enabled for the main model"),
    leaf!("providers.openai_compatible.url", providers.openai_compatible.url: String, Rule::Any, View::Url, "service endpoint URL"),
    leaf!("providers.openai_compatible.keys", providers.openai_compatible.keys: Secrets, Rule::Any, View::Keys, "credential pool; keep empty until credentials are available"),
    leaf!("providers.openai_compatible.model", providers.openai_compatible.model: String, Rule::Any, View::Plain, "model identifier"),
    leaf!("providers.openai_compatible.fallback_models", providers.openai_compatible.fallback_models: Strings, Rule::Any, View::Plain, "ordered fallback model identifiers"),
    leaf!("providers.openai_compatible.stream", providers.openai_compatible.stream: Bool, Rule::Any, View::Plain, "enable streaming transport"),
    leaf!("providers.exa.url", providers.exa.url: String, Rule::Any, View::Url, "service endpoint URL"),
    leaf!("providers.exa.keys", providers.exa.keys: Secrets, Rule::Any, View::Keys, "credential pool; keep empty until credentials are available"),
    leaf!("providers.exa.timeout", providers.exa.timeout: U64, Rule::Positive, View::Plain, "shared timeout in seconds; must be greater than zero"),
    leaf!("providers.context7.url", providers.context7.url: String, Rule::Any, View::Url, "service endpoint URL"),
    leaf!("providers.context7.keys", providers.context7.keys: Secrets, Rule::Any, View::Keys, "credential pool; keep empty until credentials are available"),
    leaf!("providers.context7.timeout", providers.context7.timeout: U64, Rule::Positive, View::Plain, "shared timeout in seconds; must be greater than zero"),
    leaf!("providers.jina.url", providers.jina.url: String, Rule::Any, View::Url, "service endpoint URL"),
    leaf!("providers.jina.keys", providers.jina.keys: Secrets, Rule::Any, View::Keys, "credential pool; keep empty until credentials are available"),
    leaf!("providers.jina.respond_with", providers.jina.respond_with: String, Rule::Any, View::Plain, "optional X-Respond-With header value"),
    leaf!("providers.jina.timeout", providers.jina.timeout: U64, Rule::Positive, View::Plain, "shared timeout in seconds; must be greater than zero"),
    leaf!("providers.tavily.url", providers.tavily.url: String, Rule::Any, View::Url, "service endpoint URL"),
    leaf!("providers.tavily.keys", providers.tavily.keys: Secrets, Rule::Any, View::Keys, "credential pool; keep empty until credentials are available"),
    leaf!("providers.tavily.timeout", providers.tavily.timeout: U64, Rule::Positive, View::Plain, "shared timeout in seconds; must be greater than zero"),
    leaf!("providers.firecrawl.url", providers.firecrawl.url: String, Rule::Any, View::Url, "service endpoint URL"),
    leaf!("providers.firecrawl.keys", providers.firecrawl.keys: Secrets, Rule::Any, View::Keys, "credential pool; keep empty until credentials are available"),
    leaf!("providers.firecrawl.timeout", providers.firecrawl.timeout: U64, Rule::Positive, View::Plain, "shared timeout in seconds; must be greater than zero"),
    leaf!("providers.anysearch.url", providers.anysearch.url: String, Rule::Any, View::Url, "service endpoint URL"),
    leaf!("providers.anysearch.keys", providers.anysearch.keys: Secrets, Rule::Any, View::Keys, "credential pool; keep empty until credentials are available"),
    leaf!("providers.anysearch.timeout", providers.anysearch.timeout: U64, Rule::Positive, View::Plain, "shared timeout in seconds; must be greater than zero"),
    leaf!("capabilities.web_search.order", capabilities.web_search.order: Strings, Rule::CapabilityOrder { capability: "web_search", allow_empty: true }, View::Plain, "authoritative provider order for this capability"),
    leaf!("capabilities.web_fetch.order", capabilities.web_fetch.order: Strings, Rule::CapabilityOrder { capability: "web_fetch", allow_empty: false }, View::Plain, "authoritative provider order for this capability"),
    leaf!("capabilities.docs_search.order", capabilities.docs_search.order: Strings, Rule::CapabilityOrder { capability: "docs_search", allow_empty: true }, View::Plain, "authoritative provider order for this capability"),
    leaf!("capabilities.vertical_search.order", capabilities.vertical_search.order: Strings, Rule::CapabilityOrder { capability: "vertical_search", allow_empty: true }, View::Plain, "authoritative provider order for this capability"),
    leaf!("log.level", log.level: String, Rule::OneOf(LOG_LEVELS), View::Plain, "stderr log level"),
    leaf!("journal.enabled", journal.enabled: Bool, Rule::Any, View::Plain, "record search result journals"),
    leaf!("journal.dir", journal.dir: String, Rule::Any, View::Plain, "journal storage directory"),
    leaf!("journal.retention_days", journal.retention_days: U64, Rule::Any, View::Plain, "journal retention in days; zero keeps entries indefinitely"),
    leaf!("retry.max_attempts", retry.max_attempts: U64, Rule::Positive, View::Plain, "maximum attempts per request"),
    leaf!("retry.multiplier", retry.multiplier: F64, Rule::PositiveFinite, View::Plain, "retry backoff multiplier"),
    leaf!("retry.max_wait", retry.max_wait: U64, Rule::Any, View::Plain, "maximum retry wait in seconds"),
    leaf!("http.ssl_verify", http.ssl_verify: Bool, Rule::Any, View::Plain, "verify TLS certificates"),
];

pub(super) static LEAVES: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| SCHEMA.iter().map(|leaf| leaf.path).collect());

pub(super) fn leaf(path: &str) -> Option<&'static Leaf> {
    SCHEMA.iter().find(|leaf| leaf.path == path)
}

pub(super) fn path_kind(path: &str) -> Option<ValueKind> {
    leaf(path).map(|leaf| leaf.kind)
}

pub(super) fn is_leaf(path: &str) -> bool {
    leaf(path).is_some()
}

pub(super) fn env_path(name: &str) -> Option<&'static str> {
    SCHEMA
        .iter()
        .map(|leaf| leaf.path)
        .find(|path| env_name(path) == name)
}

pub(super) fn env_name(path: &str) -> String {
    format!("FORAGER_{}", path.replace('.', "__").to_ascii_uppercase())
}

pub(super) fn parse_integer(raw: &str) -> Result<u64, ()> {
    raw.parse::<u64>()
        .ok()
        .filter(|value| *value <= i64::MAX.cast_unsigned())
        .ok_or(())
}

fn deserialize_integer<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = i64::deserialize(deserializer)?;
    parse_integer(&value.to_string())
        .map_err(|()| de::Error::custom(format!("integer must be between 0 and {}", i64::MAX)))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::catalog;
    use crate::redact::Secret;

    use super::{Config, FieldMut, FieldRef, Leaf, SCHEMA, ValueKind};

    #[test]
    fn default_provider_orders_are_catalog_projections() {
        let config = Config::default();
        let names = |catalog: catalog::CapabilityCatalog| {
            catalog
                .providers
                .iter()
                .map(|provider| provider.name().to_owned())
                .collect::<Vec<_>>()
        };

        assert_eq!(config.search.backends, names(catalog::MAIN_SEARCH));
        assert_eq!(
            config.capabilities.web_search.order,
            names(catalog::WEB_SEARCH)
        );
        assert_eq!(
            config.capabilities.web_fetch.order,
            names(catalog::WEB_FETCH)
        );
        assert_eq!(
            config.capabilities.docs_search.order,
            names(catalog::DOCS_SEARCH)
        );
        assert_eq!(
            config.capabilities.vertical_search.order,
            names(catalog::VERTICAL_SEARCH)
        );
    }

    fn toml_leaf_paths(value: &toml::Value, prefix: &str, paths: &mut BTreeSet<String>) {
        if let toml::Value::Table(table) = value {
            for (key, value) in table {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                toml_leaf_paths(value, &path, paths);
            }
        } else {
            paths.insert(prefix.to_owned());
        }
    }

    #[test]
    fn schema_paths_exactly_cover_the_default_configuration_leaves() {
        let value = toml::Value::try_from(Config::default()).expect("serialize default config");
        let mut serialized = BTreeSet::new();
        toml_leaf_paths(&value, "", &mut serialized);
        let declared = SCHEMA
            .iter()
            .map(|leaf| leaf.path.to_owned())
            .collect::<BTreeSet<_>>();

        assert_eq!(declared, serialized);
        assert_eq!(declared.len(), SCHEMA.len(), "duplicate schema path");
    }

    fn sentinel(index: usize, kind: ValueKind) -> toml::Value {
        let index = u32::try_from(index).expect("schema index fits u32");
        match kind {
            ValueKind::String => toml::Value::String(format!("string-sentinel-{index}")),
            ValueKind::Boolean => toml::Value::Boolean(true),
            ValueKind::Integer => toml::Value::Integer(10_000 + i64::from(index)),
            ValueKind::Float => toml::Value::Float(10_000.25 + f64::from(index)),
            ValueKind::Array => {
                toml::Value::Array(vec![toml::Value::String(format!("array-sentinel-{index}"))])
            }
        }
    }

    fn neutral(kind: ValueKind) -> toml::Value {
        match kind {
            ValueKind::String => toml::Value::String("not-the-sentinel".into()),
            ValueKind::Boolean => toml::Value::Boolean(false),
            ValueKind::Integer => toml::Value::Integer(1),
            ValueKind::Float => toml::Value::Float(1.0),
            ValueKind::Array => {
                toml::Value::Array(vec![toml::Value::String("not-the-sentinel".into())])
            }
        }
    }

    fn set_path(root: &mut toml::Value, path: &str, value: toml::Value) {
        let mut segments = path.split('.').peekable();
        let mut current = root;
        while let Some(segment) = segments.next() {
            if segments.peek().is_none() {
                current
                    .as_table_mut()
                    .expect("schema parent is a table")
                    .insert(segment.to_owned(), value);
                return;
            }
            current = current
                .as_table_mut()
                .expect("schema parent is a table")
                .get_mut(segment)
                .expect("schema path segment exists");
        }
    }

    fn value_at_path<'a>(root: &'a toml::Value, path: &str) -> &'a toml::Value {
        path.split('.').fold(root, |value, segment| {
            value.get(segment).expect("schema path segment exists")
        })
    }

    fn field_ref_value(field: &FieldRef<'_>) -> toml::Value {
        match field {
            FieldRef::String(value) => toml::Value::String((*value).to_owned()),
            FieldRef::Bool(value) => toml::Value::Boolean(*value),
            FieldRef::U64(value) => {
                toml::Value::Integer(i64::try_from(*value).expect("test integer fits i64"))
            }
            FieldRef::F64(value) => toml::Value::Float(*value),
            FieldRef::Strings(values) => {
                toml::Value::Array(values.iter().cloned().map(toml::Value::String).collect())
            }
            FieldRef::Secrets(values) => toml::Value::Array(
                values
                    .iter()
                    .map(|value| toml::Value::String(value.expose().to_owned()))
                    .collect(),
            ),
        }
    }

    fn same_field_variant(left: &FieldRef<'_>, right: &FieldRef<'_>) -> bool {
        matches!(
            (left, right),
            (FieldRef::String(_), FieldRef::String(_))
                | (FieldRef::Bool(_), FieldRef::Bool(_))
                | (FieldRef::U64(_), FieldRef::U64(_))
                | (FieldRef::F64(_), FieldRef::F64(_))
                | (FieldRef::Strings(_), FieldRef::Strings(_))
                | (FieldRef::Secrets(_), FieldRef::Secrets(_))
        )
    }

    fn write_sentinel(field: FieldMut<'_>, index: usize) {
        let index = u32::try_from(index).expect("schema index fits u32");
        match field {
            FieldMut::String(value) => *value = format!("string-sentinel-{index}"),
            FieldMut::Bool(value) => *value = true,
            FieldMut::U64(value) => *value = 10_000 + u64::from(index),
            FieldMut::F64(value) => *value = 10_000.25 + f64::from(index),
            FieldMut::Strings(value) => *value = vec![format!("array-sentinel-{index}")],
            FieldMut::Secrets(value) => {
                *value = vec![Secret::from(format!("array-sentinel-{index}").as_str())];
            }
        }
    }

    fn secret_at_path<'a>(config: &'a Config, path: &str) -> Option<&'a [Secret]> {
        match path {
            "classifier.keys" => Some(&config.classifier.keys),
            "providers.xai.keys" => Some(&config.providers.xai.keys),
            "providers.openai_compatible.keys" => Some(&config.providers.openai_compatible.keys),
            "providers.exa.keys" => Some(&config.providers.exa.keys),
            "providers.context7.keys" => Some(&config.providers.context7.keys),
            "providers.jina.keys" => Some(&config.providers.jina.keys),
            "providers.tavily.keys" => Some(&config.providers.tavily.keys),
            "providers.firecrawl.keys" => Some(&config.providers.firecrawl.keys),
            "providers.anysearch.keys" => Some(&config.providers.anysearch.keys),
            _ => None,
        }
    }

    fn mutated_value(config: &Config, leaf: &Leaf) -> toml::Value {
        if let Some(values) = secret_at_path(config, leaf.path) {
            return toml::Value::Array(
                values
                    .iter()
                    .map(|value| toml::Value::String(value.expose().to_owned()))
                    .collect(),
            );
        }
        let serialized = toml::Value::try_from(config).expect("serialize mutated config");
        value_at_path(&serialized, leaf.path).clone()
    }

    #[test]
    fn every_schema_getter_reads_its_declared_path() {
        for (target_index, target) in SCHEMA.iter().enumerate() {
            let mut serialized =
                toml::Value::try_from(Config::default()).expect("serialize default config");
            let defaults = Config::default();
            for leaf in SCHEMA {
                if same_field_variant(&(leaf.get)(&defaults), &(target.get)(&defaults)) {
                    set_path(&mut serialized, leaf.path, neutral(leaf.kind));
                }
            }
            let expected = sentinel(target_index, target.kind);
            set_path(&mut serialized, target.path, expected.clone());
            let config: Config = serialized.try_into().expect("deserialize sentinel config");

            assert_eq!(
                field_ref_value(&(target.get)(&config)),
                expected,
                "path={}",
                target.path
            );
        }
    }

    #[test]
    fn every_schema_mutator_writes_its_declared_path() {
        for (target_index, target) in SCHEMA.iter().enumerate() {
            let mut serialized =
                toml::Value::try_from(Config::default()).expect("serialize default config");
            let defaults = Config::default();
            for leaf in SCHEMA {
                if same_field_variant(&(leaf.get)(&defaults), &(target.get)(&defaults)) {
                    set_path(&mut serialized, leaf.path, neutral(leaf.kind));
                }
            }
            let mut config: Config = serialized.try_into().expect("deserialize neutral config");
            write_sentinel((target.get_mut)(&mut config), target_index);

            assert_eq!(
                mutated_value(&config, target),
                sentinel(target_index, target.kind),
                "path={}",
                target.path
            );
        }
    }

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
