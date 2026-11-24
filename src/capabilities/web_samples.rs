//! The Web Fetch and web search answers of the in-crate tests: the variants shared with the
//! command contract tests, plus those only the in-crate tests use.

use serde_json::Value;

use crate::{fixture_server, samples};

// The shared variants live with the test support under `tests/` because the command contract
// tests use them too.
#[path = "../../tests/support/web_samples.rs"]
mod shared;

pub(crate) use shared::*;

/// The stored Tavily search answer whose results are `results`.
pub(crate) fn tavily_results(results: &Value) -> fixture_server::Response {
    let mut body = samples::sample_json("tavily/search.json");
    body["results"] = results.clone();
    fixture_server::Response::json(200, &body.to_string())
}
