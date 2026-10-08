//! The `xiaohongshu_browser` search against a fake `opencli` executable that answers envelopes
//! shaped like the live page facts of 2026-10-08. These tests prove the transport, the page-fact
//! classification, the condition checks, and the decoding; only the live smoke C27 exercises the
//! JavaScript adapter.

mod support;

use std::process::Output;

use serde_json::{Value, json};

use support::RunEnvironment;
use support::opencli::{FakeOpenCli, XHS_CONTRACT};

const SESSION_FLAGS: [&str; 8] = [
    "-f",
    "json",
    "--window",
    "background",
    "--site-session",
    "ephemeral",
    "--keep-tab",
    "false",
];
const RESULTS_URL: &str =
    "https://www.xiaohongshu.com/search_result?keyword=%E5%92%96%E5%95%A1&source=web_explore_feed";
const ROOT_SEARCH_ID: &str = "2fhpxs5cks5vx6nvw1ar4";
const FILTERED_SEARCH_ID: &str = "2fhpxs5cks5vx6nvw1ar4@2fhpxsfv1qadn0lsg9txa";

fn payload(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "parse JSON stdout: {error}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The page facts of a results page that shows the search for 咖啡.
fn results_page() -> Value {
    json!({
        "url": RESULTS_URL,
        "title": "咖啡 - 小红书搜索",
        "guest": false,
        "error_code": null,
        "notice": null,
        "blocked_status": null
    })
}

/// A note card as `search/notes` returns it; `n` picks the note ID and token.
fn note(n: u32) -> Value {
    json!({
        "id": note_id(n),
        "model_type": "note",
        "xsec_token": format!("ABtoken{n}="),
        "note_card": {
            "type": "normal",
            "display_title": format!("手冲咖啡 {n}"),
            "user": {"user_id": "5ff0e6410000000001008400", "nickname": "豆子", "xsec_token": "ABuser="},
            "interact_info": {"liked": false, "liked_count": "1.2万", "collected": false, "collected_count": "356", "comment_count": "89", "shared_count": "12"},
            "cover": {"url_default": "https://sns-webpic-qc.xhscdn.com/cover.jpg"},
            "image_list": [{"width": 1080, "height": 1440}],
            "corner_tag_info": [{"type": "publish_time", "text": "2025-06-13"}]
        }
    })
}

fn note_id(n: u32) -> String {
    format!("66f0a1b2c3d4e5f6071{n:05}")
}

fn hot_query() -> Value {
    json!({
        "id": "hot_query_2fhpxs5cks5vx6nvw1ar4",
        "model_type": "hot_query",
        "hot_query": {"queries": [{"name": "咖啡豆推荐", "search_word": "咖啡豆推荐"}]}
    })
}

fn sort_filter(sort: &str) -> Value {
    json!({"tags": [sort], "type": "sort_type"})
}

fn filters(sort: &str, note_type: &str, time: &str) -> Value {
    json!([
        sort_filter(sort),
        {"tags": [note_type], "type": "filter_note_type"},
        {"tags": [time], "type": "filter_note_time"},
        {"tags": ["不限"], "type": "filter_note_range"},
        {"tags": ["不限"], "type": "filter_pos_distance"}
    ])
}

/// One captured `search/notes` exchange, after `click` filter clicks.
fn response(
    click: u32,
    page: u32,
    search_id: &str,
    filters: &Value,
    has_more: bool,
    items: &[Value],
) -> Value {
    json!({
        "click": click,
        "request": {"keyword": "咖啡", "page": page, "search_id": search_id, "filters": filters},
        "body": {"code": 0, "success": true, "msg": "成功", "data": {"has_more": has_more, "items": items}}
    })
}

/// A default-condition first page with `items`.
fn default_page(has_more: bool, items: &[Value]) -> Value {
    response(0, 1, ROOT_SEARCH_ID, &Value::Null, has_more, items)
}

fn search_data(filter_clicks: u32, responses: &[Value]) -> Value {
    json!({
        "page": results_page(),
        "filter_clicks": filter_clicks,
        "timed_out": false,
        "body_missing": false,
        "responses": responses
    })
}

fn fake(data: &Value) -> FakeOpenCli {
    FakeOpenCli::contract_envelope(XHS_CONTRACT, "ok", data)
}

fn enabled(fake: &FakeOpenCli) -> RunEnvironment {
    RunEnvironment::new(&fake.route_config(
        "xiaohongshu_browser",
        "xiaohongshu",
        "[\"xiaohongshu_browser\"]",
    ))
}

fn search(fake: &FakeOpenCli, arguments: &[&str]) -> Output {
    let environment = enabled(fake);
    let mut command = vec!["platform", "xiaohongshu", "search", "咖啡"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

/// Splits an argv into the route arguments and checks that the transport flags end it.
fn route_arguments(call: &[String]) -> Vec<String> {
    let flags = &call[call.len() - SESSION_FLAGS.len()..];
    assert_eq!(flags, SESSION_FLAGS, "argv {call:?}");
    let rest = &call[..call.len() - SESSION_FLAGS.len()];
    let [arguments @ .., flag, _timeout] = rest else {
        panic!("argv {call:?} lacks --timeout");
    };
    assert_eq!(flag, "--timeout", "argv {call:?}");
    arguments.to_vec()
}

#[test]
fn search_reads_the_pages_after_the_last_filter_click_into_note_items() {
    let week = filters("time_descending", "不限", "一周内");
    let fake = fake(&search_data(
        2,
        &[
            default_page(true, &[note(90)]),
            response(
                1,
                1,
                FILTERED_SEARCH_ID,
                &json!([sort_filter("time_descending")]),
                true,
                &[note(91)],
            ),
            response(
                2,
                1,
                FILTERED_SEARCH_ID,
                &week,
                true,
                &[hot_query(), note(1)],
            ),
            response(2, 2, FILTERED_SEARCH_ID, &week, false, &[note(2)]),
        ],
    ));

    let output = search(
        &fake,
        &[
            "--limit",
            "25",
            "--sort",
            "latest",
            "--publish-time",
            "week",
        ],
    );

    let payload = payload(&output);
    assert_eq!(
        (
            output.status.code(),
            &payload["provider"],
            refs(&output),
            &payload["items"][0],
            &payload["next_cursor"],
        ),
        (
            Some(0),
            &json!("xiaohongshu_browser"),
            vec![note_ref(1), note_ref(2)],
            &json!(
            {
                "ref": "xiaohongshu:66f0a1b2c3d4e5f607100001",
                "url": "https://www.xiaohongshu.com/explore/66f0a1b2c3d4e5f607100001",
                "depth": "metadata",
                "title": "手冲咖啡 1",
                "authors": ["豆子"],
                "published": "2025-06-13",
                "note_type": "image",
                "author_id": "5ff0e6410000000001008400",
                "likes": "1.2万",
                "collects": "356",
                "comments": "89",
                "shares": "12",
                "published_text": "2025-06-13",
                "access_url": "https://www.xiaohongshu.com/explore/66f0a1b2c3d4e5f607100001?xsec_token=ABtoken1=&xsec_source=pc_search"
            }
            ),
            &Value::Null,
        ),
        "stderr: {}",
        stderr(&output)
    );
    let calls = fake.calls();
    assert_eq!(
        calls
            .iter()
            .map(|call| route_arguments(call))
            .collect::<Vec<_>>(),
        [[
            "forager-xhs",
            "search",
            "--query",
            "咖啡",
            "--sort",
            "latest",
            "--note-type",
            "all",
            "--publish-time",
            "week",
            "--pages",
            "2"
        ]
        .map(str::to_owned)
        .to_vec()]
    );
}

fn refs(output: &Output) -> Vec<String> {
    payload(output)["items"]
        .as_array()
        .unwrap_or_else(|| panic!("items: {}\nstderr: {}", payload(output), stderr(output)))
        .iter()
        .map(|item| item["ref"].as_str().expect("ref").to_owned())
        .collect()
}

fn note_ref(n: u32) -> String {
    format!("xiaohongshu:{}", note_id(n))
}

const MORE_RESULTS: &str = "Xiaohongshu has more results than this command returned; Xiaohongshu search cannot continue from a cursor, so raise --limit (up to 100) to read more in one command";

#[test]
fn notes_repeated_across_pages_count_once_before_the_limit_truncates() {
    let first = (1..=20).map(note).collect::<Vec<_>>();
    let second = (20..=25).map(note).collect::<Vec<_>>();
    let fake = fake(&search_data(
        0,
        &[
            default_page(true, &first),
            response(0, 2, ROOT_SEARCH_ID, &Value::Null, false, &second),
        ],
    ));

    let output = search(&fake, &["--limit", "21"]);

    assert_eq!(
        (output.status.code(), refs(&output)),
        (Some(0), (1..=21).map(note_ref).collect::<Vec<_>>())
    );
    assert!(
        stderr(&output).contains(MORE_RESULTS),
        "{}",
        stderr(&output)
    );
}

#[test]
fn the_unlisted_results_diagnostic_follows_the_last_page_and_the_truncation() {
    let cases = [
        // The site has more pages than the limit read.
        (1, vec![note(1)], true),
        // The last page holds more notes than the limit keeps.
        (2, vec![note(1), note(2), note(3)], false),
        // Skipped cards leave fewer notes than the limit, and the site has no more.
        (5, vec![hot_query(), note(1), note(2)], false),
    ];
    let warned = cases.map(|(limit, items, has_more)| {
        let fake = fake(&search_data(0, &[default_page(has_more, &items)]));
        let output = search(&fake, &["--limit", &limit.to_string()]);
        assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
        stderr(&output).contains(MORE_RESULTS)
    });

    assert_eq!(warned, [true, true, false]);
}

#[test]
fn notes_without_a_valid_id_or_token_are_skipped_with_one_diagnostic() {
    let mut bad_id = note(2);
    bad_id["id"] = json!("not-a-note-id");
    let mut no_token = note(3);
    no_token
        .as_object_mut()
        .expect("note object")
        .remove("xsec_token");
    let fake = fake(&search_data(
        0,
        &[default_page(false, &[bad_id, note(1), no_token])],
    ));

    let output = search(&fake, &[]);

    assert_eq!(
        (output.status.code(), refs(&output)),
        (Some(0), vec![note_ref(1)])
    );
    assert!(
        stderr(&output)
            .contains("skipped 2 Xiaohongshu notes without a valid note ID or access token"),
        "{}",
        stderr(&output)
    );
}

fn failure(output: &Output) -> (Option<i32>, Value, String) {
    let payload = payload(output);
    (
        output.status.code(),
        payload["error_kind"].clone(),
        payload["message"].as_str().unwrap_or_default().to_owned(),
    )
}

#[test]
fn a_page_whose_notes_are_all_unusable_fails_instead_of_returning_nothing() {
    let mut bad_id = note(1);
    bad_id["id"] = json!("not-a-note-id");
    let fake = fake(&search_data(0, &[default_page(false, &[bad_id])]));

    let output = search(&fake, &[]);

    assert_eq!(
        failure(&output),
        (
            Some(4),
            json!("runtime"),
            "Xiaohongshu listed 1 notes, but none has a valid note ID and access token".to_owned()
        )
    );
}

#[test]
fn published_dates_without_a_year_or_shown_as_relative_take_the_local_clock() {
    let mut new_year = note(1);
    new_year["note_card"]["corner_tag_info"] = json!([{"type": "publish_time", "text": "01-01"}]);
    let mut relative = note(2);
    relative["note_card"]["corner_tag_info"] = json!([{"type": "publish_time", "text": "3天前"}]);
    let mut unknown = note(3);
    unknown["note_card"]["corner_tag_info"] = json!([{"type": "publish_time", "text": "前天"}]);
    let fake = fake(&search_data(
        0,
        &[default_page(false, &[new_year, relative, unknown])],
    ));
    let today = chrono::Local::now().date_naive();
    // January 1 is never in the future, so it falls in the current local year.
    let year = today.format("%Y");
    let three_days_ago = (today - chrono::Days::new(3)).format("%Y-%m-%d");

    let output = search(&fake, &[]);
    let items = payload(&output)["items"].clone();

    assert_eq!(
        [0, 1, 2].map(|index| (
            items[index]["published"].clone(),
            items[index]["published_text"].clone()
        )),
        [
            (json!(format!("{year}-01-01")), json!("01-01")),
            (json!(three_days_ago.to_string()), json!("3天前")),
            (Value::Null, json!("前天")),
        ]
    );
}

#[test]
fn a_first_page_without_notes_is_an_empty_result_only_when_nothing_more_remains() {
    let outcomes = [
        (false, vec![]),
        (false, vec![hot_query()]),
        (true, vec![hot_query()]),
    ]
    .map(|(has_more, items)| {
        let fake = fake(&search_data(0, &[default_page(has_more, &items)]));
        let output = search(&fake, &[]);
        let payload = payload(&output);
        (
            output.status.code(),
            payload.get("items").cloned(),
            payload.get("error_kind").cloned(),
        )
    });

    assert_eq!(
        outcomes,
        [
            (Some(0), Some(json!([])), None),
            (Some(0), Some(json!([])), None),
            (Some(4), None, Some(json!("runtime"))),
        ]
    );
}

#[test]
fn responses_that_do_not_answer_the_request_fail_as_runtime() {
    let week = filters("time_descending", "不限", "一周内");
    let day = filters("time_descending", "不限", "一天内");
    let cases = [
        // The site applied another publication time.
        vec![
            response(2, 1, FILTERED_SEARCH_ID, &day, true, &[note(1)]),
            response(2, 2, FILTERED_SEARCH_ID, &day, false, &[note(2)]),
        ],
        // Page 2 is missing.
        vec![
            response(2, 1, FILTERED_SEARCH_ID, &week, true, &[note(1)]),
            response(2, 3, FILTERED_SEARCH_ID, &week, false, &[note(2)]),
        ],
        // The second page belongs to another search.
        vec![
            response(2, 1, FILTERED_SEARCH_ID, &week, true, &[note(1)]),
            response(2, 2, ROOT_SEARCH_ID, &week, false, &[note(2)]),
        ],
    ];
    let mut messages = cases
        .map(|responses| {
            let fake = fake(&search_data(2, &responses));
            failure(&search(
                &fake,
                &[
                    "--limit",
                    "40",
                    "--sort",
                    "latest",
                    "--publish-time",
                    "week",
                ],
            ))
        })
        .to_vec();
    let mut other_keyword = response(0, 1, ROOT_SEARCH_ID, &Value::Null, false, &[note(1)]);
    other_keyword["request"]["keyword"] = json!("咖啡豆");
    messages.push(failure(&search(
        &fake(&search_data(0, &[other_keyword])),
        &[],
    )));
    let mut unclicked = search_data(0, &[]);
    unclicked["filter_failure"] = json!("no_filter_panel for 排序依据 最新");
    messages.push(failure(&search(&fake(&unclicked), &["--sort", "latest"])));

    assert_eq!(
        messages,
        [
            "Xiaohongshu applied `一天内` for filter_note_time, not the requested condition",
            "Xiaohongshu answered page 3 where page 2 was due",
            "the Xiaohongshu search changed its search_id between pages",
            "Xiaohongshu searched for \"咖啡豆\", not \"咖啡\"",
            "the forager-xhs adapter made 0 filter clicks, not 1 (no_filter_panel for 排序依据 最新)",
        ]
        .map(|message| (Some(4), json!("runtime"), message.to_owned()))
    );
}

fn with_page(facts: &Value, timed_out: bool, body_missing: bool) -> Value {
    json!({
        "page": facts,
        "filter_clicks": 0,
        "timed_out": timed_out,
        "body_missing": body_missing,
        "responses": []
    })
}

#[test]
fn page_facts_classify_a_search_that_returned_no_results() {
    let mut guest = results_page();
    guest["guest"] = json!(true);
    guest["notice"] = json!("登录后查看搜索结果");
    let mut blocked = results_page();
    blocked["blocked_status"] = json!(461);
    let restricted = json!({
        "url": "https://www.xiaohongshu.com/website-login/error?error_code=300031&redirectPath=",
        "title": "安全限制",
        "guest": false,
        "error_code": "300031",
        "notice": "安全限制",
        "blocked_status": null
    });
    let unknown = json!({
        "url": "https://www.xiaohongshu.com/website-login/captcha?redirectPath=",
        "title": "验证",
        "guest": false,
        "error_code": null,
        "notice": null,
        "blocked_status": null
    });
    let kinds = [
        with_page(&guest, true, false),
        with_page(&blocked, false, false),
        with_page(&restricted, false, false),
        with_page(&results_page(), true, false),
        with_page(&unknown, true, false),
        with_page(&results_page(), true, true),
    ]
    .map(|data| {
        let (code, kind, message) = failure(&search(&fake(&data), &[]));
        (
            code,
            kind,
            message.split(';').next().unwrap_or_default().to_owned(),
        )
    });

    assert_eq!(
        kinds,
        [
            (Some(4), json!("auth"), "Xiaohongshu treats the browser session as logged out".to_owned()),
            (Some(4), json!("auth"), "Xiaohongshu answered HTTP 461 and wants a verification".to_owned()),
            (Some(4), json!("parameter"), "Xiaohongshu blocked the page (300031: 安全限制)".to_owned()),
            (Some(4), json!("timeout"), "Xiaohongshu returned 0 of 1 search pages before the read deadline".to_owned()),
            (
                Some(4),
                json!("runtime"),
                "the forager-xhs adapter stopped at an unexpected page: `验证` (https://www.xiaohongshu.com/website-login/captcha?redirectPath=)".to_owned()
            ),
            (
                Some(4),
                json!("runtime"),
                format!("a Xiaohongshu search response arrived without its body on `咖啡 - 小红书搜索` ({RESULTS_URL})")
            ),
        ]
    );
}

#[test]
fn a_read_deadline_on_the_results_page_with_a_trailing_slash_is_a_timeout() {
    // Observed live on 2026-10-09: Xiaohongshu served the results under `/search_result/`.
    let mut page = results_page();
    page["url"] = json!(
        "https://www.xiaohongshu.com/search_result/?keyword=%E5%92%96%E5%95%A1&source=web_explore_feed"
    );

    let (code, kind, _) = failure(&search(&fake(&with_page(&page, true, false)), &[]));

    assert_eq!((code, kind), (Some(4), json!("timeout")));
}

#[test]
fn a_read_deadline_on_the_browser_load_error_page_is_a_network_failure() {
    // Observed live on 2026-10-09: the first navigation failed and Chrome showed its own error
    // page; the command then waits for Chrome to reload the page.
    let mut page = results_page();
    page["url"] = json!(
        "https://www.xiaohongshu.com/search_result/?keyword=%E5%92%96%E5%95%A1&source=web_explore_feed"
    );
    page["title"] = json!("www.xiaohongshu.com");
    page["load_error"] = json!("ERR_CONNECTION_CLOSED");

    let result = failure(&search(&fake(&with_page(&page, true, false)), &[]));

    assert_eq!(
        result,
        (
            Some(4),
            json!("network"),
            "Chrome could not load the Xiaohongshu page (ERR_CONNECTION_CLOSED) and had not loaded it again by the read deadline; check the network and retry".to_owned()
        )
    );
}

#[test]
fn a_read_deadline_before_the_last_filter_click_is_a_timeout() {
    let mut data = search_data(
        1,
        &[
            default_page(true, &[note(1)]),
            response(1, 1, FILTERED_SEARCH_ID, &Value::Null, true, &[note(2)]),
        ],
    );
    data["timed_out"] = json!(true);

    let result = failure(&search(
        &fake(&data),
        &["--sort", "latest", "--publish-time", "week"],
    ));

    assert_eq!(
        result,
        (
            Some(4),
            json!("timeout"),
            "the read deadline passed after 1 of 2 Xiaohongshu filter clicks".to_owned()
        )
    );
}

#[test]
fn the_default_empty_order_refuses_the_search_with_the_steps_to_enable_it() {
    let fake = fake(&search_data(0, &[]));
    let environment = RunEnvironment::new(&format!(
        "[providers.xiaohongshu_browser]\ncommand = {:?}\n",
        fake.executable().display().to_string()
    ));

    let output = environment.run(&["platform", "xiaohongshu", "search", "咖啡"]);

    assert_eq!(
        (output.status.code(), fake.calls().len()),
        (Some(3), 0),
        "{}",
        stderr(&output)
    );
    assert!(
        stderr(&output).contains(
            "platforms.xiaohongshu.order has no configured route for xiaohongshu search; to use xiaohongshu_browser, install the forager OpenCLI adapter `forager-xhs` (copy the `opencli/forager-xhs` directory of the forager skill to `~/.opencli/clis/forager-xhs`), open the site in the Chrome that OpenCLI drives and log in if it asks, then add `xiaohongshu_browser` to platforms.xiaohongshu.order"
        ),
        "{}",
        stderr(&output)
    );
}
