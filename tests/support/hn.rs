//! Hacker News fixtures: configuration for the two endpoints of `hn_api`, the stored Algolia and
//! Firebase bodies as fixture responses, and output readers shared by the `platform hn` tests.

use std::process::Output;

use serde_json::Value;

use super::Response;
use super::hn_samples::{discussion, with};

/// Points `hn_api` at the two fixture servers with one attempt per request, so a failure
/// response is terminal at once.
pub(crate) fn config(algolia: &str, firebase: &str) -> String {
    format!(
        "[providers.hn_api]\nurl = \"{algolia}/api/v1\"\nfirebase_url = \"{firebase}/v0\"\n\n[retry]\nmax_attempts = 1\n"
    )
}

pub(crate) fn json(body: &Value) -> Response {
    Response::json(200, &body.to_string())
}

/// The stored Algolia search page holding `hits` as page `page` of `pages`.
pub(crate) fn search_page(hits: &[Value], page: u64, pages: u64) -> Response {
    json(&super::hn_samples::search_page(hits, page, pages))
}

/// The stored Algolia tree of the Dropbox story with `children` in place of its comments.
pub(crate) fn story_tree(children: &[Value]) -> Value {
    with(discussion(), &serde_json::json!({"children": children}))
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
