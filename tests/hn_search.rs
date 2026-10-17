mod support;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Value, json};

use support::hn::{comment_hit, config, path, payload, query, search_page, story_hit};
use support::{Fixture, Response, RunEnvironment};

fn search(environment: &RunEnvironment, arguments: &[&str]) -> std::process::Output {
    let mut command = vec!["platform", "hn", "search"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

/// Builds the cursor of `page` for a `rust` search of `limit` results, as a previous page
/// would have issued it.
fn cursor(limit: u16, page: u64) -> String {
    let request = json!({
        "query": "rust",
        "limit": limit,
        "options": {"platform": "hn"},
        "page": page.to_string()
    });
    format!("v1.hn_api.{}", URL_SAFE_NO_PAD.encode(request.to_string()))
}

#[test]
fn options_map_to_the_date_endpoint_with_tags_and_numeric_filters() {
    let algolia = Fixture::start_sequence(vec![search_page(&[], 0, 0)]);
    let environment = RunEnvironment::new(&config(&algolia.url, "http://127.0.0.1:9"));

    let output = search(
        &environment,
        &[
            "\"sync service\" -dropbox",
            "--sort",
            "date",
            "--type",
            "show_hn",
            "--type",
            "comment",
            "--type",
            "show_hn",
            "--author",
            "pg",
            "--story",
            "8863",
            "--from",
            "2024-01-01",
            "--to",
            "2024-01-31",
            "--min-comments",
            "5",
            "--limit",
            "10",
        ],
    );
    let request = algolia.finish();

    assert_eq!(
        (output.status.code(), path(&request), query(&request)),
        (
            Some(0),
            "/api/v1/search_by_date".to_owned(),
            [
                ("query", "\"sync service\" -dropbox"),
                ("tags", "(show_hn,comment),author_pg,story_8863"),
                (
                    "numericFilters",
                    "created_at_i>=1704067200,created_at_i<=1706745599,num_comments>=5"
                ),
                ("hitsPerPage", "10"),
            ]
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect()
        )
    );
}

#[test]
fn hits_carry_item_fields_and_comments_carry_their_story() {
    let algolia = Fixture::start_sequence(vec![search_page(&[story_hit(), comment_hit()], 0, 1)]);
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

#[test]
fn a_cursor_continues_the_search_on_the_next_page() {
    let algolia = Fixture::start_sequence(vec![
        search_page(&[story_hit()], 0, 50),
        search_page(&[story_hit()], 1, 50),
    ]);
    let environment = RunEnvironment::new(&config(&algolia.url, "http://127.0.0.1:9"));

    let first = payload(&search(
        &environment,
        &["rust", "--type", "story", "--limit", "1"],
    ));
    let cursor = first["next_cursor"]
        .as_str()
        .expect("next cursor")
        .to_owned();
    let second = search(&environment, &["--cursor", &cursor]);
    let requests = algolia.finish_all();

    assert_eq!(
        (
            cursor.starts_with("v1.hn_api."),
            second.status.code(),
            query(&requests[1]),
        ),
        (
            true,
            Some(0),
            [
                ("query", "rust"),
                ("tags", "(story)"),
                ("hitsPerPage", "1"),
                ("page", "1"),
            ]
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect()
        )
    );
}

#[test]
fn no_cursor_is_issued_past_the_first_thousand_results() {
    let algolia = Fixture::start_sequence(vec![search_page(&[story_hit()], 9, 50)]);
    let environment = RunEnvironment::new(&config(&algolia.url, "http://127.0.0.1:9"));

    let output = search(&environment, &["--cursor", &cursor(100, 9)]);
    let request = algolia.finish();

    assert_eq!(
        (
            output.status.code(),
            query(&request).get("page").cloned(),
            payload(&output)["next_cursor"].clone()
        ),
        (Some(0), Some("9".to_owned()), Value::Null)
    );
}

#[test]
fn a_cursor_past_the_first_thousand_results_fails_before_any_request() {
    let algolia = Fixture::start_sequence(Vec::new());
    let environment = RunEnvironment::new(&config(&algolia.url, "http://127.0.0.1:9"));

    let output = search(&environment, &["--cursor", &cursor(100, 10)]);

    assert_eq!(
        (output.status.code(), algolia.finish_all().len()),
        (Some(2), 0)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("past the first 1000 results"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_search_without_hits_is_an_empty_success() {
    let algolia = Fixture::start_sequence(vec![search_page(&[], 0, 0)]);
    let environment = RunEnvironment::new(&config(&algolia.url, "http://127.0.0.1:9"));

    let output = search(&environment, &["nothing matches this"]);
    algolia.finish();

    assert_eq!(
        (output.status.code(), payload(&output)["items"].clone()),
        (Some(0), json!([]))
    );
}

#[test]
fn rejected_and_rate_limited_requests_map_to_their_error_kinds() {
    let algolia = Fixture::start_sequence(vec![
        Response::json(
            400,
            r#"{"code":400,"message":"Invalid value for \"numericFilters\""}"#,
        ),
        Response::json(429, r#"{"message":"Too many requests"}"#),
    ]);
    let environment = RunEnvironment::new(&config(&algolia.url, "http://127.0.0.1:9"));

    let rejected = payload(&search(&environment, &["rust"]));
    let limited = search(&environment, &["rust"]);
    algolia.finish_all();

    assert_eq!(
        (
            rejected["error_kind"].clone(),
            rejected["message"].clone(),
            limited.status.code(),
            payload(&limited)["error_kind"].clone()
        ),
        (
            json!("parameter"),
            json!("HN Search API rejected the request: Invalid value for \"numericFilters\""),
            Some(4),
            json!("rate_limited")
        )
    );
}

#[test]
fn contradictory_options_fail_before_any_request() {
    let algolia = Fixture::start_sequence(Vec::new());
    let environment = RunEnvironment::new(&config(&algolia.url, "http://127.0.0.1:9"));

    let outputs = [
        search(
            &environment,
            &["rust", "--type", "comment", "--min-points", "10"],
        ),
        search(&environment, &["rust", "--story", "08863"]),
        search(&environment, &["rust", "--limit", "101"]),
    ];

    assert_eq!(
        (
            outputs.map(|output| output.status.code()),
            algolia.finish_all().len()
        ),
        ([Some(2); 3], 0)
    );
}
