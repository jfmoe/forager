use std::collections::HashSet;

use serde::Serialize;
use serde_json::{Value as JsonValue, json};

use crate::redact::{CREDENTIAL_MASK, redact_url};

use super::load::load_effective_config;
use super::location::ConfigError;
use super::schema::{Config, Endpoint, EndpointDefaults};

/// A serialized, credential-safe view of the effective configuration.
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct EffectiveConfigView(JsonValue);

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

/// Serializes the shared effective configuration view.
///
/// # Errors
///
/// Returns a configuration or JSON serialization error.
pub fn effective_view_json() -> Result<String, ConfigError> {
    serde_json::to_string(&effective_view()?)
        .map_err(|error| ConfigError::Message(error.to_string()))
}

fn build_view(config: &Config, file: &toml::Value, environment: &HashSet<String>) -> JsonValue {
    macro_rules! leaf {
        ($path:literal, $value:expr) => {
            leaf_value(&json!($value), source($path, file, environment))
        };
    }
    macro_rules! keys {
        ($path:literal, $value:expr) => {
            key_value($value, source($path, file, environment))
        };
    }
    macro_rules! url {
        ($path:literal, $value:expr) => {
            leaf_value(&json!(redact_url($value)), source($path, file, environment))
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
        "url": leaf_value(&json!(redact_url(&endpoint.url)), source(&url_path, file, environment)),
        "keys": key_value(&endpoint.keys, source(&keys_path, file, environment)),
        "timeout": leaf_value(&json!(endpoint.timeout), source(&timeout_path, file, environment)),
    })
}

fn leaf_value(value: &JsonValue, source: &'static str) -> JsonValue {
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
