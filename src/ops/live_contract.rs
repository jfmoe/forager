//! Whether a Live Check's output shows what its contract requires, and what a failure observed.
//! An observation holds only types, counts and lengths, so a failure never carries a title, a
//! text or any other value of the output.

use serde::Serialize;
use serde_json::Value;

use crate::catalog::{Field, LiveContract, SampleRule, Shape};
pub(super) use crate::types::LiveCheckRule as Rule;

/// Which rule a check broke, where, and what was there.
#[derive(Debug, PartialEq, Serialize)]
pub(super) struct Detail {
    pub(super) rule: Rule,
    /// The field as the contract names it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) path: Option<&'static str>,
    /// The item where the rule broke, when the path steps into a list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) observed: Option<Observed>,
}

impl Detail {
    /// A rule of the run itself, such as the command exiting nonzero, which has no path.
    pub(super) const fn rule(rule: Rule) -> Self {
        Self {
            rule,
            path: None,
            at: None,
            observed: None,
        }
    }

    fn at(rule: Rule, path: &'static str, at: &str, value: Option<&Value>) -> Self {
        Self {
            rule,
            path: Some(path),
            at: (at != path).then(|| at.to_owned()),
            observed: Some(Observed::of(value)),
        }
    }

    /// A one-line description that names only the rule and the path.
    pub(super) fn describe(&self) -> String {
        match (self.at.as_deref().or(self.path), &self.observed) {
            (Some(path), Some(observed)) => format!(
                "the output breaks the contract rule `{}` at `{path}`: found {}",
                self.rule.as_str(),
                observed.describe()
            ),
            _ => format!("the check broke the rule `{}`", self.rule.as_str()),
        }
    }
}

/// The type of what a path held, with its length or count.
#[derive(Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum Observed {
    Missing,
    Null,
    String {
        length: usize,
    },
    Number,
    Bool,
    Array {
        count: usize,
    },
    Object {
        fields: usize,
    },
    /// How many of a list's items had the field.
    Items {
        with_field: usize,
        count: usize,
    },
}

impl Observed {
    fn of(value: Option<&Value>) -> Self {
        match value {
            None => Self::Missing,
            Some(Value::Null) => Self::Null,
            Some(Value::String(text)) => Self::String {
                length: text.chars().count(),
            },
            Some(Value::Number(_)) => Self::Number,
            Some(Value::Bool(_)) => Self::Bool,
            Some(Value::Array(items)) => Self::Array { count: items.len() },
            Some(Value::Object(fields)) => Self::Object {
                fields: fields.len(),
            },
        }
    }

    fn describe(&self) -> String {
        match self {
            Self::Missing => "nothing".into(),
            Self::Null => "null".into(),
            Self::String { length } => format!("a string of {length} characters"),
            Self::Number => "a number".into(),
            Self::Bool => "a boolean".into(),
            Self::Array { count } => format!("a list of {count} items"),
            Self::Object { fields } => format!("an object of {fields} fields"),
            Self::Items { with_field, count } => {
                format!("{with_field} of {count} items with the field")
            }
        }
    }
}

/// Returns the first rule `payload` breaks, or `None` when it meets `contract`.
pub(super) fn violation(contract: &LiveContract, payload: &Value) -> Option<Detail> {
    runtime_attempt(payload)
        .or_else(|| {
            contract
                .fields
                .iter()
                .find_map(|field| broken(field, payload))
        })
        .or_else(|| {
            contract
                .sample
                .iter()
                .find_map(|rule| sample_violation(*rule, payload))
        })
}

/// A runtime-class attempt means a route answered something it could not classify, even when
/// another route then succeeded.
fn runtime_attempt(payload: &Value) -> Option<Detail> {
    const PATH: &str = "provider_attempts[].error_kind";
    values(payload, PATH)
        .into_iter()
        .find(|(_, value)| value.and_then(Value::as_str) == Some("runtime"))
        .map(|(at, value)| Detail::at(Rule::NoRuntimeAttempt, PATH, &at, value))
}

fn broken(field: &Field, payload: &Value) -> Option<Detail> {
    let located = locate(payload, field.path);
    if field.in_some_items {
        let with_field = located
            .iter()
            .filter(|(_, reach)| matches!(reach, Reach::Field(value) if shape_rule(field.shape, *value).is_none()))
            .count();
        return (with_field == 0).then_some(Detail {
            rule: Rule::InSomeItems,
            path: Some(field.path),
            at: None,
            observed: Some(Observed::Items {
                with_field,
                count: located.len(),
            }),
        });
    }
    located.iter().find_map(|(at, reach)| match *reach {
        Reach::Field(value) => {
            shape_rule(field.shape, value).map(|rule| Detail::at(rule, field.path, at, value))
        }
        Reach::Stuck(value) => {
            let rule = if value.is_none_or(Value::is_null) {
                Rule::Present
            } else {
                Rule::Type
            };
            Some(Detail::at(rule, field.path, at, value))
        }
    })
}

/// The rule `value` breaks for `shape`, if any.
fn shape_rule(shape: Shape, value: Option<&Value>) -> Option<Rule> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Some(Rule::Present);
    };
    let (typed, bounded) = match shape {
        Shape::Text => (
            value.is_string(),
            value.as_str().is_some_and(|text| !text.trim().is_empty()),
        ),
        Shape::Count { min } => (
            value.is_u64(),
            value.as_u64().is_some_and(|count| count >= min),
        ),
        Shape::Number { min } => (
            value.is_number(),
            value.as_f64().is_some_and(|number| number >= min),
        ),
        Shape::Bool => (value.is_boolean(), true),
        Shape::List { min } => (
            value.is_array(),
            value.as_array().is_some_and(|items| items.len() >= min),
        ),
    };
    if !typed {
        Some(Rule::Type)
    } else if !bounded {
        Some(match shape {
            Shape::Text => Rule::Nonblank,
            Shape::List { .. } => Rule::MinItems,
            _ => Rule::AtLeast,
        })
    } else {
        None
    }
}

/// What a path found at one place.
enum Reach<'a> {
    /// The field itself, `None` when it is absent.
    Field(Option<&'a Value>),
    /// The path could not go on, because what it found here, if anything, is not an object.
    Stuck(Option<&'a Value>),
}

/// Every place `path` reaches, with its concrete location. A `name[]` step goes into each item
/// of the list at `name`; a list that is absent or empty reaches nothing, because the list has
/// its own rule.
fn locate<'a>(payload: &'a Value, path: &str) -> Vec<(String, Reach<'a>)> {
    let steps = path.split('.').collect::<Vec<_>>();
    let mut objects = vec![(String::new(), payload)];
    let mut reached = Vec::new();
    for (position, step) in steps.iter().enumerate() {
        let last = position + 1 == steps.len();
        let (key, into_items) = step
            .strip_suffix("[]")
            .map_or((*step, false), |key| (key, true));
        let mut next = Vec::new();
        for (at, value) in objects {
            let Some(object) = value.as_object() else {
                reached.push((at, Reach::Stuck(Some(value))));
                continue;
            };
            let at = if at.is_empty() {
                key.to_owned()
            } else {
                format!("{at}.{key}")
            };
            let field = object.get(key);
            if into_items {
                let items = field.and_then(Value::as_array).into_iter().flatten();
                for (index, item) in items.enumerate() {
                    let at = format!("{at}[{index}]");
                    if last {
                        reached.push((at, Reach::Field(Some(item))));
                    } else {
                        next.push((at, item));
                    }
                }
            } else if last {
                reached.push((at, Reach::Field(field)));
            } else if let Some(field) = field {
                next.push((at, field));
            } else {
                reached.push((at, Reach::Stuck(None)));
            }
        }
        objects = next;
    }
    reached
}

/// The values of the field `path` names, wherever the path reaches it.
fn values<'a>(payload: &'a Value, path: &str) -> Vec<(String, Option<&'a Value>)> {
    locate(payload, path)
        .into_iter()
        .filter_map(|(at, reach)| match reach {
            Reach::Field(value) => Some((at, value)),
            Reach::Stuck(_) => None,
        })
        .collect()
}

fn sample_violation(rule: SampleRule, payload: &Value) -> Option<Detail> {
    match rule {
        SampleRule::Credentials { anonymous } => {
            const PATH: &str = "provider_attempts[].anonymous";
            let name = if anonymous {
                Rule::AnonymousRead
            } else {
                Rule::KeyedRead
            };
            let attempts = values(payload, PATH);
            if attempts.is_empty() {
                return Some(Detail::at(
                    name,
                    "provider_attempts",
                    "provider_attempts",
                    payload.get("provider_attempts"),
                ));
            }
            attempts
                .into_iter()
                .find(|(_, value)| value.and_then(Value::as_bool) != Some(anonymous))
                .map(|(at, value)| Detail::at(name, PATH, &at, value))
        }
        SampleRule::ResolvedByPayout { winner } => resolved_by_payout(payload, winner),
        SampleRule::LiveQuotes => {
            let open = values(payload, "markets[].status")
                .iter()
                .any(|(_, status)| status.and_then(Value::as_str) == Some("open"));
            let source = payload.get("price_source");
            (open && source.and_then(Value::as_str) != Some("clob"))
                .then(|| Detail::at(Rule::LiveQuotes, "price_source", "price_source", source))
        }
    }
}

/// The winning market resolved Yes at a payout of 1, priced by its resolution, and the fetch
/// made one attempt, so it never asked the CLOB.
fn resolved_by_payout(payload: &Value, winner: &str) -> Option<Detail> {
    const RULE: Rule = Rule::ResolvedByPayout;
    let markets = payload["markets"].as_array().map_or(&[][..], Vec::as_slice);
    let Some((index, market)) = markets
        .iter()
        .enumerate()
        .find(|(_, market)| market["ref"] == winner)
    else {
        return Some(Detail {
            rule: RULE,
            path: Some("markets[].ref"),
            at: None,
            observed: Some(Observed::Array {
                count: markets.len(),
            }),
        });
    };
    let at = |field: &str| format!("markets[{index}].{field}");
    let yes = &market["outcomes"][0];
    let checks: [(&'static str, String, Option<&Value>, bool); 4] = [
        (
            "markets[].status",
            at("status"),
            market.get("status"),
            market["status"] == "resolved",
        ),
        (
            "markets[].outcomes[].name",
            at("outcomes[0].name"),
            yes.get("name"),
            yes["name"] == "Yes",
        ),
        (
            "markets[].outcomes[].price",
            at("outcomes[0].price"),
            yes.get("price"),
            yes["price"].as_f64().is_some_and(|price| price >= 1.0),
        ),
        (
            "markets[].outcomes[].basis",
            at("outcomes[0].basis"),
            yes.get("basis"),
            yes["basis"] == "resolution",
        ),
    ];
    if let Some((path, at, value, _)) = checks.into_iter().find(|(.., holds)| !holds) {
        return Some(Detail::at(RULE, path, &at, value));
    }
    let attempts = payload.get("provider_attempts");
    (attempts.and_then(Value::as_array).map(Vec::len) != Some(1))
        .then(|| Detail::at(RULE, "provider_attempts", "provider_attempts", attempts))
}

#[cfg(test)]
#[path = "live_contract_tests.rs"]
mod tests;
