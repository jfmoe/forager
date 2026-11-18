//! The `xiaohongshu_browser` search against a fake `opencli` executable: the adapter command line,
//! the enabling steps, and the contract check. The judgement of the page facts is unit-tested with
//! the route.

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
    RunEnvironment::new(&fake.config("[\"xiaohongshu_browser\"]"))
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

fn failure(output: &Output) -> (Option<i32>, Value, String) {
    let payload = payload(output);
    (
        output.status.code(),
        payload["error_kind"].clone(),
        payload["message"].as_str().unwrap_or_default().to_owned(),
    )
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

#[test]
fn an_adapter_of_the_previous_contract_fails_with_the_reinstall_steps() {
    let fake = FakeOpenCli::contract_envelope(
        "forager-xhs/1",
        "ok",
        &search_data(0, &[default_page(false, &[note(1)])]),
    );

    let (code, kind, message) = failure(&search(&fake, &[]));

    assert_eq!((code, kind), (Some(4), json!("runtime")), "{message}");
    assert!(
        message.contains(
            "answers contract `forager-xhs/1`, but forager expects `forager-xhs/2`; install or update the forager OpenCLI adapter: copy the `opencli/forager-xhs` directory of the forager skill to `~/.opencli/clis/forager-xhs`"
        ),
        "{message}"
    );
}
