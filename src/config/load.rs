use std::collections::HashSet;
use std::env;
use std::fs;
use std::io;

use serde::Deserialize;

use super::edit::{config_leaf, parse_edit_value};
use super::location::{ConfigError, ConfigLocation};
use super::schema::{Config, FieldMut, SCHEMA, env_path, leaf, parse_integer};
use super::validate::validate;
use crate::redact::Secret;

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
                "invalid value for configuration key `{path}` from environment variable `{name}`"
            ))
        })?;
        apply_env_value(config, path, &value).map_err(|()| {
            ConfigError::Message(format!(
                "invalid value for configuration key `{path}` from environment variable `{name}`"
            ))
        })?;
        env_paths.insert(path.to_owned());
    }
    Ok(())
}

fn apply_env_value(config: &mut Config, path: &str, raw: &str) -> Result<(), ()> {
    let leaf = leaf(path).ok_or(())?;
    match (leaf.get_mut)(config) {
        FieldMut::String(slot) => raw.clone_into(slot),
        FieldMut::Bool(slot) => *slot = raw.parse().map_err(|_| ())?,
        FieldMut::U64(slot) => *slot = parse_integer(raw)?,
        FieldMut::F64(slot) => *slot = raw.parse().map_err(|_| ())?,
        FieldMut::Strings(slot) => *slot = parse_string_array(raw)?,
        FieldMut::Secrets(slot) => *slot = parse_secret_array(raw)?,
    }
    Ok(())
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

fn parse_secret_array(raw: &str) -> Result<Vec<Secret>, ()> {
    #[derive(Deserialize)]
    struct Wrapped {
        value: Vec<Secret>,
    }
    toml::from_str::<Wrapped>(&format!("value = {raw}"))
        .map(|wrapped| wrapped.value)
        .map_err(|_| ())
}

fn normalize_credentials(config: &mut Config) {
    for leaf in SCHEMA {
        if let FieldMut::Secrets(keys) = (leaf.get_mut)(config) {
            Secret::normalize(keys);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::apply_env_value;
    use crate::config::schema::Config;

    #[test]
    fn schema_mutator_executor_applies_every_field_kind_to_its_value() {
        let mut config = Config::default();

        apply_env_value(&mut config, "search.fallback", "off").expect("string");
        apply_env_value(&mut config, "journal.enabled", "false").expect("boolean");
        apply_env_value(&mut config, "retry.max_wait", "92").expect("integer");
        apply_env_value(&mut config, "retry.multiplier", "2.5").expect("float");
        apply_env_value(&mut config, "search.backends", r#"["xai"]"#).expect("strings");
        apply_env_value(&mut config, "classifier.keys", r#"["first", "second"]"#).expect("secrets");

        let expected_backends = vec!["xai".to_owned()];
        assert_eq!(
            (
                config.search.fallback.as_str(),
                config.journal.enabled,
                config.retry.max_wait,
                config.retry.multiplier,
                config.search.backends.as_slice(),
                config.classifier.keys.len(),
            ),
            ("off", false, 92, 2.5, expected_backends.as_slice(), 2)
        );
    }
}
