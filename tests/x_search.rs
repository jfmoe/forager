//! `forager platform x search` against a local xAI Responses fixture that replays the stored
//! search response.

mod support;

use serde_json::{Value, json};

use support::x::{completed, config, fixture, payload, refs, stderr};
use support::{Fixture, RunEnvironment};

const KEY: &str = "test-xai-key-a";

#[test]
fn verified_posts_become_snippet_items_dated_by_their_id() {
    let fixture = Fixture::start_sequence(vec![completed(&fixture("x-search.json"))]);
    let environment = RunEnvironment::new(&config(&fixture.url, &[KEY]));

    let output = environment.run(&["platform", "x", "search", "Rust 2027 edition"]);
    fixture.finish_all();
    let page = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &page["platform"],
            &page["provider"],
            &page["next_cursor"]
        ),
        (Some(0), &json!("x"), &json!("xai"), &Value::Null),
        "stderr: {}",
        stderr(&output)
    );
    assert_eq!(
        page["items"][0],
        json!({
            "ref": "x:2106088717547077910",
            "url": "https://x.com/i/status/2106088717547077910",
            "depth": "snippet",
            "title": null,
            "authors": ["loncothad"],
            "published": "2026-10-02T18:27:34Z",
            "author_handle": "loncothad",
            "text": "Rust developers (of The Rust, not working With) are on the generation run to make the Edition 2027 the dream language. Wow. But I can't see the reason why - AI is banned there and it's not like any new cash grants were given to them... Just a random lock in, it seems..."
        })
    );
    assert_eq!(
        refs(&page),
        ["x:2106088717547077910", "x:2110286468623351937"]
    );
}
