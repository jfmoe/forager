mod support;

use std::process::Output;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use support::hn::{config, json, path, payload, query, search_page, stderr, story_hit};
use support::{Fixture, Response, RunEnvironment};

fn stories(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "hn", "stories"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

fn hit(id: u64, title: &str) -> Value {
    let mut hit = story_hit();
    hit["objectID"] = json!(id.to_string());
    hit["story_id"] = json!(id);
    hit["title"] = json!(title);
    hit
}

fn ranked(payload: &Value) -> Vec<(Value, Value, Value)> {
    payload["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|item| {
            (
                item["rank"].clone(),
                item["ref"].clone(),
                item["title"].clone(),
            )
        })
        .collect()
}

#[test]
fn the_front_page_lists_items_in_rank_order_and_reads_unindexed_ones_from_firebase() {
    let firebase = Fixture::start_sequence(vec![
        Response::json(200, "[30, 10, 20, 40]"),
        json(
            &json!({"by": "erin", "id": 20, "score": 5, "time": 1_175_714_200, "title": "Too new", "type": "story", "descendants": 0}),
        ),
    ]);
    let algolia = Fixture::start_sequence(vec![search_page(
        &[hit(10, "Ten"), hit(30, "Thirty")],
        0,
        1,
    )]);
    let environment = RunEnvironment::new(&config(&algolia.url, &firebase.url));

    let started = Instant::now();
    let output = stories(&environment, &["--limit", "3"]);
    let elapsed = started.elapsed();
    let firebase_paths = firebase
        .finish_all()
        .iter()
        .map(|request| path(request))
        .collect::<Vec<_>>();
    let algolia_request = algolia.finish();
    let payload = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            firebase_paths,
            query(&algolia_request),
            [&payload["platform"], &payload["provider"], &payload["list"]],
            payload.get("next_cursor").is_none(),
            ranked(&payload),
        ),
        (
            Some(0),
            vec![
                "/v0/topstories.json".to_owned(),
                "/v0/item/20.json".to_owned()
            ],
            [
                ("filters", "objectID:30 OR objectID:10 OR objectID:20"),
                ("hitsPerPage", "3"),
            ]
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect(),
            [&json!("hn"), &json!("hn_api"), &json!("top")],
            true,
            vec![
                (json!(1), json!("hn:30"), json!("Thirty")),
                (json!(2), json!("hn:10"), json!("Ten")),
                (json!(3), json!("hn:20"), json!("Too new")),
            ],
        ),
        "stderr: {}",
        stderr(&output)
    );
    // Both endpoints share one access policy of one request every half second.
    assert!(elapsed >= Duration::from_secs(1), "{elapsed:?}");
}

#[test]
fn deleted_and_dead_items_are_skipped_with_one_diagnostic() {
    let firebase = Fixture::start_sequence(vec![
        Response::json(200, "[1, 2, 3]"),
        json(&json!({"id": 2, "deleted": true, "type": "story", "time": 1})),
        json(&json!({"id": 3, "dead": true, "by": "x", "type": "story", "time": 1})),
    ]);
    let algolia = Fixture::start_sequence(vec![search_page(&[hit(1, "One")], 0, 1)]);
    let environment = RunEnvironment::new(&config(&algolia.url, &firebase.url));

    let output = stories(&environment, &["ask"]);
    let firebase_paths = firebase
        .finish_all()
        .iter()
        .map(|request| path(request))
        .collect::<Vec<_>>();
    algolia.finish();
    let payload = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            firebase_paths[0].clone(),
            payload["list"].clone(),
            ranked(&payload),
        ),
        (
            Some(0),
            "/v0/askstories.json".to_owned(),
            json!("ask"),
            vec![(json!(1), json!("hn:1"), json!("One"))],
        )
    );
    assert!(
        stderr(&output).contains("skipped 2 deleted, dead, or missing items of the HN ask list"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_failed_item_read_skips_that_item_and_keeps_the_rest() {
    let firebase = Fixture::start_sequence(vec![
        Response::json(200, "[1, 2]"),
        Response::json(503, "{}"),
    ]);
    let algolia = Fixture::start_sequence(vec![search_page(&[hit(1, "One")], 0, 1)]);
    let environment = RunEnvironment::new(&config(&algolia.url, &firebase.url));

    let output = stories(&environment, &[]);
    firebase.finish_all();
    algolia.finish();
    let payload = payload(&output);

    assert_eq!(
        (output.status.code(), ranked(&payload)),
        (Some(0), vec![(json!(1), json!("hn:1"), json!("One"))]),
        "stderr: {}",
        stderr(&output)
    );
    assert!(
        stderr(&output).contains("could not read 1 item of the HN top list"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn an_empty_list_is_an_empty_success_and_a_failed_list_fails() {
    let firebase = Fixture::start_sequence(vec![
        Response::json(200, "[]"),
        Response::json(503, "{}"),
        Response::json(200, "null"),
    ]);
    let algolia = Fixture::start_sequence(Vec::new());
    let environment = RunEnvironment::new(&config(&algolia.url, &firebase.url));

    let empty = stories(&environment, &["job"]);
    let failed = stories(&environment, &["new"]);
    let missing = stories(&environment, &["best"]);
    firebase.finish_all();

    assert_eq!(
        (
            empty.status.code(),
            payload(&empty)["items"].clone(),
            failed.status.code(),
            payload(&failed)["error_kind"].clone(),
            missing.status.code(),
            payload(&missing)["message"].clone(),
            algolia.finish_all().len(),
        ),
        (
            Some(0),
            json!([]),
            Some(4),
            json!("network"),
            Some(4),
            json!("HN Firebase API returned no best story list"),
            0,
        )
    );
}

#[test]
fn an_out_of_range_limit_fails_before_any_request() {
    let firebase = Fixture::start_sequence(Vec::new());
    let environment = RunEnvironment::new(&config("http://127.0.0.1:9", &firebase.url));

    let outputs = [
        stories(&environment, &["--limit", "0"]),
        stories(&environment, &["--limit", "101"]),
        stories(&environment, &["front"]),
    ];

    assert_eq!(
        (
            outputs.map(|output| output.status.code()),
            firebase.finish_all().len()
        ),
        ([Some(2); 3], 0)
    );
}
