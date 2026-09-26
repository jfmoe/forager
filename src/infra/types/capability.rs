use std::collections::HashSet;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// Whether provider and model fallback chains may continue after the primary choice.
pub enum FallbackPolicy {
    /// Continue through configured fallback choices.
    Auto,
    /// Execute only the primary configured choice.
    Off,
}

impl FallbackPolicy {
    /// Returns whether another configured choice may run.
    #[must_use]
    pub const fn allows_fallback(self) -> bool {
        matches!(self, Self::Auto)
    }

    /// Returns the stable configuration and output identifier.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Off => "off",
        }
    }
}

impl std::fmt::Display for FallbackPolicy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for FallbackPolicy {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "auto" => Ok(Self::Auto),
            "off" => Ok(Self::Off),
            _ => Err(format!(
                "invalid fallback policy `{value}`; expected auto or off"
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// A research capability exposed by an execution seam.
pub enum Capability {
    DocsSearch,
    WebSearch,
    WebFetch,
    VerticalSearch,
}

impl Capability {
    const VOCABULARY: [Self; 4] = [
        Self::DocsSearch,
        Self::WebSearch,
        Self::WebFetch,
        Self::VerticalSearch,
    ];

    /// Returns the stable snake-case capability identifier used by CLI output.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DocsSearch => "docs_search",
            Self::WebSearch => "web_search",
            Self::WebFetch => "web_fetch",
            Self::VerticalSearch => "vertical_search",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// A deduplicated capability set kept in the canonical [`Capability`] order.
pub struct CapabilitySet(Vec<Capability>);

impl CapabilitySet {
    pub(crate) fn from_capabilities(capabilities: impl IntoIterator<Item = Capability>) -> Self {
        let selected = capabilities.into_iter().collect::<HashSet<_>>();
        Self(
            Capability::VOCABULARY
                .into_iter()
                .filter(|capability| selected.contains(capability))
                .collect(),
        )
    }

    pub(crate) fn default_supplemental_web_search() -> Self {
        Self(vec![Capability::WebSearch])
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = Capability> + '_ {
        self.0.iter().copied()
    }
}

impl FromStr for CapabilitySet {
    type Err = String;

    fn from_str(declaration: &str) -> Result<Self, Self::Err> {
        if declaration.trim().is_empty() {
            return Err(
                "capability declaration must not be empty; use `none` for an empty set".into(),
            );
        }
        let values = declaration
            .split(',')
            .map(|value| value.trim().to_ascii_lowercase())
            .collect::<Vec<_>>();
        if values.iter().any(String::is_empty) {
            return Err(
                "capability declaration contains an empty CSV value; use `none` for an empty set"
                    .into(),
            );
        }
        if values.iter().any(|value| value == "none") {
            return if values.len() == 1 {
                Ok(Self(Vec::new()))
            } else {
                Err("`none` must be used alone".into())
            };
        }
        if let Some(unknown) = values.iter().find(|value| {
            !Capability::VOCABULARY
                .iter()
                .any(|capability| capability.as_str() == value.as_str())
        }) {
            return Err(format!(
                "unknown capability `{unknown}`; expected docs_search, web_search, web_fetch, vertical_search, or none"
            ));
        }
        let selected = values.into_iter().collect::<HashSet<_>>();
        Ok(Self::from_capabilities(
            Capability::VOCABULARY
                .into_iter()
                .filter(|capability| selected.contains(capability.as_str())),
        ))
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// A capability that a Schema v1 research plan may authorize.
pub enum PlanCapability {
    /// Documentation Search Capability.
    DocsSearch,
    /// Supplemental Web Search Capability.
    WebSearch,
    /// Vertical Search Capability.
    VerticalSearch,
}

impl PlanCapability {
    pub(crate) fn as_capability(self) -> Capability {
        match self {
            Self::DocsSearch => Capability::DocsSearch,
            Self::WebSearch => Capability::WebSearch,
            Self::VerticalSearch => Capability::VerticalSearch,
        }
    }
}

impl<'de> Deserialize<'de> for PlanCapability {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "docs_search" => Ok(Self::DocsSearch),
            "web_search" => Ok(Self::WebSearch),
            "vertical_search" => Ok(Self::VerticalSearch),
            "web_fetch" => Err(serde::de::Error::custom(
                "`web_fetch` is a research engine invariant and is executed automatically",
            )),
            _ => Err(serde::de::Error::custom(format!(
                "unknown plan capability `{value}`; expected docs_search, web_search, or vertical_search"
            ))),
        }
    }
}
