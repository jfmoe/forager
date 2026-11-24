//! Hacker News fixtures: configuration for the two endpoints of `hn_api`, the stored Algolia and
//! Firebase bodies as fixture responses, shared by the `platform hn` tests.

use serde_json::Value;

use super::hn_samples::{discussion, with};
use super::{Response, ok_json};

/// Points `hn_api` at the two fixture servers with one attempt per request, so a failure
/// response is terminal at once.
pub(crate) fn config(algolia: &str, firebase: &str) -> String {
    format!(
        "[providers.hn_api]\nurl = \"{algolia}/api/v1\"\nfirebase_url = \"{firebase}/v0\"\n\n[retry]\nmax_attempts = 1\n"
    )
}

/// The stored Algolia search page holding `hits` as page `page` of `pages`.
pub(crate) fn search_page(hits: &[Value], page: u64, pages: u64) -> Response {
    ok_json(&super::hn_samples::search_page(hits, page, pages))
}

/// The stored Algolia tree of the Dropbox story with `children` in place of its comments.
pub(crate) fn story_tree(children: &[Value]) -> Value {
    with(discussion(), &serde_json::json!({"children": children}))
}
