use std::collections::HashSet;

use serde::Serialize;
use serde_json::{Map, Value as JsonValue, json};

use crate::redact::{CREDENTIAL_MASK, Secret, redact_url};

use super::load::load_effective_config;
use super::location::ConfigError;
use super::schema::{Config, FieldRef, Leaf, SCHEMA, View};

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
    let mut root = Map::new();
    for leaf in SCHEMA {
        insert_path(
            &mut root,
            leaf.path,
            project_leaf(leaf, config, source(leaf.path, file, environment)),
        );
    }
    JsonValue::Object(root)
}

fn project_leaf(leaf: &Leaf, config: &Config, source: &'static str) -> JsonValue {
    match (leaf.view, (leaf.get)(config)) {
        (View::Keys, FieldRef::Secrets(keys)) => key_value(keys, source),
        (View::Url, FieldRef::String(value)) => leaf_value(&json!(redact_url(value)), source),
        (View::Plain, FieldRef::String(value)) => leaf_value(&json!(value), source),
        (View::Plain, FieldRef::Bool(value)) => leaf_value(&json!(value), source),
        (View::Plain, FieldRef::U64(value)) => leaf_value(&json!(value), source),
        (View::Plain, FieldRef::F64(value)) => leaf_value(&json!(value), source),
        (View::Plain, FieldRef::Strings(value)) => leaf_value(&json!(value), source),
        _ => unreachable!("schema view must match its field accessor"),
    }
}

fn insert_path(root: &mut Map<String, JsonValue>, path: &str, value: JsonValue) {
    let mut object = root;
    let mut segments = path.split('.').peekable();
    while let Some(segment) = segments.next() {
        if segments.peek().is_none() {
            object.insert(segment.to_owned(), value);
            return;
        }
        object = object
            .entry(segment)
            .or_insert_with(|| JsonValue::Object(Map::new()))
            .as_object_mut()
            .expect("schema path prefixes must be objects");
    }
}

fn leaf_value(value: &JsonValue, source: &'static str) -> JsonValue {
    json!({"value": value, "source": source})
}

fn key_value(keys: &[Secret], source: &'static str) -> JsonValue {
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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use serde_json::json;

    use super::build_view;
    use crate::config::schema::Config;
    use crate::redact::Secret;

    #[test]
    fn schema_views_project_plain_url_and_keys_at_nested_paths() {
        let mut config = Config::default();
        config.search.validation = "strict".into();
        config.classifier.url = "https://user:pass@example.test/?token=secret".into();
        config.classifier.keys = vec![Secret::from("canary")];
        let file: toml::Value = toml::from_str(
            "[classifier]\nurl = \"https://user:pass@example.test/?token=secret\"\n",
        )
        .expect("parse file layer");
        let environment = HashSet::from(["classifier.keys".to_owned()]);

        let view = build_view(&config, &file, &environment);

        assert_eq!(
            (
                &view["search"]["validation"],
                &view["classifier"]["url"],
                &view["classifier"]["keys"],
            ),
            (
                &json!({"value": "strict", "source": "default"}),
                &json!({"value": "https://example.test/?token=********", "source": "file"}),
                &json!({
                    "value": ["********"],
                    "source": "env",
                    "configured": true,
                    "key_count": 1
                }),
            )
        );
    }
}
