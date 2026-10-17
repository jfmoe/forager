mod support;

use std::fs;
use std::process::Output;

use serde_json::{Value, json};

use support::hn::{
    comment_hit, config, json, node, path, payload, query, search_page, stderr, story_hit,
    story_item, story_tree,
};
use support::{Fixture, Response, RunEnvironment, request_json};

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
fn without_the_firebase_rank_every_comment_is_in_time_order() {
    let algolia = Fixture::start_sequence(vec![json(&discussion())]);
    let firebase = Fixture::start_sequence(vec![Response::json(503, "{}")]);
    let environment = RunEnvironment::new(&config(&algolia.url, &firebase.url));

    let output = fetch(&environment, &["hn:8863", "--format", "content"]);
    algolia.finish_all();
    firebase.finish_all();
    let expected = [
        format!(
            "{HEADER}- Points: 100\n- Comments: 6\n- Published: 2007-04-04T19:16:40Z\n\n## Comments\n"
        ),
        comment(0, "alice", 10, "First\n\nSecond"),
        comment(1, "dave", 13, "reply b"),
        comment(2, "frank", 14, "deep"),
        comment(1, "erin", 15, "reply c"),
        comment(0, "bob", 11, "Second top"),
        comment(0, "carol", 12, "Third"),
    ]
    .concat();

    assert_eq!(
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
        ),
        (Some(0), format!("{expected}\n"))
    );
    assert!(
        stderr(&output).contains(
            "HN Firebase API unavailable (HN Firebase API returned HTTP 503 (Service Unavailable)); comments are in chronological order and counts may lag"
        ),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_comment_thread_names_its_story_and_prints_without_a_file() {
    let mut root = node(
        8952,
        "brett",
        "Root <i>comment</i>",
        &[node(9000, "erin", "reply", &[])],
    );
    root["created_at_i"] = json!(1_175_714_252);
    let algolia = Fixture::start_sequence(vec![json(&root)]);
    let firebase = Fixture::start_sequence(vec![
        json(
            &json!({"by": "brett", "id": 8952, "kids": [9000], "parent": 8863, "text": "Root", "time": 1_175_714_252, "type": "comment"}),
        ),
        json(&story_item(&[8952])),
    ]);
    let environment = RunEnvironment::new(&config(&algolia.url, &firebase.url));
    let content_dir = tempfile::tempdir().expect("content directory");

    let output = fetch(
        &environment,
        &[
            "hn:8952",
            "--format",
            "content",
            "--content-dir",
            content_dir.path().to_str().expect("UTF-8 path"),
        ],
    );
    algolia.finish_all();
    let firebase_paths = firebase
        .finish_all()
        .iter()
        .map(|request| path(request))
        .collect::<Vec<_>>();

    assert_eq!(
        (
            output.status.code(),
            firebase_paths,
            String::from_utf8_lossy(&output.stdout).into_owned(),
            fs::read_dir(content_dir.path()).expect("read content directory").count(),
        ),
        (
            Some(0),
            vec!["/v0/item/8952.json".to_owned(), "/v0/item/8863.json".to_owned()],
            [
                "# Comment by brett\n\n- Ref: `hn:8952` <https://news.ycombinator.com/item?id=8952>\n- Story: My YC app: Dropbox - Throw away your USB drive `hn:8863`\n- Author: brett\n- Published: 2007-04-04T19:17:32Z\n\nRoot *comment*\n\n## Comments\n".to_owned(),
                "\n- **erin** · 2007-04-04T21:46:40Z · `hn:9000`\n\n  reply\n\n".to_owned(),
            ]
            .concat(),
            0,
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn metadata_reads_the_indexed_item_with_live_counts_and_no_discussion() {
    let algolia = Fixture::start_sequence(vec![search_page(&[story_hit()], 0, 1)]);
    let firebase = Fixture::start_sequence(vec![json(&story_item(&[]))]);
    let environment = RunEnvironment::new(&config(&algolia.url, &firebase.url));

    let output = fetch(&environment, &["hn:8863", "--depth", "metadata"]);
    let algolia_request = algolia.finish();
    let firebase_request = firebase.finish();
    let payload = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            path(&algolia_request),
            query(&algolia_request),
            path(&firebase_request),
            [&payload["depth"], &payload["points"], &payload["comments"]],
            payload.get("content_path").is_none(),
        ),
        (
            Some(0),
            "/api/v1/search".to_owned(),
            [("filters", "objectID:8863"), ("hitsPerPage", "1")]
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .collect(),
            "/v0/item/8863.json".to_owned(),
            [&json!("metadata"), &json!(104), &json!(71)],
            true,
        )
    );
}

#[test]
fn an_item_too_new_for_the_search_index_is_read_from_firebase() {
    let algolia = Fixture::start_sequence(vec![search_page(&[], 0, 0)]);
    let firebase = Fixture::start_sequence(vec![json(&json!({
        "by": "erin", "id": 9999, "parent": 8863, "text": "Fresh &amp; new", "time": 1_175_714_200, "type": "comment"
    }))]);
    let environment = RunEnvironment::new(&config(&algolia.url, &firebase.url));

    let output = fetch(&environment, &["hn:9999", "--depth", "metadata"]);
    algolia.finish();
    firebase.finish();
    let mut payload = payload(&output);
    payload.as_object_mut().expect("object").retain(|key, _| {
        [
            "title",
            "item_type",
            "text",
            "story_ref",
            "story_title",
            "parent_ref",
        ]
        .contains(&key.as_str())
    });

    assert_eq!(
        (output.status.code(), payload),
        (
            Some(0),
            json!({
                "title": null,
                "item_type": "comment",
                "text": "Fresh & new",
                "story_ref": null,
                "story_title": null,
                "parent_ref": "hn:8863"
            })
        )
    );
}

#[test]
fn missing_deleted_and_unindexed_items_are_parameter_errors() {
    let algolia = Fixture::start_sequence(vec![
        search_page(&[], 0, 0),
        search_page(&[story_hit()], 0, 1),
        Response::json(404, r#"{"status":404,"error":"Not Found"}"#),
    ]);
    let firebase = Fixture::start_sequence(vec![
        Response::json(200, "null"),
        json(&json!({"id": 8863, "deleted": true, "type": "story", "time": 1})),
    ]);
    let environment = RunEnvironment::new(&config(&algolia.url, &firebase.url));

    let outputs = [
        fetch(&environment, &["hn:1", "--depth", "metadata"]),
        fetch(&environment, &["hn:8863", "--depth", "metadata"]),
        fetch(&environment, &["hn:8863"]),
    ];
    algolia.finish_all();
    firebase.finish_all();

    assert_eq!(
        outputs.map(|output| {
            let payload = payload(&output);
            (
                output.status.code(),
                payload["error_kind"].clone(),
                payload["message"].clone(),
            )
        }),
        [
            (
                Some(4),
                json!("parameter"),
                json!("HN item not found: hn:1")
            ),
            (
                Some(4),
                json!("parameter"),
                json!("HN item hn:8863 is deleted or dead")
            ),
            (
                Some(4),
                json!("parameter"),
                json!("HN item not found: hn:8863")
            ),
        ]
    );
}

/// Adds Firecrawl as the only configured Web Fetch provider.
fn with_web_fetch(algolia: &str, firebase: &str, firecrawl: &str) -> String {
    format!(
        "{}\n[providers.firecrawl]\nurl = \"{firecrawl}\"\nkeys = [\"firecrawl-key\"]\ntimeout = 5\n",
        config(algolia, firebase)
    )
}

#[test]
fn the_full_text_of_a_link_story_is_the_linked_page_through_web_fetch() {
    let body = format!("# Dropbox\n\n{}", "Screencast transcript. ".repeat(40));
    let algolia = Fixture::start_sequence(vec![search_page(&[story_hit()], 0, 1)]);
    let firebase = Fixture::start_sequence(vec![json(&story_item(&[]))]);
    let firecrawl = Fixture::start_sequence(vec![json(
        &json!({"success": true, "data": {"markdown": body}}),
    )]);
    let environment =
        RunEnvironment::new(&with_web_fetch(&algolia.url, &firebase.url, &firecrawl.url));
    let content_dir = tempfile::tempdir().expect("content directory");

    let output = fetch(
        &environment,
        &[
            "hn:8863",
            "--depth",
            "full_text",
            "--content-dir",
            content_dir.path().to_str().expect("UTF-8 path"),
        ],
    );
    algolia.finish();
    firebase.finish();
    let fetched = request_json(&firecrawl.finish())["url"].clone();
    let payload = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            fetched,
            [
                &payload["depth"],
                &payload["content_url"],
                &payload["content_provider"]
            ],
            fs::read_to_string(content_dir.path().join("hn-8863.md")).ok(),
        ),
        (
            Some(0),
            json!("http://www.getdropbox.com/u/2/screencast.html"),
            [
                &json!("full_text"),
                &json!("http://www.getdropbox.com/u/2/screencast.html"),
                &json!("firecrawl")
            ],
            Some(body),
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn the_full_text_of_a_text_post_is_its_own_title_and_body() {
    let mut ask = story_hit();
    ask["title"] = json!("Ask HN: How do you back up?");
    ask["url"] = Value::Null;
    ask["story_text"] = json!("I use <i>rsync</i>.");
    let algolia = Fixture::start_sequence(vec![search_page(&[ask], 0, 1)]);
    let firebase = Fixture::start_sequence(vec![json(&story_item(&[]))]);
    let firecrawl = Fixture::start_sequence(Vec::new());
    let environment =
        RunEnvironment::new(&with_web_fetch(&algolia.url, &firebase.url, &firecrawl.url));

    let output = fetch(
        &environment,
        &["hn:8863", "--depth", "full_text", "--format", "content"],
    );
    algolia.finish();
    firebase.finish();

    assert_eq!(
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            firecrawl.finish_all().len(),
        ),
        (
            Some(0),
            "# Ask HN: How do you back up?\n\nI use *rsync*.\n".to_owned(),
            0
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_comment_has_no_full_text() {
    let algolia = Fixture::start_sequence(vec![search_page(&[comment_hit()], 0, 1)]);
    let firebase = Fixture::start_sequence(vec![json(&json!({
        "by": "brett", "id": 8952, "parent": 8863, "text": "x", "time": 1, "type": "comment"
    }))]);
    let firecrawl = Fixture::start_sequence(Vec::new());
    let environment =
        RunEnvironment::new(&with_web_fetch(&algolia.url, &firebase.url, &firecrawl.url));

    let output = fetch(&environment, &["hn:8952", "--depth", "full_text"]);
    algolia.finish();
    firebase.finish();
    let payload = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            payload["error_kind"].clone(),
            payload["message"].clone(),
            firecrawl.finish_all().len(),
        ),
        (
            Some(4),
            json!("parameter"),
            json!(
                "hn:8952 is a comment; full_text applies to stories, polls, and jobs; use --depth thread"
            ),
            0,
        )
    );
}

#[test]
fn unrecognized_refs_and_unsupported_depths_fail_before_any_request() {
    let algolia = Fixture::start_sequence(Vec::new());
    let firebase = Fixture::start_sequence(Vec::new());
    let environment = RunEnvironment::new(&config(&algolia.url, &firebase.url));

    let outputs = [
        fetch(&environment, &["https://news.ycombinator.com/user?id=pg"]),
        fetch(&environment, &["hn:8863", "--depth", "abstract"]),
    ];

    assert_eq!(
        (
            outputs.each_ref().map(|output| output.status.code()),
            algolia.finish_all().len() + firebase.finish_all().len(),
        ),
        ([Some(2), Some(2)], 0)
    );
    assert!(
        stderr(&outputs[0]).contains("pass an `hn:<id>` ref or a Hacker News item URL"),
        "{}",
        stderr(&outputs[0])
    );
}
