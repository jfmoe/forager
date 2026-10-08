//! The `xiaohongshu_browser` comments against a fake `opencli` executable that answers envelopes
//! shaped like the live comment responses of 2026-10-09. These tests prove the transport, the
//! page order and ownership checks, and the decoding of comments and replies; only the live
//! smoke C29 exercises the JavaScript adapter.

mod support;

use std::process::Output;

use serde_json::{Value, json};

use support::RunEnvironment;
use support::opencli::{FakeOpenCli, XHS_CONTRACT};

const NOTE_ID: &str = "6a9d4f5f00000000260323c4";
const OTHER_NOTE_ID: &str = "6a9d4f5f00000000260399ff";
const TOKEN: &str = "ABfakeTOKENcanary4242=";
/// The token without its padding, which no URL encoding changes.
const TOKEN_BODY: &str = "ABfakeTOKENcanary4242";

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

fn access_url() -> String {
    format!(
        "https://www.xiaohongshu.com/explore/{NOTE_ID}?xsec_token={TOKEN}&xsec_source=pc_search"
    )
}

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

fn printed(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// Runs `comments` once in a fresh environment, so the route's pacing never waits.
fn comments_with(fake: &FakeOpenCli, input: &str, arguments: &[&str]) -> Output {
    let environment = RunEnvironment::new(&fake.route_config(
        "xiaohongshu_browser",
        "xiaohongshu",
        "[\"xiaohongshu_browser\"]",
    ));
    let mut command = vec!["platform", "xiaohongshu", "comments", input];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

fn comments(data: &Value, arguments: &[&str]) -> (FakeOpenCli, Output) {
    let fake = FakeOpenCli::contract_envelope(XHS_CONTRACT, "ok", data);
    let output = comments_with(&fake, &access_url(), arguments);
    (fake, output)
}

fn failure(output: &Output) -> (Option<i32>, Value, String) {
    let payload = payload(output);
    (
        output.status.code(),
        payload["error_kind"].clone(),
        payload["message"].as_str().unwrap_or_default().to_owned(),
    )
}

/// The page facts of the note page, as the adapter reports them: the token value removed.
fn note_page() -> Value {
    json!({
        "url": format!("https://www.xiaohongshu.com/explore/{NOTE_ID}?xsec_source=pc_search"),
        "title": "提神咖啡排行榜 - 小红书",
        "guest": false,
        "error_code": null,
        "notice": null,
        "blocked_status": null
    })
}

/// A comment ID: a fixed prefix and a four-digit serial.
fn id(serial: u32) -> String {
    format!("6a9eda8f00000000140{serial:05}")
}

/// A top-level comment without replies, as the adapter keeps it.
fn comment(serial: u32) -> Value {
    json!({
        "id": id(serial),
        "content": format!("评论 {serial}"),
        "create_time": 1_759_900_000_000_u64,
        "ip_location": "上海",
        "like_count": "3",
        "sub_comment_count": "0",
        "sub_comment_cursor": "",
        "sub_comment_has_more": false,
        "sub_comments": [],
        "user_info": {"user_id": "5ff0e6410000000001008400", "nickname": "豆子"},
        "target_comment": null
    })
}

/// A reply to `target`, as the adapter keeps it.
fn reply(serial: u32, target: u32) -> Value {
    json!({
        "id": id(serial),
        "content": format!("回复 {serial}"),
        "create_time": 1_759_903_600_000_u64,
        "ip_location": "北京",
        "like_count": "1",
        "user_info": {"user_id": "5ff0e6410000000001008401", "nickname": "拿铁"},
        "target_comment": {"id": id(target)}
    })
}

/// A top-level comment that carries reply `inline` and has more replies after it.
fn expandable(serial: u32, inline: u32) -> Value {
    let mut comment = comment(serial);
    comment["sub_comment_count"] = json!("20");
    comment["sub_comment_cursor"] = json!(id(inline));
    comment["sub_comment_has_more"] = json!(true);
    comment["sub_comments"] = json!([reply(inline, serial)]);
    comment
}

fn page(cursor: &str, comments: &[Value], next: &str, has_more: bool) -> Value {
    json!({
        "kind": "page",
        "params": {"note_id": NOTE_ID, "cursor": cursor},
        "body": {"msg": "成功", "data": {"comments": comments, "cursor": next, "has_more": has_more}}
    })
}

fn sub_page(root: u32, cursor: &str, replies: &[Value], has_more: bool) -> Value {
    json!({
        "kind": "sub",
        "params": {"note_id": NOTE_ID, "cursor": cursor, "root_comment_id": id(root), "num": "10"},
        "body": {"msg": "成功", "data": {"comments": replies, "cursor": id(999), "has_more": has_more}}
    })
}

fn data(responses: &[Value]) -> Value {
    json!({
        "page": note_page(),
        "timed_out": false,
        "body_missing": false,
        "expand_failure": null,
        "responses": responses
    })
}

/// Two pages of ten comments; the first comment carries reply 101 and has more replies, and
/// so does the twelfth, which a limit of 15 keeps.
fn two_pages() -> [Value; 2] {
    let mut first = (1..=10).map(comment).collect::<Vec<_>>();
    first[0] = expandable(1, 101);
    let mut second = (11..=20).map(comment).collect::<Vec<_>>();
    second[1] = expandable(12, 121);
    [
        page("", &first, &id(10), true),
        page(&id(10), &second, &id(20), true),
    ]
}

fn ids(payload: &Value) -> Vec<String> {
    payload["comments"]
        .as_array()
        .expect("comments")
        .iter()
        .map(|comment| comment["id"].as_str().unwrap_or_default().to_owned())
        .collect()
}

#[test]
fn comments_follow_the_cursor_chain_and_expand_the_first_selected_replies() {
    let [first, second] = two_pages();
    let (fake, output) = comments(
        &data(&[
            first,
            second,
            sub_page(
                1,
                &id(101),
                &[reply(102, 1), reply(103, 102), reply(101, 1)],
                true,
            ),
        ]),
        &["--limit", "15", "--replies", "1"],
    );
    let payload = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &payload["platform"],
            &payload["provider"],
            &payload["note"],
            &payload["has_more"],
            ids(&payload),
            &payload["comments"][0],
            &payload["comments"][11]["replies_has_more"],
            &payload["comments"][11]["replies"].as_array().map(Vec::len),
        ),
        (
            Some(0),
            &json!("xiaohongshu"),
            &json!("xiaohongshu_browser"),
            &json!("xiaohongshu:6a9d4f5f00000000260323c4"),
            &json!(true),
            (1..=15).map(id).collect::<Vec<_>>(),
            &json!({
                "id": "6a9eda8f0000000014000001",
                "author": "豆子",
                "author_id": "5ff0e6410000000001008400",
                "text": "评论 1",
                "likes": "3",
                "published": "2025-10-08T13:06:40+08:00",
                "ip_location": "上海",
                "reply_count": "20",
                "replies": [
                    {"id": "6a9eda8f0000000014000101", "author": "拿铁", "author_id": "5ff0e6410000000001008401", "text": "回复 101", "likes": "1", "published": "2025-10-08T14:06:40+08:00", "ip_location": "北京", "reply_to": "6a9eda8f0000000014000001"},
                    {"id": "6a9eda8f0000000014000102", "author": "拿铁", "author_id": "5ff0e6410000000001008401", "text": "回复 102", "likes": "1", "published": "2025-10-08T14:06:40+08:00", "ip_location": "北京", "reply_to": "6a9eda8f0000000014000001"},
                    {"id": "6a9eda8f0000000014000103", "author": "拿铁", "author_id": "5ff0e6410000000001008401", "text": "回复 103", "likes": "1", "published": "2025-10-08T14:06:40+08:00", "ip_location": "北京", "reply_to": "6a9eda8f0000000014000102"}
                ],
                "replies_has_more": true
            }),
            &json!(true),
            &Some(1),
        ),
        "stderr: {}",
        stderr(&output)
    );
    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    let call = &calls[0];
    assert_eq!(&call[call.len() - SESSION_FLAGS.len()..], SESSION_FLAGS);
    assert_eq!(
        call[..call.len() - SESSION_FLAGS.len() - 2],
        [
            "forager-xhs",
            "comments",
            "--id",
            NOTE_ID,
            "--xsec-token",
            TOKEN,
            "--limit",
            "15",
            "--expand",
            "1"
        ]
    );
}

#[test]
fn a_broken_order_or_ownership_of_any_response_is_a_runtime_failure() {
    let [first, second] = two_pages();
    let mut broken_chain = second.clone();
    broken_chain["params"]["cursor"] = json!(id(9));
    let mut foreign_page = second.clone();
    foreign_page["params"]["note_id"] = json!(OTHER_NOTE_ID);
    let unselected_root = sub_page(12, &id(121), &[reply(122, 12)], false);
    let mut foreign_replies = sub_page(1, &id(101), &[reply(102, 1)], false);
    foreign_replies["params"]["note_id"] = json!(OTHER_NOTE_ID);
    let wrong_start = sub_page(1, "", &[reply(102, 1)], false);
    let cases = [
        vec![first.clone(), broken_chain],
        vec![first.clone(), foreign_page],
        vec![first.clone(), second.clone(), unselected_root],
        vec![first.clone(), second.clone(), foreign_replies],
        vec![first, second, wrong_start],
    ];

    let results = cases.map(|responses| {
        failure(&comments(&data(&responses), &["--limit", "15", "--replies", "1"]).1)
    });

    let runtime = |message: &str| (Some(4), json!("runtime"), message.to_owned());
    assert_eq!(
        results,
        [
            runtime(
                "Xiaohongshu comment page 2 continued from cursor `6a9eda8f0000000014000009`, not `6a9eda8f0000000014000010`"
            ),
            runtime(
                "Xiaohongshu answered comments of note `6a9d4f5f00000000260399ff` for xiaohongshu:6a9d4f5f00000000260323c4"
            ),
            runtime(
                "Xiaohongshu answered replies of comment `6a9eda8f0000000014000012`, which this command did not expand"
            ),
            runtime(
                "Xiaohongshu answered comments of note `6a9d4f5f00000000260399ff` for xiaohongshu:6a9d4f5f00000000260323c4"
            ),
            runtime(
                "the first Xiaohongshu reply page of comment `6a9eda8f0000000014000001` started from cursor ``, not `6a9eda8f0000000014000101`"
            ),
        ]
    );
}

#[test]
fn comments_cut_by_the_limit_on_the_last_page_still_report_more() {
    let last = page("", &(1..=8).map(comment).collect::<Vec<_>>(), "", false);

    let results = ["8", "5"].map(|limit| {
        let payload = payload(&comments(&data(std::slice::from_ref(&last)), &["--limit", limit]).1);
        (ids(&payload).len(), payload["has_more"].clone())
    });

    assert_eq!(results, [(8, json!(false)), (5, json!(true))]);
}

#[test]
fn a_note_without_comments_lists_none_and_succeeds() {
    let (_, output) = comments(&data(&[page("", &[], "", false)]), &[]);

    assert_eq!(
        (output.status.code(), payload(&output)),
        (
            Some(0),
            json!({
                "platform": "xiaohongshu",
                "provider": "xiaohongshu_browser",
                "note": "xiaohongshu:6a9d4f5f00000000260323c4",
                "comments": [],
                "has_more": false
            })
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_reply_expansion_the_adapter_could_not_make_is_a_runtime_failure_with_its_reason() {
    let [first, _] = two_pages();
    let mut failed = data(&[first]);
    failed["expand_failure"] = json!("no_expand_button for comment 6a9eda8f0000000014000001");
    let [first, _] = two_pages();
    let mut waiting = data(&[first]);
    waiting["timed_out"] = json!(true);

    let results = [failed, waiting]
        .map(|data| failure(&comments(&data, &["--limit", "10", "--replies", "1"]).1));

    assert_eq!(
        results,
        [
            (
                Some(4),
                json!("runtime"),
                "the forager-xhs adapter could not expand the replies of a comment (no_expand_button for comment 6a9eda8f0000000014000001)".to_owned()
            ),
            (
                Some(4),
                json!("timeout"),
                "Xiaohongshu returned 0 of 1 reply expansions before the read deadline".to_owned()
            ),
        ]
    );
}

#[test]
fn a_note_without_an_access_token_fails_before_starting_opencli() {
    let fake = FakeOpenCli::contract_envelope(XHS_CONTRACT, "ok", &json!({}));

    let output = comments_with(&fake, &format!("xiaohongshu:{NOTE_ID}"), &[]);

    assert_eq!(
        (output.status.code(), fake.calls().len()),
        (Some(2), 0),
        "{}",
        stderr(&output)
    );
    assert!(
        stderr(&output).contains("xiaohongshu comments needs the note's access token: pass the `access_url` of a xiaohongshu search result"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn the_access_token_never_appears_in_a_result_or_a_failure() {
    let (_, success) = comments(&data(&two_pages()), &["--limit", "15", "--verbose"]);
    let echoed = FakeOpenCli::answering(
        "",
        &format!(
            "ok: false\nerror:\n  code: COMMAND_EXEC\n  message: forager-xhs/comments failed for --xsec-token {TOKEN} on note {NOTE_ID}\n  exitCode: 1\n"
        ),
        1,
    );
    let mut misshapen = data(&[]);
    misshapen["page"]["guest"] = json!(TOKEN);
    let misshapen = FakeOpenCli::contract_envelope(XHS_CONTRACT, "ok", &misshapen);
    let mut redirected = data(&[]);
    redirected["page"]["url"] = json!(format!(
        "https://www.xiaohongshu.com/website-login/captcha?redirectPath=https%3A%2F%2Fwww.xiaohongshu.com%2Fexplore%2F{NOTE_ID}%3Fxsec_token%3D{TOKEN_BODY}%253D"
    ));
    let redirected = FakeOpenCli::contract_envelope(XHS_CONTRACT, "ok", &redirected);

    let failures = [&echoed, &misshapen, &redirected].map(|fake| {
        let output = comments_with(fake, &access_url(), &["--verbose"]);
        (output.status.code(), printed(&output))
    });
    let malformed = comments_with(
        &echoed,
        &format!("https://www.xiaohongshu.com/explore/{NOTE_ID}?xsec_token={TOKEN}%zz"),
        &[],
    );

    assert_eq!(success.status.code(), Some(0), "{}", printed(&success));
    assert!(
        !printed(&success).contains(TOKEN_BODY),
        "{}",
        printed(&success)
    );
    for (code, printed) in &failures {
        assert_eq!(*code, Some(4), "{printed}");
        assert!(!printed.contains(TOKEN_BODY), "{printed}");
        assert!(printed.contains("********"), "{printed}");
    }
    assert_eq!(malformed.status.code(), Some(2), "{}", printed(&malformed));
    assert!(
        !printed(&malformed).contains(TOKEN_BODY),
        "{}",
        printed(&malformed)
    );
}
