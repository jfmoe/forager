use std::collections::HashSet;
use std::path::Path;

use toml_edit::{Document, TableLike, Value};

use crate::providers;

use super::location::{ConfigError, EditError};
use super::schema::{Config, FieldRef, Rule, SCHEMA, leaf};

pub(super) fn validate_edit_value(path: &str, value: &Value) -> Result<(), EditError> {
    let valid = leaf(path).is_some_and(|leaf| edit_value_satisfies(leaf.rule, value));
    if valid {
        Ok(())
    } else {
        Err(invalid_value(path))
    }
}

fn edit_value_satisfies(rule: Rule, value: &Value) -> bool {
    match rule {
        Rule::Any => true,
        Rule::OneOf(allowed) => value.as_str().is_some_and(|value| allowed.contains(&value)),
        Rule::Subset {
            allowed,
            unique,
            allow_empty,
        } => value.as_array().is_some_and(|values| {
            strings_satisfy(
                values.iter().map(Value::as_str),
                allowed,
                unique,
                allow_empty,
            )
        }),
        Rule::Positive => value.as_integer().is_some_and(|value| value > 0),
        Rule::PositiveFinite => value
            .as_float()
            .is_some_and(|value| value.is_finite() && value > 0.0),
        Rule::CapabilityOrder {
            capability,
            allow_empty,
        } => value.as_array().is_some_and(|values| {
            (allow_empty || !values.is_empty())
                && values.iter().all(|provider| {
                    provider
                        .as_str()
                        .is_some_and(|provider| providers::supports(capability, provider))
                })
        }),
    }
}

pub(super) fn invalid_value(path: &str) -> EditError {
    EditError::Argument(format!("invalid value for `{path}`"))
}

pub(super) fn validate(config: &Config, path: &Path, content: &str) -> Result<(), ConfigError> {
    for leaf in SCHEMA {
        if !field_satisfies(leaf.rule, (leaf.get)(config)) {
            return Err(value_error(path, content, leaf.path));
        }
    }
    Ok(())
}

fn field_satisfies(rule: Rule, field: FieldRef<'_>) -> bool {
    match (rule, field) {
        (Rule::Any, _) => true,
        (Rule::OneOf(allowed), FieldRef::String(value)) => allowed.contains(&value),
        (
            Rule::Subset {
                allowed,
                unique,
                allow_empty,
            },
            FieldRef::Strings(values),
        ) => strings_satisfy(
            values.iter().map(|value| Some(value.as_str())),
            allowed,
            unique,
            allow_empty,
        ),
        (Rule::Positive, FieldRef::U64(value)) => value > 0,
        (Rule::PositiveFinite, FieldRef::F64(value)) => value.is_finite() && value > 0.0,
        (
            Rule::CapabilityOrder {
                capability,
                allow_empty,
            },
            FieldRef::Strings(values),
        ) => {
            (allow_empty || !values.is_empty())
                && values
                    .iter()
                    .all(|provider| providers::supports(capability, provider))
        }
        _ => false,
    }
}

fn strings_satisfy<'a>(
    values: impl Iterator<Item = Option<&'a str>>,
    allowed: &[&str],
    unique: bool,
    allow_empty: bool,
) -> bool {
    let mut seen = HashSet::new();
    let mut count = 0;
    let valid = values.into_iter().all(|value| {
        count += 1;
        value.is_some_and(|value| {
            allowed.contains(&value) && (!unique || seen.insert(value.to_owned()))
        })
    });
    valid && (allow_empty || count > 0)
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
    use std::path::Path;

    use toml_edit::{Array, Value};

    use super::{source_position, validate, validate_edit_value};
    use crate::config::schema::Config;

    fn validates(config: &Config) -> bool {
        validate(config, Path::new("config.toml"), "").is_ok()
    }

    #[test]
    fn any_rule_accepts_zero_in_full_and_edit_validation() {
        let mut config = Config::default();
        config.retry.max_wait = 0;

        assert!(validates(&config));
        assert!(validate_edit_value("retry.max_wait", &Value::from(0)).is_ok());
    }

    #[test]
    fn one_of_rule_rejects_unknown_values_in_full_and_edit_validation() {
        let mut config = Config::default();
        config.search.fallback = "unknown".into();

        assert!(!validates(&config));
        assert!(validate_edit_value("search.fallback", &Value::from("unknown")).is_err());
    }

    #[test]
    fn subset_rule_enforces_membership_and_uniqueness_in_both_validators() {
        let mut config = Config::default();
        config.search.backends = vec!["xai".into(), "xai".into()];
        let mut values = Array::new();
        values.push("xai");
        values.push("xai");

        assert!(!validates(&config));
        assert!(validate_edit_value("search.backends", &Value::Array(values)).is_err());
    }

    #[test]
    fn positive_rule_rejects_zero_in_full_and_edit_validation() {
        let mut config = Config::default();
        config.providers.exa.timeout = 0;

        assert!(!validates(&config));
        assert!(validate_edit_value("providers.exa.timeout", &Value::from(0)).is_err());
    }

    #[test]
    fn positive_finite_rule_rejects_nan_in_full_and_edit_validation() {
        let mut config = Config::default();
        config.retry.multiplier = f64::NAN;

        assert!(!validates(&config));
        assert!(validate_edit_value("retry.multiplier", &Value::from(f64::NAN)).is_err());
    }

    #[test]
    fn capability_order_rule_rejects_unknown_providers_in_both_validators() {
        let mut config = Config::default();
        config.capabilities.web_fetch.order = vec!["unknown".into()];
        let mut values = Array::new();
        values.push("unknown");

        assert!(!validates(&config));
        assert!(
            validate_edit_value("capabilities.web_fetch.order", &Value::Array(values),).is_err()
        );
    }

    #[test]
    fn source_position_reports_the_leaf_value_location() {
        let content = "[retry]\nmax_attempts = 0\n";

        assert_eq!(
            source_position(content, "retry.max_attempts"),
            Some((2, 16))
        );
    }
}
