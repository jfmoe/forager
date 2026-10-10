//! The JSON cursor of the ChatGPT conversation response decoding: a value with its path from
//! the root of the response, and the error that names where the response stopped matching the
//! shape forager decodes.

use serde_json::Value;

/// Where a response stopped matching the shape forager decodes.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub(super) struct ShapeError(String);

impl ShapeError {
    pub(super) fn at(path: &str, problem: &str) -> Self {
        Self(format!("{path}: {problem}"))
    }
}

/// A JSON value and its path from the root of the response, for shape errors.
#[derive(Clone)]
pub(super) struct Node<'a> {
    pub(super) value: &'a Value,
    path: String,
}

impl<'a> Node<'a> {
    pub(super) fn root(value: &'a Value, path: &str) -> Self {
        Self {
            value,
            path: path.to_owned(),
        }
    }

    pub(super) fn path(&self) -> &str {
        &self.path
    }

    /// The member `name`, null included, or `None` when it is absent.
    pub(super) fn member(&self, name: &str) -> Option<Self> {
        self.value.get(name).map(|value| Self {
            value,
            path: format!("{}.{name}", self.path),
        })
    }

    /// The non-null member `name`, or `None`.
    pub(super) fn get(&self, name: &str) -> Option<Self> {
        self.member(name).filter(|member| !member.value.is_null())
    }

    pub(super) fn field(&self, name: &str) -> Result<Self, ShapeError> {
        self.get(name)
            .ok_or_else(|| self.error(&format!("no `{name}`")))
    }

    pub(super) fn items(&self) -> Result<impl Iterator<Item = Node<'a>>, ShapeError> {
        let path = self.path.clone();
        let items = self
            .value
            .as_array()
            .ok_or_else(|| self.error("not an array"))?;
        Ok(items.iter().enumerate().map(move |(index, value)| Node {
            value,
            path: format!("{path}[{index}]"),
        }))
    }

    pub(super) fn text(&self) -> Result<&'a str, ShapeError> {
        self.value
            .as_str()
            .ok_or_else(|| self.error("not a string"))
    }

    pub(super) fn index(&self) -> Result<usize, ShapeError> {
        self.value
            .as_u64()
            .and_then(|number| usize::try_from(number).ok())
            .ok_or_else(|| self.error("not a whole number"))
    }

    pub(super) fn error(&self, problem: &str) -> ShapeError {
        ShapeError::at(&self.path, problem)
    }
}
