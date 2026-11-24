//! Google Scholar fixtures: the stored SerpApi bodies of `tests/samples/serpapi/` and
//! configuration, shared by the `platform scholar` and `serpapi` tests.

use serde_json::Value;

use super::samples::sample;

/// The stored SerpApi body `name`, as the upstream answered it.
pub(crate) fn fixture(name: &str) -> Value {
    serde_json::from_str(&sample(&format!("serpapi/{name}")))
        .unwrap_or_else(|error| panic!("stored serpapi/{name}: {error}"))
}

/// A configuration that sends SerpApi requests to `url` with `keys` and never waits to retry.
pub(crate) fn config(url: &str, keys: &[&str]) -> String {
    let keys = keys
        .iter()
        .map(|key| format!("\"{key}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "[providers.serpapi]\nurl = \"{url}/search.json\"\nkeys = [{keys}]\n[retry]\nmax_wait = 0\n"
    )
}
