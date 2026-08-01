use std::collections::HashSet;
use std::env;
use std::fs;
use std::io;

use serde::Deserialize;

use super::edit::{config_leaf, parse_edit_value};
use super::location::{ConfigError, ConfigLocation};
use super::schema::{Config, env_path};
use super::validate::validate;

pub(super) struct LoadedConfig {
    pub(super) config: Config,
    pub(super) file_value: toml::Value,
    pub(super) env_paths: HashSet<String>,
}

pub(super) fn load_effective_config() -> Result<LoadedConfig, ConfigError> {
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
            let detail = if config_leaf(&key_path) == "keys" {
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

pub(super) fn diagnostic_without_source(detail: &str) -> String {
    detail
        .lines()
        .filter(|line| !line.contains(" |") && line.trim() != "|")
        .collect::<Vec<_>>()
        .join("\n")
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
        apply_env_value(config, path, &value).map_err(|()| {
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

#[cfg(test)]
mod tests {
    use super::normalize_strings;

    #[test]
    fn string_normalization_trims_drops_empty_and_preserves_first_occurrence() {
        let mut values = vec![
            " alpha ".to_owned(),
            String::new(),
            "beta".to_owned(),
            "alpha".to_owned(),
            "  ".to_owned(),
        ];

        normalize_strings(&mut values);

        assert_eq!(values, ["alpha", "beta"]);
    }
}
