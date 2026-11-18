//! Comments observations shaped like the live comment responses of 2026-10-09 and 2026-11-18,
//! judged into comments or failures without a browser.

use serde_json::{Value, json};

use super::{CommentsData, read_comments, without_token};
use crate::redact::CREDENTIAL_MASK;
use crate::types::{AccessToken, AttemptErrorKind, XiaohongshuCommentsRequest, XiaohongshuRef};

const NOTE_ID: &str = "6a9d4f5f00000000260323c4";
const OTHER_NOTE_ID: &str = "6a9d4f5f00000000260399ff";
const TOKEN: &str = "ABfakeTOKENcanary4242=";
/// The token without its padding, which no URL encoding changes.
const TOKEN_BODY: &str = "ABfakeTOKENcanary4242";

/// The page facts of the note page: the token value removed.
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

/// A comment ID: a fixed prefix and a five-digit serial.
fn id(serial: u32) -> String {
    format!("6a9eda8f00000000140{serial:05}")
}

/// A top-level comment without replies.
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
        "user_info": {"user_id": "5ff0e6410000000001008400", "nickname": "豆子", "xsec_token": "ABuser="},
        "target_comment": null
    })
}

/// A reply to `target`.
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
        "body": {"code": 0, "success": true, "msg": "成功", "data": {"comments": comments, "cursor": next, "has_more": has_more}}
    })
}

fn sub_page(root: u32, cursor: &str, replies: &[Value], has_more: bool) -> Value {
    json!({
        "kind": "sub",
        "params": {"note_id": NOTE_ID, "cursor": cursor, "root_comment_id": id(root)},
        "body": {"code": 0, "success": true, "msg": "成功", "data": {"comments": replies, "cursor": id(999), "has_more": has_more}}
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

fn request(limit: u16, replies: u16) -> XiaohongshuCommentsRequest {
    XiaohongshuCommentsRequest {
        note: XiaohongshuRef::from_note_id(NOTE_ID).expect("note ID"),
        access: AccessToken::new(TOKEN).expect("access token"),
        limit,
        replies,
    }
}

/// Judges `data` and renders the comments as the public result shows them.
fn judge(
    data: &Value,
    limit: u16,
    replies: u16,
) -> Result<(Value, bool), (AttemptErrorKind, String)> {
    let data = serde_json::from_value::<CommentsData>(data.clone()).expect("comments observation");
    read_comments(&data, &request(limit, replies))
        .map(|(comments, has_more)| {
            (
                serde_json::to_value(&comments).expect("comments JSON"),
                has_more,
            )
        })
        .map_err(|failure| (failure.kind, failure.message))
}

fn ids(comments: &Value) -> Vec<String> {
    comments
        .as_array()
        .expect("comments")
        .iter()
        .map(|comment| comment["id"].as_str().unwrap_or_default().to_owned())
        .collect()
}

#[test]
fn comments_follow_the_cursor_chain_and_expand_the_first_selected_replies() {
    let [first, second] = two_pages();
    let observed = data(&[
        first,
        second,
        sub_page(
            1,
            &id(101),
            &[reply(102, 1), reply(103, 102), reply(101, 1)],
            true,
        ),
    ]);

    let (comments, has_more) = judge(&observed, 15, 1).expect("comments");

    assert_eq!(
        (
            has_more,
            ids(&comments),
            &comments[0],
            &comments[11]["replies_has_more"],
            comments[11]["replies"].as_array().map(Vec::len),
        ),
        (
            true,
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
            Some(1),
        )
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

    let results = cases.map(|responses| judge(&data(&responses), 15, 1).map(|_| ()));

    let runtime = |message: &str| Err((AttemptErrorKind::Runtime, message.to_owned()));
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

    let results = [8, 5].map(|limit| {
        judge(&data(std::slice::from_ref(&last)), limit, 0)
            .map(|(comments, has_more)| (ids(&comments).len(), has_more))
    });

    assert_eq!(results, [Ok((8, false)), Ok((5, true))]);
}

#[test]
fn a_note_without_comments_lists_none_and_succeeds() {
    let result = judge(&data(&[page("", &[], "", false)]), 20, 0);

    assert_eq!(result, Ok((json!([]), false)));
}

#[test]
fn a_first_page_without_comments_that_says_more_remain_is_a_runtime_failure() {
    let result = judge(&data(&[page("", &[], "c1", true)]), 10, 0);

    assert_eq!(
        result,
        Err((
            AttemptErrorKind::Runtime,
            "Xiaohongshu listed no comments on the first page but says more remain".to_owned()
        ))
    );
}

#[test]
fn a_reply_expansion_the_page_logic_could_not_make_is_a_failure_with_its_reason() {
    let [first, _] = two_pages();
    let mut failed = data(&[first]);
    failed["expand_failure"] = json!("no_expand_button for comment 6a9eda8f0000000014000001");
    let [first, _] = two_pages();
    let mut waiting = data(&[first]);
    waiting["timed_out"] = json!(true);

    let results = [failed, waiting].map(|data| judge(&data, 10, 1).map(|_| ()));

    assert_eq!(
        results,
        [
            Err((
                AttemptErrorKind::Runtime,
                "the forager-xhs adapter could not expand the replies of a comment (no_expand_button for comment 6a9eda8f0000000014000001)".to_owned()
            )),
            Err((
                AttemptErrorKind::Timeout,
                "Xiaohongshu returned 0 of 1 reply expansions before the read deadline".to_owned()
            )),
        ]
    );
}

#[test]
fn a_failure_that_quotes_the_access_token_masks_it() {
    let mut redirected = data(&[]);
    redirected["timed_out"] = json!(true);
    redirected["page"]["url"] = json!(format!(
        "https://www.xiaohongshu.com/website-login/captcha?redirectPath=https%3A%2F%2Fwww.xiaohongshu.com%2Fexplore%2F{NOTE_ID}%3Fxsec_token%3D{TOKEN_BODY}%253D"
    ));
    let observed =
        serde_json::from_value::<CommentsData>(redirected).expect("comments observation");

    let failure = read_comments(&observed, &request(20, 0))
        .map(|_| ())
        .unwrap_err();
    let message = without_token(failure, &AccessToken::new(TOKEN).expect("access token")).message;

    assert!(
        !message.contains(TOKEN_BODY) && message.contains(CREDENTIAL_MASK),
        "{message}"
    );
}
