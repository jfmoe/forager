mod support;

use std::fs;
use std::process::Output;

use serde_json::{Value, json};

use support::hn::{config, json, node, payload, stderr, story_item, story_tree};
use support::path;
use support::{Fixture, RunEnvironment};

fn fetch(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "hn", "fetch"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

/// Three top-level comments posted in ID order, with replies listed out of order.
fn discussion() -> Value {
    story_tree(&[
        node(
            10,
            "alice",
            "First<p>Second",
            &[
                node(15, "erin", "reply c", &[]),
                node(13, "dave", "reply b", &[node(14, "frank", "deep", &[])]),
            ],
        ),
        node(11, "bob", "Second top", &[]),
        node(12, "carol", "Third", &[]),
    ])
}

const HEADER: &str = "# My YC app: Dropbox - Throw away your USB drive\n\n- Ref: `hn:8863` <https://news.ycombinator.com/item?id=8863>\n- Link: <http://www.getdropbox.com/u/2/screencast.html>\n- Author: dhouston\n";

fn comment(depth: usize, author: &str, id: u64, text: &str) -> String {
    let indent = "  ".repeat(depth);
    let lines = text
        .lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{indent}  {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "\n{indent}- **{author}** · 2007-04-04T19:16:{} · `hn:{id}`\n\n{lines}\n",
        format_args!("{}Z", 40 + id)
    )
}

#[test]
fn a_thread_is_written_to_a_file_with_top_level_comments_in_rank_order() {
    let algolia = Fixture::start_sequence(vec![json(&discussion())]);
    let firebase = Fixture::start_sequence(vec![json(&story_item(&[12, 99, 10]))]);
    let environment = RunEnvironment::new(&config(&algolia.url, &firebase.url));
    let content_dir = tempfile::tempdir().expect("content directory");

    let output = fetch(
        &environment,
        &[
            "https://news.ycombinator.com/item?id=8863&p=2",
            "--content-dir",
            content_dir.path().to_str().expect("UTF-8 path"),
        ],
    );
    let algolia_requests = algolia.finish_all();
    let firebase_requests = firebase.finish_all();
    let mut payload = payload(&output);
    let content_path = payload["content_path"]
        .as_str()
        .expect("content path")
        .to_owned();
    payload["content_path"] = Value::Null;
    let written = fs::read_to_string(&content_path).expect("read discussion file");
    let expected = [
        format!(
            "{HEADER}- Points: 104\n- Comments: 71\n- Published: 2007-04-04T19:16:40Z\n\n## Comments\n"
        ),
        comment(0, "carol", 12, "Third"),
        comment(0, "alice", 10, "First\n\nSecond"),
        comment(1, "dave", 13, "reply b"),
        comment(2, "frank", 14, "deep"),
        comment(1, "erin", 15, "reply c"),
        comment(0, "bob", 11, "Second top"),
    ]
    .concat();

    assert_eq!(
        (
            output.status.code(),
            algolia_requests
                .iter()
                .map(|request| path(request))
                .collect::<Vec<_>>(),
            firebase_requests
                .iter()
                .map(|request| path(request))
                .collect::<Vec<_>>(),
            content_path,
            written.clone(),
        ),
        (
            Some(0),
            vec!["/api/v1/items/8863".to_owned()],
            vec!["/v0/item/8863.json".to_owned()],
            content_dir.path().join("hn-8863.md").display().to_string(),
            expected,
        ),
        "stderr: {}",
        stderr(&output)
    );
    assert_eq!(
        payload,
        json!({
            "platform": "hn",
            "provider": "hn_api",
            "ref": "hn:8863",
            "url": "https://news.ycombinator.com/item?id=8863",
            "depth": "thread",
            "title": "My YC app: Dropbox - Throw away your USB drive",
            "authors": ["dhouston"],
            "published": "2007-04-04T19:16:40Z",
            "item_type": "story",
            "link_url": "http://www.getdropbox.com/u/2/screencast.html",
            "points": 104,
            "comments": 71,
            "text": null,
            "story_ref": null,
            "story_title": null,
            "parent_ref": null,
            "comments_delivered": 6,
            "order": "ranked_top_level",
            "content_url": "https://news.ycombinator.com/item?id=8863",
            "content_provider": "hn_api",
            "content_path": null,
            "content_len": written.chars().count()
        })
    );
}

#[test]
fn a_thread_that_cannot_be_written_is_a_runtime_failure() {
    let algolia = Fixture::start_sequence(vec![json(&discussion())]);
    let firebase = Fixture::start_sequence(vec![json(&story_item(&[12, 99, 10]))]);
    let environment = RunEnvironment::new(&config(&algolia.url, &firebase.url));
    let blocker = tempfile::NamedTempFile::new().expect("a file where the directory should be");
    let content_dir = blocker.path().join("threads").display().to_string();

    let output = fetch(&environment, &["hn:8863", "--content-dir", &content_dir]);
    algolia.finish_all();
    firebase.finish_all();
    let payload = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            payload["error_kind"].clone(),
            payload["message"]
                .as_str()
                .is_some_and(|message| message.starts_with("cannot write the full text to ")),
        ),
        (Some(4), json!("runtime"), true),
        "{payload}"
    );
}
