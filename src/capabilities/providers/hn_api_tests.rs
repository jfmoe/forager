use std::time::Duration;

use serde_json::{Value, json};

use super::search::{SearchResponse, search_support};
use super::thread::decode_tree;
use super::{FirebaseItem, HnApi, decode_json, fetch_support, thread_item};
use crate::config::HnRouteRuntimeConfig;
use crate::fixture_server::{Fixture, Response, block_on, ok_json, pairs, path, query};
use crate::net::{RetryPolicy, build_client};
use crate::providers::RouteContext;
use crate::providers::test_projection::{failure, to_json};
use crate::rate_limit::test_pacing::paced_limiters;
// The stored bodies below read the sample store through this name.
use crate::samples;
use crate::types::{
    AttemptErrorKind, ContentDepth, Deadline, FullTextSource, HnItemType, HnRef, HnSearchOptions,
    HnSearchType, HnSort, HnStoriesList, HnStoriesOutcome, HnStoriesRequest, PlatformFetchOutcome,
    PlatformFetchRequest, PlatformRef, PlatformSearchOptions, PlatformSearchOutcome,
    PlatformSearchRequest, ProviderError,
};

#[path = "../../../tests/support/hn_samples.rs"]
mod stored;

use stored::{discussion, search_page, stored_json, story_hit, story_item, story_list, with};

/// The stored hit of a comment on the Dropbox story, `hn:8952`.
fn comment_hit() -> Value {
    stored_json("search.json")["hits"][1].clone()
}

/// The stored tree's first comment, a top-level reply to the story, as comment `id` by
/// `author` saying `text`, posted `id` seconds after the story, with `children`.
fn node(id: u64, author: &str, text: &str, children: &[Value]) -> Value {
    let tree = discussion();
    let posted = tree["created_at_i"].as_u64().expect("story time") + id;
    with(
        tree["children"][0].clone(),
        &json!({"id": id, "author": author, "text": text, "created_at_i": posted, "children": children}),
    )
}

/// The stored Firebase item as comment `id` by `by` on `parent` saying `text`, with `kids`.
fn firebase_comment(id: u64, by: &str, parent: u64, text: &str, kids: &[u64]) -> Value {
    with(
        without(
            stored_json("firebase-item.json"),
            &["descendants", "score", "title", "url"],
        ),
        &json!({"id": id, "by": by, "parent": parent, "text": text, "kids": kids, "type": "comment"}),
    )
}

fn without(mut record: Value, fields: &[&str]) -> Value {
    let object = record.as_object_mut().expect("record object");
    for field in fields {
        object.remove(*field);
    }
    record
}

/// The route with both endpoints on the fixture at `url`, under the paths the real endpoints
/// use, with one attempt per request, so a failure response is terminal at once.
fn route(url: &str, deadline: Duration) -> HnApi {
    HnApi::build(
        HnRouteRuntimeConfig {
            url: format!("{url}/api/v1"),
            firebase_url: format!("{url}/v0"),
            timeout_seconds: 5,
        },
        RouteContext {
            client: build_client(true).expect("build client"),
            retry_policy: RetryPolicy::new(1, 1.0, Duration::ZERO),
            deadline: Deadline::new(deadline),
        },
    )
}

fn stored_body(name: &str) -> String {
    stored::stored(name)
}

fn paths(requests: &[String]) -> Vec<String> {
    requests.iter().map(|request| path(request)).collect()
}

fn hn(id: u64) -> HnRef {
    HnRef::new(id).expect("valid item ID")
}

fn search_request(query: &str, limit: u16, options: HnSearchOptions) -> PlatformSearchRequest {
    PlatformSearchRequest {
        query: query.to_owned(),
        limit,
        options: PlatformSearchOptions::Hn(options),
        page: None,
    }
}

/// Searches with `request` against a fixture that answers `answers`, and returns the outcome
/// and the requests the fixture received.
fn search(
    request: &PlatformSearchRequest,
    answers: Vec<Response>,
) -> (Result<PlatformSearchOutcome, ProviderError>, Vec<String>) {
    let fixture = Fixture::start_sequence(answers);
    let outcome = block_on(route(&fixture.url, Duration::from_secs(10)).search(request));
    (outcome, fixture.finish_all())
}

/// Fetches `hn:<id>` at `depth` against a fixture that answers `answers` from both endpoints in
/// order, and returns the outcome and the requests the fixture received.
fn fetch(
    id: u64,
    depth: ContentDepth,
    answers: Vec<Response>,
) -> (Result<PlatformFetchOutcome, ProviderError>, Vec<String>) {
    let fixture = Fixture::start_sequence(answers);
    let request = PlatformFetchRequest::new(PlatformRef::Hn(hn(id)), depth);
    let outcome = block_on(route(&fixture.url, Duration::from_secs(10)).fetch(&request));
    (outcome, fixture.finish_all())
}

/// Lists `list` up to `limit` against a fixture that answers `answers` from both endpoints in
/// order, and returns the outcome and the requests the fixture received.
fn stories(
    list: HnStoriesList,
    limit: u16,
    answers: Vec<Response>,
) -> (Result<HnStoriesOutcome, ProviderError>, Vec<String>) {
    let fixture = Fixture::start_sequence(answers);
    let request = HnStoriesRequest { list, limit };
    let outcome = block_on(route(&fixture.url, Duration::from_secs(10)).stories(&request));
    (outcome, fixture.finish_all())
}

fn native(source: &FullTextSource) -> &str {
    match source {
        FullTextSource::Native(body) => body,
        FullTextSource::Urls(urls) => panic!("expected a native body, got {urls:?}"),
    }
}

fn hit(id: u64, title: &str) -> Value {
    with(
        story_hit(),
        &json!({"objectID": id.to_string(), "story_id": id, "title": title}),
    )
}

fn ranked(outcome: &HnStoriesOutcome) -> Vec<(usize, String, Option<String>)> {
    outcome
        .items
        .iter()
        .map(|ranked| {
            (
                ranked.rank,
                ranked.item.reference.to_string(),
                ranked.item.title.clone(),
            )
        })
        .collect()
}

const HEADER: &str = "# My YC app: Dropbox - Throw away your USB drive\n\n- Ref: `hn:8863` <https://news.ycombinator.com/item?id=8863>\n- Link: <http://www.getdropbox.com/u/2/screencast.html>\n- Author: dhouston\n";

/// One comment of the discussion file at `depth`, posted `id` seconds after the story.
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

// Search.

#[test]
fn a_search_page_decodes_stories_and_comments_with_their_story() {
    let page = decode_json::<SearchResponse>(&stored_body("search.json")).expect("a search page");

    assert_eq!(
        page.into_listed_items().map(|items| to_json(&items)),
        Ok(json!([
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
        ]))
    );
}

#[test]
fn options_map_to_the_date_endpoint_with_tags_and_numeric_filters() {
    let day = |value: &str| chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").ok();
    let request = search_request(
        "\"sync service\" -dropbox",
        10,
        HnSearchOptions {
            sort: HnSort::Date,
            types: vec![HnSearchType::ShowHn, HnSearchType::Comment],
            author: Some("pg".to_owned()),
            story: Some(8863),
            from: day("2024-01-01"),
            to: day("2024-01-31"),
            min_points: None,
            min_comments: Some(5),
        },
    );

    let (outcome, requests) = search(&request, vec![ok_json(&search_page(&[], 0, 0))]);

    assert_eq!(
        (
            outcome
                .map(|outcome| outcome.items.len())
                .map_err(|error| error.message),
            path(&requests[0]),
            query(&requests[0])
        ),
        (
            Ok(0),
            "/api/v1/search_by_date".to_owned(),
            pairs(&[
                ("query", "\"sync service\" -dropbox"),
                ("tags", "(show_hn,comment),author_pg,story_8863"),
                (
                    "numericFilters",
                    "created_at_i>=1704067200,created_at_i<=1706745599,num_comments>=5"
                ),
                ("hitsPerPage", "10"),
            ])
        )
    );
}

#[test]
fn a_first_page_short_of_the_last_names_the_next_page_position() {
    let request = search_request(
        "rust",
        1,
        HnSearchOptions {
            types: vec![HnSearchType::Story],
            ..HnSearchOptions::default()
        },
    );

    let (outcome, requests) = search(&request, vec![ok_json(&search_page(&[story_hit()], 0, 50))]);

    assert_eq!(
        (
            outcome
                .map(|outcome| outcome.next_page)
                .map_err(|error| error.message),
            path(&requests[0]),
            query(&requests[0])
        ),
        (
            Ok(Some("1".to_owned())),
            "/api/v1/search".to_owned(),
            pairs(&[("query", "rust"), ("tags", "(story)"), ("hitsPerPage", "1")])
        )
    );
}

#[test]
fn a_page_position_is_sent_as_the_algolia_page() {
    let request = PlatformSearchRequest {
        page: Some("1".to_owned()),
        ..search_request(
            "rust",
            1,
            HnSearchOptions {
                types: vec![HnSearchType::Story],
                ..HnSearchOptions::default()
            },
        )
    };

    let (outcome, requests) = search(&request, vec![ok_json(&search_page(&[story_hit()], 1, 50))]);

    assert_eq!(
        (
            outcome
                .map(|outcome| outcome.next_page)
                .map_err(|error| error.message),
            query(&requests[0])
        ),
        (
            Ok(Some("2".to_owned())),
            pairs(&[
                ("query", "rust"),
                ("tags", "(story)"),
                ("hitsPerPage", "1"),
                ("page", "1"),
            ])
        )
    );
}

#[test]
fn no_next_page_is_named_past_the_first_thousand_results() {
    let request = PlatformSearchRequest {
        page: Some("9".to_owned()),
        ..search_request("rust", 100, HnSearchOptions::default())
    };

    let (outcome, requests) = search(&request, vec![ok_json(&search_page(&[story_hit()], 9, 50))]);

    assert_eq!(
        (
            outcome
                .map(|outcome| outcome.next_page)
                .map_err(|error| error.message),
            query(&requests[0]).get("page").cloned()
        ),
        (Ok(None), Some("9".to_owned()))
    );
}

#[test]
fn a_page_past_the_first_thousand_results_is_refused_before_any_request() {
    let page = |position: &str| PlatformSearchRequest {
        page: Some(position.to_owned()),
        ..search_request("rust", 100, HnSearchOptions::default())
    };

    let (outcome, requests) = search(&page("10"), Vec::new());

    assert_eq!(
        (
            search_support(&page("9")),
            failure(outcome),
            requests.len()
        ),
        (
            Ok(()),
            (
                AttemptErrorKind::Parameter,
                "hn_api cannot read past the first 1000 results of a search; narrow the query or filters"
                    .to_owned()
            ),
            0
        )
    );
}

#[test]
fn a_search_without_hits_is_an_empty_success() {
    let request = search_request("nothing matches this", 20, HnSearchOptions::default());

    let (outcome, _) = search(&request, vec![ok_json(&search_page(&[], 0, 0))]);

    assert_eq!(
        outcome
            .map(|outcome| (outcome.items.len(), outcome.next_page))
            .map_err(|error| error.message),
        Ok((0, None))
    );
}

#[test]
fn rejected_and_rate_limited_requests_map_to_their_error_kinds() {
    let request = search_request("rust", 20, HnSearchOptions::default());
    let error = stored_body("search-error.json");

    let (rejected, _) = search(&request, vec![Response::json(400, &error)]);
    let (limited, _) = search(&request, vec![Response::json(429, &error)]);

    assert_eq!(
        (failure(rejected), failure(limited).0),
        (
            (
                AttemptErrorKind::Parameter,
                "HN Search API rejected the request: Invalid value for \"numericFilters\""
                    .to_owned()
            ),
            AttemptErrorKind::RateLimited
        )
    );
}

// Fetch.

#[test]
fn a_discussion_ranks_top_level_comments_by_firebase_and_replies_by_time() {
    let tree = decode_tree(&stored_body("item-tree.json")).expect("a discussion tree");
    let live = decode_json::<FirebaseItem>(&stored_body("firebase-item.json")).expect("an item");

    let (item, markdown) = thread_item(hn(8863), HnItemType::Story, tree, Some(&live), None, None);

    assert_eq!(
        (to_json(&item), markdown),
        (
            json!({
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
                "order": "ranked_top_level"
            }),
            [
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
            .concat()
        )
    );
}

#[test]
fn a_thread_reads_the_algolia_tree_and_then_the_firebase_rank() {
    let (outcome, requests) = fetch(
        8863,
        ContentDepth::Thread,
        vec![ok_json(&discussion()), ok_json(&story_item(&[12, 99, 10]))],
    );

    let outcome = outcome.expect("a discussion");
    assert_eq!(
        (
            paths(&requests),
            native(&outcome.content_source).starts_with(HEADER),
            to_json(&outcome.item)["order"].clone()
        ),
        (
            vec![
                "/api/v1/items/8863".to_owned(),
                "/v0/item/8863.json".to_owned()
            ],
            true,
            json!("ranked_top_level")
        )
    );
}

#[test]
fn without_the_firebase_rank_every_comment_is_in_time_order() {
    let (outcome, _) = fetch(
        8863,
        ContentDepth::Thread,
        vec![ok_json(&discussion()), Response::json(503, "")],
    );

    let outcome = outcome.expect("a discussion");
    assert_eq!(
        (native(&outcome.content_source), outcome.diagnostic.as_deref()),
        (
            [
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
            .concat()
            .as_str(),
            Some(
                "HN Firebase API unavailable (HN Firebase API returned HTTP 503 (Service Unavailable)); comments are in chronological order and counts may lag"
            )
        )
    );
}

#[test]
fn a_comment_thread_names_its_story_from_firebase() {
    let root = with(
        node(
            8952,
            "brett",
            "Root <i>comment</i>",
            &[node(9000, "erin", "reply", &[])],
        ),
        &json!({"created_at_i": 1_175_714_252}),
    );

    let (outcome, requests) = fetch(
        8952,
        ContentDepth::Thread,
        vec![
            ok_json(&root),
            ok_json(&firebase_comment(8952, "brett", 8863, "Root", &[9000])),
            ok_json(&story_item(&[8952])),
        ],
    );

    let outcome = outcome.expect("a discussion");
    assert_eq!(
        (paths(&requests), native(&outcome.content_source)),
        (
            vec![
                "/api/v1/items/8952".to_owned(),
                "/v0/item/8952.json".to_owned(),
                "/v0/item/8863.json".to_owned()
            ],
            [
                "# Comment by brett\n\n- Ref: `hn:8952` <https://news.ycombinator.com/item?id=8952>\n- Story: My YC app: Dropbox - Throw away your USB drive `hn:8863`\n- Author: brett\n- Published: 2007-04-04T19:17:32Z\n\nRoot *comment*\n\n## Comments\n",
                "\n- **erin** · 2007-04-04T21:46:40Z · `hn:9000`\n\n  reply\n",
            ]
            .concat()
            .as_str()
        )
    );
}

#[test]
fn metadata_reads_the_indexed_item_with_live_counts_and_no_discussion() {
    let (outcome, requests) = fetch(
        8863,
        ContentDepth::Metadata,
        vec![
            ok_json(&search_page(&[story_hit()], 0, 1)),
            ok_json(&story_item(&[])),
        ],
    );

    let outcome = outcome.expect("an item");
    let item = to_json(&outcome.item);
    assert_eq!(
        (
            paths(&requests),
            query(&requests[0]),
            [&item["depth"], &item["points"], &item["comments"]],
            outcome.content_source,
        ),
        (
            vec!["/api/v1/search".to_owned(), "/v0/item/8863.json".to_owned()],
            pairs(&[("filters", "objectID:8863"), ("hitsPerPage", "1")]),
            [&json!("metadata"), &json!(104), &json!(71)],
            FullTextSource::Urls(Vec::new()),
        )
    );
}

#[test]
fn an_item_too_new_for_the_search_index_is_read_from_firebase() {
    let (outcome, _) = fetch(
        9999,
        ContentDepth::Metadata,
        vec![
            ok_json(&search_page(&[], 0, 0)),
            ok_json(&firebase_comment(
                9999,
                "erin",
                8863,
                "Fresh &amp; new",
                &[],
            )),
        ],
    );

    let mut item = to_json(&outcome.expect("an item").item);
    item.as_object_mut().expect("object").retain(|key, _| {
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
        item,
        json!({
            "title": null,
            "item_type": "comment",
            "text": "Fresh & new",
            "story_ref": null,
            "story_title": null,
            "parent_ref": "hn:8863"
        })
    );
}

#[test]
fn missing_deleted_and_unindexed_items_are_parameter_errors() {
    let missing = fetch(
        1,
        ContentDepth::Metadata,
        vec![
            ok_json(&search_page(&[], 0, 0)),
            Response::json(200, &stored_body("firebase-null.json")),
        ],
    );
    let deleted = fetch(
        8863,
        ContentDepth::Metadata,
        vec![
            ok_json(&search_page(&[story_hit()], 0, 1)),
            ok_json(&with(story_item(&[]), &json!({"deleted": true}))),
        ],
    );
    let unindexed = fetch(
        8863,
        ContentDepth::Thread,
        vec![Response::json(404, &stored_body("item-not-found.json"))],
    );

    assert_eq!(
        [missing, deleted, unindexed].map(|(outcome, _)| failure(outcome)),
        [
            (
                AttemptErrorKind::Parameter,
                "HN item not found: hn:1".to_owned()
            ),
            (
                AttemptErrorKind::Parameter,
                "HN item hn:8863 is deleted or dead".to_owned()
            ),
            (
                AttemptErrorKind::Parameter,
                "HN item not found: hn:8863".to_owned()
            ),
        ]
    );
}

#[test]
fn the_full_text_of_a_link_story_is_the_page_it_links_to() {
    let (outcome, _) = fetch(
        8863,
        ContentDepth::FullText,
        vec![
            ok_json(&search_page(&[story_hit()], 0, 1)),
            ok_json(&story_item(&[])),
        ],
    );

    assert_eq!(
        outcome
            .map(|outcome| outcome.content_source)
            .map_err(|error| error.message),
        Ok(FullTextSource::Urls(vec![
            "http://www.getdropbox.com/u/2/screencast.html".to_owned()
        ]))
    );
}

#[test]
fn the_full_text_of_a_text_post_is_its_own_title_and_body() {
    let ask = with(
        story_hit(),
        &json!({"title": "Ask HN: How do you back up?", "url": null, "story_text": "I use <i>rsync</i>."}),
    );

    let (outcome, _) = fetch(
        8863,
        ContentDepth::FullText,
        vec![
            ok_json(&search_page(&[ask], 0, 1)),
            ok_json(&story_item(&[])),
        ],
    );

    assert_eq!(
        outcome
            .map(|outcome| outcome.content_source)
            .map_err(|error| error.message),
        Ok(FullTextSource::Native(
            "# Ask HN: How do you back up?\n\nI use *rsync*.".to_owned()
        ))
    );
}

#[test]
fn a_comment_has_no_full_text() {
    let (outcome, _) = fetch(
        8952,
        ContentDepth::FullText,
        vec![
            ok_json(&search_page(&[comment_hit()], 0, 1)),
            ok_json(&firebase_comment(8952, "brett", 8863, "x", &[])),
        ],
    );

    assert_eq!(
        failure(outcome),
        (
            AttemptErrorKind::Parameter,
            "hn:8952 is a comment; full_text applies to stories, polls, and jobs; use --depth thread"
                .to_owned()
        )
    );
}

#[test]
fn only_a_thread_metadata_or_full_text_can_be_fetched() {
    let supported = [
        ContentDepth::Thread,
        ContentDepth::Metadata,
        ContentDepth::FullText,
        ContentDepth::Snippet,
        ContentDepth::Abstract,
    ]
    .map(|depth| fetch_support(&PlatformFetchRequest::new(PlatformRef::Hn(hn(8863)), depth)));

    assert_eq!(
        supported,
        [
            Ok(()),
            Ok(()),
            Ok(()),
            Err("hn_api cannot fetch at depth `snippet`".to_owned()),
            Err("hn_api cannot fetch at depth `abstract`".to_owned()),
        ]
    );
}

#[test]
fn both_endpoints_wait_on_one_access_window() {
    let fixture = Fixture::start_sequence(vec![ok_json(&search_page(&[story_hit()], 0, 1))]);
    // The Firebase window opens a full 500 ms interval after the Algolia request, so any budget
    // below the interval makes its reservation fail at once and no test waits. The budget sits
    // just below the interval to leave the Algolia request the most time on a loaded machine.
    let route = {
        let _paced = paced_limiters();
        route(&fixture.url, Duration::from_millis(490))
    };
    let request = PlatformFetchRequest::new(PlatformRef::Hn(hn(8863)), ContentDepth::Metadata);

    let outcome = block_on(route.fetch(&request)).expect("the Algolia item stands");
    let attempts = outcome
        .attempts
        .iter()
        .map(|attempt| attempt.error_kind)
        .collect::<Vec<_>>();

    assert_eq!(
        (
            paths(&fixture.finish_all()),
            attempts,
            to_json(&outcome.item)["points"].clone()
        ),
        (
            vec!["/api/v1/search".to_owned()],
            vec![None, Some(AttemptErrorKind::Timeout)],
            json!(100)
        )
    );
}

// Stories.

#[test]
fn the_front_page_lists_items_in_rank_order_and_reads_unindexed_ones_from_firebase() {
    let too_new = with(
        story_item(&[]),
        &json!({"id": 20, "by": "erin", "score": 5, "title": "Too new", "descendants": 0}),
    );

    let (outcome, requests) = stories(
        HnStoriesList::Top,
        3,
        vec![
            ok_json(&stored_json("firebase-stories.json")),
            ok_json(&search_page(&[hit(10, "Ten"), hit(30, "Thirty")], 0, 1)),
            ok_json(&too_new),
        ],
    );

    assert_eq!(
        (
            paths(&requests),
            query(&requests[1]),
            outcome
                .map(|outcome| ranked(&outcome))
                .map_err(|error| error.message),
        ),
        (
            vec![
                "/v0/topstories.json".to_owned(),
                "/api/v1/search".to_owned(),
                "/v0/item/20.json".to_owned()
            ],
            pairs(&[
                ("filters", "objectID:30 OR objectID:10 OR objectID:20"),
                ("hitsPerPage", "3"),
            ]),
            Ok(vec![
                (1, "hn:30".to_owned(), Some("Thirty".to_owned())),
                (2, "hn:10".to_owned(), Some("Ten".to_owned())),
                (3, "hn:20".to_owned(), Some("Too new".to_owned())),
            ]),
        )
    );
}

#[test]
fn deleted_and_dead_items_are_skipped_with_one_diagnostic() {
    let (outcome, requests) = stories(
        HnStoriesList::Ask,
        30,
        vec![
            ok_json(&story_list(&[1, 2, 3])),
            ok_json(&search_page(&[hit(1, "One")], 0, 1)),
            ok_json(&with(story_item(&[]), &json!({"id": 2, "deleted": true}))),
            ok_json(&with(story_item(&[]), &json!({"id": 3, "dead": true}))),
        ],
    );

    let outcome = outcome.expect("a listing");
    assert_eq!(
        (
            path(&requests[0]),
            ranked(&outcome),
            outcome.diagnostic.as_deref()
        ),
        (
            "/v0/askstories.json".to_owned(),
            vec![(1, "hn:1".to_owned(), Some("One".to_owned()))],
            Some("skipped 2 deleted, dead, or missing items of the HN ask list")
        )
    );
}

#[test]
fn a_failed_item_read_skips_that_item_and_keeps_the_rest() {
    let (outcome, _) = stories(
        HnStoriesList::Top,
        30,
        vec![
            ok_json(&story_list(&[1, 2])),
            ok_json(&search_page(&[hit(1, "One")], 0, 1)),
            Response::json(503, ""),
        ],
    );

    let outcome = outcome.expect("a listing");
    assert_eq!(
        (ranked(&outcome), outcome.diagnostic.as_deref()),
        (
            vec![(1, "hn:1".to_owned(), Some("One".to_owned()))],
            Some("could not read 1 item of the HN top list from the HN Firebase API")
        )
    );
}

#[test]
fn an_empty_list_is_an_empty_success_and_a_failed_or_missing_list_fails() {
    let (empty, empty_requests) = stories(HnStoriesList::Job, 30, vec![ok_json(&story_list(&[]))]);
    let (failed, _) = stories(HnStoriesList::New, 30, vec![Response::json(503, "")]);
    let (missing, _) = stories(
        HnStoriesList::Best,
        30,
        vec![Response::json(200, &stored_body("firebase-null.json"))],
    );

    assert_eq!(
        (
            empty
                .map(|outcome| outcome.items.len())
                .map_err(|error| error.message),
            paths(&empty_requests),
            failure(failed).0,
            failure(missing),
        ),
        (
            Ok(0),
            vec!["/v0/jobstories.json".to_owned()],
            AttemptErrorKind::Network,
            (
                AttemptErrorKind::Runtime,
                "HN Firebase API returned no best story list".to_owned()
            ),
        )
    );
}
