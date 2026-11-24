//! `forager platform x search` against a local xAI Responses fixture that replays the stored
//! search response.

use serde_json::{Value, json};

use crate::support::x::{completed, config, fixture};
use crate::support::{Fixture, RunEnvironment, payload, refs, stderr};

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
            "ref": "x:2106088717546398899",
            "url": "https://x.com/i/status/2106088717546398899",
            "depth": "snippet",
            "title": null,
            "authors": ["example_dev1"],
            "published": "2026-10-02T18:27:34Z",
            "author_handle": "example_dev1",
            "text": "Rust developers (of the language, not just users of it) are working hard to make the Edition 2027 a great release. Nice. But I wonder which of the planned changes will land first - the list keeps growing and the release date is not far off... Just curious, I guess..."
        })
    );
    assert_eq!(
        refs(&page),
        ["x:2106088717546398899", "x:2110286468622476580"]
    );
}
