//! Shared configuration for `forager doctor` process tests.

use std::fmt::Write;

use super::Response;

/// The HTTP providers that [`shallow_config`] points at its URL, each with the key it gets when
/// it reads one. A shallow doctor sends one reachability request to each.
pub(crate) const SHALLOW_PROVIDERS: &[(&str, Option<&str>)] = &[
    ("xai", Some("xai-secret")),
    ("exa", Some("exa-secret")),
    ("tavily", Some("tavily-secret")),
    ("firecrawl", Some("firecrawl-secret")),
    ("jina", Some("jina-secret")),
    ("arxiv_api", None),
    ("ssrn_crossref", None),
    ("hn_api", None),
    ("polymarket_api", None),
];

/// Shallow doctor counts any HTTP answer as reachable, so an unauthenticated 401 will do.
pub(crate) fn reachable_responses(count: usize) -> Vec<Response> {
    (0..count)
        .map(|_| Response::new(401, "application/json", ""))
        .collect()
}

/// Points each of [`SHALLOW_PROVIDERS`] at `url`, with its key.
pub(crate) fn shallow_config(url: &str) -> String {
    let mut config = String::new();
    for (provider, key) in SHALLOW_PROVIDERS {
        let _ = write!(config, "\n[providers.{provider}]\nurl = {url:?}\n");
        if let Some(key) = key {
            let _ = writeln!(config, "keys = [\"{key}\"]");
        }
    }
    config
}
