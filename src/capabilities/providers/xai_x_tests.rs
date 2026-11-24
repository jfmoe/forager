use std::time::Duration;

use serde_json::{Value, json};

use super::super::Xai;
use super::{decode_answer, decode_search, decode_thread, fetch_support};
use crate::config::XaiRuntimeConfig;
use crate::fixture_server::{Fixture, Response, block_on, path};
use crate::net::{AttemptFailure, RetryPolicy, build_client};
use crate::providers::RouteContext;
use crate::providers::test_projection::{failure, to_json};
use crate::types::{
    AttemptErrorKind, AttemptTarget, ContentDepth, Deadline, PlatformFetchOutcome,
    PlatformFetchRequest, PlatformItem, PlatformRef, PlatformSearchOptions, PlatformSearchOutcome,
    PlatformSearchRequest, ProviderError, XAskOutcome, XAskRequest, XRef, XSearchOptions,
};

use crate::main_search_samples::completed_events;
use crate::x_samples::stored;

const KEY: &str = "test-xai-key-a";
const SECOND_KEY: &str = "test-xai-key-b";
const ROOT: &str = "2110126570233094527";

/// The route against the fixture at `url`, built as its factory does, with `keys` in pool order
/// and one attempt per request. Main search enables web search too, which no X request may send.
fn route(url: &str, keys: &[&str]) -> Xai {
    Xai::build(
        XaiRuntimeConfig {
            url: url.to_owned(),
            keys: keys.iter().map(|key| (*key).into()).collect(),
            model: "grok-test".to_owned(),
            tools: vec!["web_search".to_owned(), "x_search".to_owned()],
        },
        RouteContext {
            client: build_client(true).expect("build client"),
            retry_policy: RetryPolicy::new(1, 1.0, Duration::ZERO),
            deadline: Deadline::new(Duration::from_secs(10)),
        },
    )
}

fn completed(response: &Value) -> Response {
    Response::sse(200, &completed_events(response))
}

/// Streams one terminal event of `kind` with `body` merged into it.
fn terminal(kind: &str, body: &Value) -> Response {
    let mut event = body.clone();
    event["type"] = json!(kind);
    Response::sse(200, &format!("event: {kind}\ndata: {event}\n\n"))
}

/// The stored search response with its structured output replaced by `posts` and its sources
/// by `cited` status URLs.
fn search_with(posts: &Value, cited: &[&str]) -> Value {
    let mut response = stored("x-search.json");
    let message = response["output"]
        .as_array_mut()
        .expect("output")
        .last_mut()
        .expect("message");
    message["content"][0]["text"] = json!(json!({ "posts": posts }).to_string());
    message["content"][0]["annotations"] = cited
        .iter()
        .map(|url| json!({"type": "url_citation", "url": url, "start_index": 0, "end_index": 0, "title": url}))
        .collect();
    response
}

/// The stored thread response with its structured output replaced by `posts`.
fn thread_with(posts: &Value) -> Value {
    let mut response = stored("x-thread.json");
    response["output"][1]["content"][0]["text"] = json!(json!({ "posts": posts }).to_string());
    response
}

fn post(id: &str) -> Value {
    json!({"url": format!("https://x.com/a/status/{id}"), "author_handle": "a", "text": id})
}

fn x_ref(id: &str) -> XRef {
    XRef::from_post_id(id).expect("a post ID")
}

fn search_request(query: &str, limit: u16, options: XSearchOptions) -> PlatformSearchRequest {
    PlatformSearchRequest {
        query: query.to_owned(),
        limit,
        options: PlatformSearchOptions::X(options),
        page: None,
    }
}

fn search(
    keys: &[&str],
    request: &PlatformSearchRequest,
    answers: Vec<Response>,
) -> (Result<PlatformSearchOutcome, ProviderError>, Vec<String>) {
    let fixture = Fixture::start_sequence(answers);
    let outcome = block_on(route(&fixture.url, keys).search_posts(request));
    (outcome, fixture.finish_all())
}

fn fetch(
    depth: ContentDepth,
    answers: Vec<Response>,
) -> (Result<PlatformFetchOutcome, ProviderError>, Vec<String>) {
    let fixture = Fixture::start_sequence(answers);
    let request = PlatformFetchRequest::new(PlatformRef::X(x_ref(ROOT)), depth);
    let outcome = block_on(route(&fixture.url, &[KEY]).fetch_post(&request));
    (outcome, fixture.finish_all())
}

fn ask(
    request: &XAskRequest,
    answers: Vec<Response>,
) -> (Result<XAskOutcome, ProviderError>, Vec<String>) {
    let fixture = Fixture::start_sequence(answers);
    let outcome = block_on(route(&fixture.url, &[KEY]).ask(request));
    (outcome, fixture.finish_all())
}

/// The JSON body of a request the fixture server recorded.
fn body(request: &str) -> Value {
    let (_, body) = request.split_once("\r\n\r\n").expect("request body");
    serde_json::from_str(body).expect("request JSON")
}

fn authorization(request: &str) -> Option<String> {
    request.lines().find_map(|line| {
        let (name, value) = line.split_once(": ")?;
        name.eq_ignore_ascii_case("authorization")
            .then(|| value.to_owned())
    })
}

fn day(value: &str) -> chrono::NaiveDate {
    value.parse().expect("a calendar day")
}

fn handles(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn refs(items: &[PlatformItem]) -> Vec<String> {
    items
        .iter()
        .map(|item| item.reference.to_string())
        .collect()
}

fn roles(item: &PlatformItem) -> Vec<Value> {
    to_json(item)["thread"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|post| post["role"].clone())
        .collect()
}

fn attempt_failure<T>(result: Result<T, AttemptFailure>) -> (AttemptErrorKind, String) {
    match result {
        Ok(_) => panic!("expected a failure"),
        Err(failure) => (failure.kind, failure.message),
    }
}

// Search decoding.

#[test]
fn verified_posts_become_snippet_items_dated_by_their_id() {
    let (items, dropped) =
        decode_search(&stored("x-search.json"), &XSearchOptions::default(), 10).expect("items");

    assert_eq!(
        (to_json(&items[0]), refs(&items), dropped),
        (
            json!({
                "ref": "x:2106088717546398899",
                "url": "https://x.com/i/status/2106088717546398899",
                "depth": "snippet",
                "title": null,
                "authors": ["example_dev1"],
                "published": "2026-10-02T18:27:34Z",
                "author_handle": "example_dev1",
                "text": "Rust developers (of the language, not just users of it) are working hard to make the Edition 2027 a great release. Nice. But I wonder which of the planned changes will land first - the list keeps growing and the release date is not far off... Just curious, I guess..."
            }),
            vec![
                "x:2106088717546398899".to_owned(),
                "x:2110286468622476580".to_owned()
            ],
            None
        )
    );
}

#[test]
fn posts_dated_outside_the_requested_days_are_dropped() {
    // 2106088717546398899 dates to 2026-10-02 and 2110286468622476580 to 2026-10-14.
    let options = XSearchOptions::new(
        Vec::new(),
        Vec::new(),
        Some(day("2026-10-03")),
        Some(day("2026-10-14")),
    );

    let (items, dropped) = decode_search(&stored("x-search.json"), &options, 10).expect("items");

    assert_eq!(
        (refs(&items), dropped),
        (
            vec!["x:2110286468622476580".to_owned()],
            Some("xai dropped 1 of 2 X posts it could not verify: https://x.com/example_dev1/status/2106088717546398899 (published outside the requested dates)".to_owned())
        )
    );
}

#[test]
fn repeated_posts_appear_once_and_the_list_stops_at_the_limit() {
    let response = search_with(
        &json!([
            post("2106088717546398899"),
            post("2106088717546398899"),
            post("2110286468622476580")
        ]),
        &[
            "https://x.com/i/status/2106088717546398899",
            "https://x.com/i/status/2110286468622476580",
        ],
    );

    let decoded = decode_search(&response, &XSearchOptions::default(), 1).expect("items");
    let unlimited = decode_search(&response, &XSearchOptions::default(), 10).expect("items");

    assert_eq!(
        (refs(&decoded.0), decoded.1, refs(&unlimited.0)),
        (
            vec!["x:2106088717546398899".to_owned()],
            None,
            vec![
                "x:2106088717546398899".to_owned(),
                "x:2110286468622476580".to_owned()
            ]
        )
    );
}

#[test]
fn a_list_whose_every_post_is_unverified_is_a_runtime_failure() {
    let response = search_with(
        &json!([{"url": "https://x.com/someone/status/2110000000000000000", "author_handle": "someone", "text": "invented"}]),
        &["https://x.com/i/status/2106088717546398899"],
    );

    assert_eq!(
        attempt_failure(decode_search(&response, &XSearchOptions::default(), 10)),
        (
            AttemptErrorKind::Runtime,
            "xAI returned 1 X posts but none could be verified; xai dropped 1 of 1 X posts it could not verify: https://x.com/someone/status/2110000000000000000 (not among the posts the search read)".to_owned()
        )
    );
}

#[test]
fn an_empty_list_is_a_legitimate_empty_result() {
    let response = search_with(&json!([]), &[]);

    let (items, dropped) =
        decode_search(&response, &XSearchOptions::default(), 10).expect("an empty result");

    assert_eq!((items.len(), dropped), (0, None));
}

#[test]
fn output_that_is_not_the_requested_json_is_a_runtime_failure() {
    let mut response = stored("x-search.json");
    response["output"][2]["content"][0]["text"] = json!("Here are some posts: none.");

    let (kind, message) = attempt_failure(decode_search(&response, &XSearchOptions::default(), 10));

    assert_eq!(
        (
            kind,
            message.starts_with("xAI returned X posts that are not the requested JSON: ")
        ),
        (AttemptErrorKind::Runtime, true),
        "{message}"
    );
}

// Fetch decoding.

#[test]
fn a_thread_returns_the_root_with_every_post_in_order() {
    let (item, skipped) =
        decode_thread(&stored("x-thread.json"), x_ref(ROOT), ContentDepth::Thread)
            .expect("a thread");
    let fetched = to_json(&item);

    assert_eq!(
        (
            &fetched["ref"],
            &fetched["url"],
            &fetched["depth"],
            &fetched["authors"],
            &fetched["published"],
            skipped,
        ),
        (
            &json!("x:2110126570233094527"),
            &json!("https://x.com/i/status/2110126570233094527"),
            &json!("thread"),
            &json!(["SpaceX"]),
            &json!("2026-10-13T21:52:33Z"),
            None,
        )
    );
    assert_eq!(
        fetched["thread"][1],
        json!({
            "ref": "x:2110127170868446477",
            "url": "https://x.com/i/status/2110127170868446477",
            "author_handle": "example_fan1",
            "published": "2026-10-13T21:54:56Z",
            "text": "Wonderful photographs.",
            "role": "reply"
        })
    );
    assert_eq!(
        roles(&item),
        [
            json!("root"),
            json!("reply"),
            json!("reply"),
            json!("reply")
        ]
    );
}

#[test]
fn metadata_depth_returns_the_post_without_its_thread() {
    let (item, _) = decode_thread(
        &stored("x-thread.json"),
        x_ref(ROOT),
        ContentDepth::Metadata,
    )
    .expect("a post");
    let fetched = to_json(&item);

    assert_eq!(
        (
            &fetched["depth"],
            fetched.get("thread"),
            fetched["text"]
                .as_str()
                .map(|text| text.starts_with("The first Starship"))
        ),
        (&json!("metadata"), None, Some(true))
    );
}

#[test]
fn a_reply_is_fetched_although_its_thread_fetch_cites_only_the_parent() {
    let (item, _) = decode_thread(
        &stored("x-reply-thread.json"),
        x_ref("2098634733248720992"),
        ContentDepth::Thread,
    )
    .expect("a thread");

    assert_eq!(
        (item.reference.to_string(), roles(&item)),
        (
            "x:2098634733248720992".to_owned(),
            vec![json!("root"), json!("parent"), json!("reply")]
        )
    );
}

#[test]
fn a_root_cited_without_a_thread_fetch_is_fetched() {
    let mut response = stored("x-thread.json");
    response["output"].as_array_mut().expect("output").remove(0);

    let fetched = decode_thread(&response, x_ref(ROOT), ContentDepth::Thread);

    assert_eq!(
        fetched.map(|(item, _)| item.reference.to_string()).ok(),
        Some("x:2110126570233094527".to_owned())
    );
}

#[test]
fn a_root_other_than_the_requested_post_is_a_runtime_failure() {
    let response = thread_with(&json!([
        {"url": "https://x.com/SpaceX/status/2110126570233094000", "author_handle": "SpaceX", "text": "other", "role": "root"}
    ]));

    assert_eq!(
        attempt_failure(decode_thread(&response, x_ref(ROOT), ContentDepth::Thread)),
        (
            AttemptErrorKind::Runtime,
            "xAI returned https://x.com/SpaceX/status/2110126570233094000 as the root post instead of x:2110126570233094527".to_owned()
        )
    );
}

#[test]
fn a_root_neither_thread_fetched_nor_cited_is_a_runtime_failure() {
    let mut response = stored("x-thread.json");
    response["output"].as_array_mut().expect("output").remove(0);
    response["output"][0]["content"][0]["annotations"] = json!([]);

    assert_eq!(
        attempt_failure(decode_thread(&response, x_ref(ROOT), ContentDepth::Thread)),
        (
            AttemptErrorKind::Runtime,
            "xAI neither fetched nor cited x:2110126570233094527 among the posts it read"
                .to_owned()
        )
    );
}

#[test]
fn a_thread_fetch_of_another_post_does_not_verify_the_root() {
    let mut response = stored("x-thread.json");
    response["output"][0]["input"] = json!(r#"{"post_id":"2110126570233094000"}"#);
    response["output"][1]["content"][0]["annotations"] = json!([]);

    assert_eq!(
        attempt_failure(decode_thread(&response, x_ref(ROOT), ContentDepth::Thread)).0,
        AttemptErrorKind::Runtime
    );
}

#[test]
fn an_unfinished_thread_fetch_does_not_verify_the_root() {
    let mut response = stored("x-thread.json");
    response["output"][0]["status"] = json!("failed");
    response["output"][1]["content"][0]["annotations"] = json!([]);

    assert_eq!(
        attempt_failure(decode_thread(&response, x_ref(ROOT), ContentDepth::Thread)).0,
        AttemptErrorKind::Runtime
    );
}

#[test]
fn a_thread_without_exactly_one_root_is_a_runtime_failure() {
    let reply = json!({"url": "https://x.com/a/status/2110127170868446477", "author_handle": "a", "text": "hi", "role": "reply"});

    assert_eq!(
        attempt_failure(decode_thread(
            &thread_with(&json!([reply])),
            x_ref(ROOT),
            ContentDepth::Thread
        )),
        (
            AttemptErrorKind::Runtime,
            "xAI returned 0 root posts for x:2110126570233094527; expected exactly one".to_owned()
        )
    );
}

#[test]
fn a_post_the_model_cannot_see_is_an_attempt_parameter_error() {
    assert_eq!(
        attempt_failure(decode_thread(
            &thread_with(&json!([])),
            x_ref(ROOT),
            ContentDepth::Thread
        )),
        (
            AttemptErrorKind::Parameter,
            "X post x:2110126570233094527 was not found or is not visible".to_owned()
        )
    );
}

// Ask decoding.

#[test]
fn the_answer_keeps_its_citations_and_cited_posts_become_refs() {
    let (answer, sources) = decode_answer(
        &stored("x-ask.json"),
        &route("http://127.0.0.1:9", &[KEY]).credentials,
    )
    .expect("an answer");

    assert_eq!(
        (
            answer.contains("[[1]](https://x.com/SpaceX/status/2106089751837278533)"),
            to_json(&sources)
        ),
        (
            true,
            json!([
                {"ref": "x:2106089751837278533", "url": "https://x.com/i/status/2106089751837278533", "title": ""},
                {"ref": "x:2106090316747207047", "url": "https://x.com/i/status/2106090316747207047", "title": ""},
                {"ref": "x:2110126570233094527", "url": "https://x.com/i/status/2110126570233094527", "title": ""}
            ])
        ),
        "{answer}"
    );
}

#[test]
fn a_cited_profile_keeps_its_url_without_a_ref() {
    let mut response = stored("x-ask.json");
    response["output"][2]["content"][0]["annotations"] = json!([
        {"type": "url_citation", "url": "https://x.com/i/user/34743251", "start_index": 0, "end_index": 10, "title": "SpaceX"}
    ]);

    let (_, sources) = decode_answer(&response, &route("http://127.0.0.1:9", &[KEY]).credentials)
        .expect("an answer");

    assert_eq!(
        to_json(&sources[0]),
        json!({"ref": null, "url": "https://x.com/i/user/34743251", "title": "SpaceX"})
    );
}

// Requests.

#[test]
fn a_search_sends_only_the_x_search_tool_with_its_filters_and_strict_structured_output() {
    let options = XSearchOptions::new(
        handles(&["@rustlang", "RustLang", "nikomatsakis"]),
        Vec::new(),
        Some(day("2026-09-01")),
        Some(day("2026-10-15")),
    );
    let request = search_request("Rust 2027 edition", 5, options);

    let (outcome, requests) = search(&[KEY], &request, vec![completed(&stored("x-search.json"))]);
    let sent = body(&requests[0]);

    assert_eq!(
        (
            outcome.is_ok(),
            path(&requests[0]),
            &sent["model"],
            &sent["stream"],
            &sent["tools"],
            &sent["text"]["format"]["type"],
            &sent["text"]["format"]["name"],
            &sent["text"]["format"]["strict"],
        ),
        (
            true,
            "/responses".to_owned(),
            &json!("grok-test"),
            &json!(true),
            &json!([{
                "type": "x_search",
                "allowed_x_handles": ["rustlang", "nikomatsakis"],
                "from_date": "2026-09-01",
                "to_date": "2026-10-16"
            }]),
            &json!("json_schema"),
            &json!("x_posts"),
            &json!(true),
        )
    );
}

#[test]
fn posts_the_search_did_not_read_are_dropped_with_one_diagnostic() {
    let response = search_with(
        &json!([
            {"url": "https://x.com/example_dev1/status/2106088717546398899", "author_handle": "example_dev1", "text": "read"},
            {"url": "https://x.com/someone/status/2110000000000000000", "author_handle": "someone", "text": "invented"},
            {"url": "https://example.com/post", "author_handle": "someone", "text": "elsewhere"}
        ]),
        &[
            "https://x.com/i/status/2106088717546398899",
            "https://x.com/i/user/1112661938606744997",
        ],
    );

    let (outcome, _) = search(
        &[KEY],
        &search_request("Rust 2027 edition", 10, XSearchOptions::default()),
        vec![completed(&response)],
    );
    let outcome = outcome.expect("a page");

    assert_eq!(
        (refs(&outcome.items), outcome.diagnostic),
        (
            vec!["x:2106088717546398899".to_owned()],
            Some("xai dropped 2 of 3 X posts it could not verify: https://x.com/someone/status/2110000000000000000 (not among the posts the search read); https://example.com/post (not an X post URL)".to_owned())
        )
    );
}

#[test]
fn a_fetch_sends_the_x_search_tool_alone_and_asks_for_each_posts_role() {
    let (outcome, requests) = fetch(
        ContentDepth::Thread,
        vec![completed(&stored("x-thread.json"))],
    );
    let sent = body(&requests[0]);

    assert_eq!(
        (
            outcome.is_ok(),
            path(&requests[0]),
            &sent["tools"],
            &sent["text"]["format"]["name"],
            &sent["text"]["format"]["schema"]["properties"]["posts"]["items"]["properties"]["role"]
                ["enum"],
            &sent["input"][0]["content"],
        ),
        (
            true,
            "/responses".to_owned(),
            &json!([{"type": "x_search"}]),
            &json!("x_thread"),
            &json!(["parent", "root", "reply", "quoted"]),
            &json!(
                "The requested post: ID 2110126570233094527, https://x.com/i/status/2110126570233094527"
            ),
        )
    );
}

#[test]
fn a_fetch_requires_the_model_to_call_its_tool() {
    let (outcome, requests) = fetch(
        ContentDepth::Metadata,
        vec![completed(&stored("x-thread.json"))],
    );

    assert_eq!(
        (outcome.is_ok(), &body(&requests[0])["tool_choice"]),
        (true, &json!("required"))
    );
}

#[test]
fn a_fetch_at_a_depth_other_than_thread_or_metadata_is_refused_before_any_request() {
    for depth in [
        ContentDepth::Snippet,
        ContentDepth::Abstract,
        ContentDepth::FullText,
    ] {
        let request = PlatformFetchRequest::new(PlatformRef::X(x_ref(ROOT)), depth);
        let message = format!(
            "xai cannot fetch at depth `{}`; X posts are read at `thread` or `metadata` depth",
            depth.as_str()
        );

        let (outcome, requests) = fetch(depth, Vec::new());

        assert_eq!(
            (fetch_support(&request), failure(outcome), requests.len()),
            (
                Err(message.clone()),
                (AttemptErrorKind::Parameter, message),
                0
            )
        );
    }
}

#[test]
fn an_ask_sends_the_filtered_x_search_tool_without_structured_output() {
    let request = XAskRequest {
        query: "What did SpaceX say about Starship in early October?".to_owned(),
        options: XSearchOptions::new(
            handles(&["SpaceX"]),
            Vec::new(),
            Some(day("2026-10-01")),
            Some(day("2026-10-14")),
        ),
    };

    let (outcome, requests) = ask(&request, vec![completed(&stored("x-ask.json"))]);
    let sent = body(&requests[0]);
    let input = sent["input"][0]["content"].as_str().unwrap_or_default();

    assert_eq!(
        (
            outcome.is_ok(),
            path(&requests[0]),
            &sent["tools"],
            sent.get("text"),
            input.starts_with("[Current Time Context]"),
            input.ends_with("\n\nWhat did SpaceX say about Starship in early October?"),
        ),
        (
            true,
            "/responses".to_owned(),
            &json!([{
                "type": "x_search",
                "allowed_x_handles": ["SpaceX"],
                "from_date": "2026-10-01",
                "to_date": "2026-10-15"
            }]),
            None,
            true,
            true,
        ),
        "{sent}"
    );
}

// Attempts and keys.

#[test]
fn each_x_operation_records_its_attempt_under_the_x_platform() {
    let (searched, _) = search(
        &[KEY],
        &search_request("rust", 10, XSearchOptions::default()),
        vec![completed(&stored("x-search.json"))],
    );
    let (fetched, _) = fetch(
        ContentDepth::Thread,
        vec![completed(&stored("x-thread.json"))],
    );
    let (asked, _) = ask(
        &XAskRequest {
            query: "Starship news".to_owned(),
            options: XSearchOptions::default(),
        },
        vec![completed(&stored("x-ask.json"))],
    );
    let attempts = [
        searched.expect("a page").attempts,
        fetched.expect("a post").attempts,
        asked.expect("an answer").attempts,
    ]
    .map(|attempts| {
        attempts
            .iter()
            .map(|attempt| {
                (
                    attempt.provider,
                    attempt.target,
                    attempt.error_kind,
                    attempt.model.clone(),
                    attempt.transport,
                )
            })
            .collect::<Vec<_>>()
    });

    let expected = |operation| {
        vec![(
            "xai",
            AttemptTarget::platform("x", operation),
            None,
            Some("grok-test".to_owned()),
            Some("sse"),
        )]
    };
    assert_eq!(
        attempts,
        [expected("search"), expected("fetch"), expected("ask")]
    );
}

#[test]
fn a_rate_limited_key_rotates_to_the_next_key() {
    let keys = [KEY, SECOND_KEY];

    let (outcome, requests) = search(
        &keys,
        &search_request("rust", 10, XSearchOptions::default()),
        vec![
            terminal(
                "response.failed",
                &json!({"response": {"error": {"code": "rate_limit_exceeded", "message": "slow down"}}}),
            ),
            completed(&stored("x-search.json")),
        ],
    );
    let attempts = outcome.expect("the next key succeeds").attempts;
    // The credential cursor persists across requests, so the first key may be either.
    let first = attempts[0].credential_index;
    let second = (first + 1) % keys.len();

    assert_eq!(
        (
            attempts
                .iter()
                .map(|attempt| (
                    attempt.error_kind,
                    attempt.credential_index,
                    attempt.rotation_count
                ))
                .collect::<Vec<_>>(),
            requests
                .iter()
                .map(|request| authorization(request))
                .collect::<Vec<_>>()
        ),
        (
            vec![
                (Some(AttemptErrorKind::RateLimited), first, 0),
                (None, second, 1),
            ],
            vec![
                Some(format!("Bearer {}", keys[first])),
                Some(format!("Bearer {}", keys[second]))
            ]
        )
    );
}

#[test]
fn a_rejected_request_is_a_parameter_failure() {
    let (outcome, requests) = search(
        &[KEY],
        &search_request("rust", 10, XSearchOptions::default()),
        vec![terminal(
            "response.failed",
            &json!({"response": {"error": {"code": "invalid_request_error", "message": "bad tool"}}}),
        )],
    );

    assert_eq!(
        (failure(outcome), requests.len()),
        (
            (
                AttemptErrorKind::Parameter,
                "xAI Responses ended with response.failed: bad tool".to_owned()
            ),
            1
        )
    );
}
