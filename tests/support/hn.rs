//! Hacker News fixtures: configuration for the two endpoints of `hn_api`, record builders in
//! the shapes the Algolia and Firebase APIs returned on 2026-10-17, and request and output
//! readers shared by the `platform hn` tests.

use std::collections::BTreeMap;
use std::process::Output;

use serde_json::{Value, json};

use super::Response;

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

/// An Algolia search response holding `hits` on page `page` of `pages`.
pub(crate) fn search_page(hits: &[Value], page: u64, pages: u64) -> Response {
    json(&json!({
        "hits": hits,
        "nbHits": hits.len(),
        "page": page,
        "nbPages": pages,
        "hitsPerPage": hits.len(),
        "exhaustiveNbHits": true,
        "query": "",
        "params": ""
    }))
}

/// The Algolia hit of the 2007 Dropbox launch story.
pub(crate) fn story_hit() -> Value {
    json!({
        "_tags": ["story", "author_dhouston", "story_8863"],
        "author": "dhouston",
        "created_at": "2007-04-04T19:16:40Z",
        "created_at_i": 1_175_714_200,
        "num_comments": 70,
        "objectID": "8863",
        "points": 100,
        "story_id": 8863,
        "title": "My YC app: Dropbox - Throw away your USB drive",
        "updated_at": "2026-10-01T00:00:00Z",
        "url": "http://www.getdropbox.com/u/2/screencast.html"
    })
}

/// An Algolia comment hit on the Dropbox story.
pub(crate) fn comment_hit() -> Value {
    json!({
        "_tags": ["comment", "author_brett", "story_8863"],
        "author": "brett",
        "comment_text": "I&#x27;ve used <i>this</i>.<p>See <a href=\"https:&#x2F;&#x2F;example.com&#x2F;post\" rel=\"nofollow\">https:&#x2F;&#x2F;example.com&#x2F;post</a>",
        "created_at": "2007-04-04T20:53:20Z",
        "created_at_i": 1_175_720_000,
        "objectID": "8952",
        "parent_id": 8863,
        "points": null,
        "story_id": 8863,
        "story_title": "My YC app: Dropbox - Throw away your USB drive",
        "story_url": "http://www.getdropbox.com/u/2/screencast.html"
    })
}

/// The Firebase item of the Dropbox story, with live counts and ranked replies.
pub(crate) fn story_item(kids: &[u64]) -> Value {
    json!({
        "by": "dhouston",
        "descendants": 71,
        "id": 8863,
        "kids": kids,
        "score": 104,
        "time": 1_175_714_200,
        "title": "My YC app: Dropbox - Throw away your USB drive",
        "type": "story",
        "url": "http://www.getdropbox.com/u/2/screencast.html"
    })
}

/// One node of an Algolia item tree.
pub(crate) fn node(id: u64, author: &str, text: &str, children: &[Value]) -> Value {
    json!({
        "author": author,
        "children": children,
        "created_at": "",
        "created_at_i": 1_175_714_200 + id,
        "id": id,
        "options": [],
        "parent_id": 8863,
        "points": null,
        "story_id": 8863,
        "text": text,
        "title": null,
        "type": "comment",
        "url": null
    })
}

/// The Algolia tree of the Dropbox story with `children`.
pub(crate) fn story_tree(children: &[Value]) -> Value {
    json!({
        "author": "dhouston",
        "children": children,
        "created_at": "2007-04-04T19:16:40.000Z",
        "created_at_i": 1_175_714_200,
        "id": 8863,
        "options": [],
        "parent_id": null,
        "points": 100,
        "story_id": 8863,
        "text": null,
        "title": "My YC app: Dropbox - Throw away your USB drive",
        "type": "story",
        "url": "http://www.getdropbox.com/u/2/screencast.html"
    })
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

/// Returns the path of a recorded request.
pub(crate) fn path(request: &str) -> String {
    let target = request.split_whitespace().nth(1).expect("request target");
    target.split('?').next().unwrap_or_default().to_owned()
}

/// Returns the decoded query parameters of a recorded request.
pub(crate) fn query(request: &str) -> BTreeMap<String, String> {
    let target = request.split_whitespace().nth(1).expect("request target");
    reqwest::Url::parse(&format!("http://fixture.test{target}"))
        .expect("request URL")
        .query_pairs()
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}

pub(crate) fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
