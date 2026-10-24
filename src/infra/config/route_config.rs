//! The runtime configuration of one route, by how it reaches its source: anonymous or keyed
//! HTTP, a model API, several official endpoints, or a local OpenCLI command.

use crate::redact::Secret;

/// The configuration of a platform route that sends HTTP requests without credentials.
#[derive(Clone, Debug)]
pub(crate) struct HttpRouteRuntimeConfig {
    pub(crate) url: String,
    pub(crate) timeout_seconds: u64,
}

/// The configuration of the Hacker News route, which combines two official endpoints under one
/// access policy.
#[derive(Clone, Debug)]
pub(crate) struct HnRouteRuntimeConfig {
    /// The Algolia Hacker News Search API.
    pub(crate) url: String,
    /// The official Hacker News Firebase API.
    pub(crate) firebase_url: String,
    pub(crate) timeout_seconds: u64,
}

/// The configuration of the Polymarket route, which combines official endpoints under one
/// access policy.
#[derive(Clone, Debug)]
pub(crate) struct PolymarketRouteRuntimeConfig {
    /// The Gamma API: discovery and metadata.
    pub(crate) url: String,
    /// The CLOB API: live order books and last trades.
    pub(crate) clob_url: String,
    /// The Data API: price history.
    pub(crate) data_url: String,
    pub(crate) timeout_seconds: u64,
}

/// The configuration of a platform route that sends HTTP requests with pooled credentials.
#[derive(Clone, Debug)]
pub(crate) struct KeyedHttpRouteRuntimeConfig {
    pub(crate) url: String,
    pub(crate) keys: Vec<Secret>,
    pub(crate) timeout_seconds: u64,
}

/// The configuration of a platform route that sends model requests with pooled credentials.
#[derive(Clone, Debug)]
pub(crate) struct ModelRouteRuntimeConfig {
    pub(crate) url: String,
    pub(crate) keys: Vec<Secret>,
    pub(crate) model: String,
    pub(crate) timeout_seconds: u64,
}

/// The configuration of a route that runs local OpenCLI commands.
#[derive(Clone, Debug)]
pub(crate) struct ProcessRouteRuntimeConfig {
    pub(crate) command: String,
    pub(crate) timeout_seconds: u64,
}
