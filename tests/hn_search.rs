mod support;

use serde_json::json;

use support::hn::{config, payload};
use support::hn_samples::stored;
use support::{Fixture, Response, RunEnvironment};
use support::{path, query};

fn search(environment: &RunEnvironment, arguments: &[&str]) -> std::process::Output {
    let mut command = vec!["platform", "hn", "search"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

#[test]
fn hits_carry_item_fields_and_comments_carry_their_story() {
    let algolia = Fixture::start_sequence(vec![Response::json(200, &stored("search.json"))]);
    let environment = RunEnvironment::new(&config(&algolia.url, "http://127.0.0.1:9"));

    let output = search(&environment, &["dropbox"]);
    let request = algolia.finish();
    let payload = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            path(&request),
            query(&request).get("hitsPerPage").cloned(),
            payload
        ),
        (
            Some(0),
            "/api/v1/search".to_owned(),
            Some("20".to_owned()),
            json!({
                "platform": "hn",
                "provider": "hn_api",
                "items": [
                    {
                        "ref": "hn:8863",
                        "url": "https://news.ycombinator.com/item?id=8863",
                        "depth": "metadata",
                        "title": "My YC app: Dropbox - Throw away your USB drive",
                        "authors": ["dhouston"],
                        "published": "2007-04-04T19:16:40Z",
                        "item_type": "story",
                        "link_url": "http://www.getdropbox.com/u/2/screencast.html",
                        "points": 100,
                        "comments": 70,
                        "text": null,
                        "story_ref": null,
                        "story_title": null,
                        "parent_ref": null
                    },
                    {
                        "ref": "hn:8952",
                        "url": "https://news.ycombinator.com/item?id=8952",
                        "depth": "snippet",
                        "title": null,
                        "authors": ["brett"],
                        "published": "2007-04-04T20:53:20Z",
                        "item_type": "comment",
                        "link_url": null,
                        "points": null,
                        "comments": null,
                        "text": "I've used *this*.\n\nSee https://example.com/post",
                        "story_ref": "hn:8863",
                        "story_title": "My YC app: Dropbox - Throw away your USB drive",
                        "parent_ref": "hn:8863"
                    }
                ],
                "next_cursor": null
            })
        )
    );
}
