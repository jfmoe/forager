//! Google Scholar fixtures: trimmed real SerpApi responses, configuration, and request and
//! output readers shared by the `platform scholar` tests.

use std::collections::BTreeMap;
use std::process::Output;

use serde_json::Value;

use super::Response;

/// Reads a trimmed real SerpApi response from `tests/fixtures/scholar`.
pub(crate) fn fixture(name: &str) -> Value {
    let path = format!(
        "{}/tests/fixtures/scholar/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).expect("read fixture")).expect("fixture")
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

pub(crate) fn request_target(request: &str) -> reqwest::Url {
    let target = request.split_whitespace().nth(1).expect("request target");
    reqwest::Url::parse(&format!("http://fixture.test{target}")).expect("request URL")
}

pub(crate) fn query_pairs(request: &str) -> BTreeMap<String, String> {
    request_target(request)
        .query_pairs()
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}

pub(crate) fn refs(page: &Value) -> Vec<String> {
    page["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|item| item["ref"].as_str().expect("ref").to_owned())
        .collect()
}
