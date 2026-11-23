//! X platform configuration, stored responses and output readers shared by the `platform x`
//! tests.

use std::process::Output;

use serde_json::Value;

use super::Response;
use super::x_samples::{completed_events, stored};

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

pub(crate) fn payload(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "parse JSON stdout: {error}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

pub(crate) fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

pub(crate) fn refs(page: &Value) -> Vec<String> {
    page["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|item| item["ref"].as_str().expect("ref").to_owned())
        .collect()
}
