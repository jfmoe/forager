use std::collections::HashSet;
use std::path::Path;

use toml_edit::{Document, TableLike, Value};

use crate::providers;

use super::location::{ConfigError, EditError};
use super::schema::Config;

pub(super) fn validate_edit_value(path: &str, value: &Value) -> Result<(), EditError> {
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

pub(super) fn invalid_value(path: &str) -> EditError {
    EditError::Argument(format!("invalid value for `{path}`"))
}

pub(super) fn validate(config: &Config, path: &Path, content: &str) -> Result<(), ConfigError> {
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

pub(super) fn validate_enum(
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

pub(super) fn validate_list(
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

pub(super) fn validate_order(
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

#[cfg(test)]
mod tests {
    use super::source_position;

    #[test]
    fn source_position_reports_the_leaf_value_location() {
        let content = "[retry]\nmax_attempts = 0\n";

        assert_eq!(
            source_position(content, "retry.max_attempts"),
            Some((2, 16))
        );
    }
}
