//! X platform configuration and stored responses shared by the `platform x` tests.

use serde_json::Value;

use super::Response;
use super::main_search_samples::completed_events;
use super::x_samples::stored;

/// The stored completed xAI response `name` from `tests/samples/xai`.
pub(crate) fn fixture(name: &str) -> Value {
    stored(name)
}

/// Streams `response` as xAI does: a creation event, then `response.completed`.
pub(crate) fn completed(response: &Value) -> Response {
    Response::sse(200, &completed_events(response))
}

/// A configuration that sends xAI requests to `url` with `keys`; main search also enables web
/// search, which platform requests must never send.
pub(crate) fn config(url: &str, keys: &[&str]) -> String {
    let keys = keys
        .iter()
        .map(|key| format!("\"{key}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "[providers.xai]\nurl = \"{url}\"\nkeys = [{keys}]\nmodel = \"grok-test\"\ntools = [\"web_search\", \"x_search\"]\n[retry]\nmax_wait = 0\n"
    )
}
