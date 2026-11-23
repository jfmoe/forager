//! Google Scholar fixtures: the stored SerpApi bodies of `tests/samples/serpapi/`, configuration,
//! and output readers shared by the `platform scholar` and `serpapi` tests.

use std::process::Output;

use serde_json::Value;

use super::Response;
use super::samples::sample;

/// The stored SerpApi body `name`, as the upstream answered it.
pub(crate) fn fixture(name: &str) -> Value {
    serde_json::from_str(&sample(&format!("serpapi/{name}")))
        .unwrap_or_else(|error| panic!("stored serpapi/{name}: {error}"))
}

pub(crate) fn ok(body: &Value) -> Response {
    Response::json(200, &body.to_string())
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
