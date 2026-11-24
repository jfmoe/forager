//! Comments observations derived from the stored comments observation, judged into comments
//! or failures without a browser.

use reqwest::Url;
use serde_json::{Value, json};

use super::{
    CommentsData, Exchange, RequestMethod, ResponseBody, captured_comments, expansion_targets,
    read_comments, without_token,
};
use crate::providers::xiaohongshu_browser::tests::stored;
use crate::redact::CREDENTIAL_MASK;
use crate::types::{AccessToken, AttemptErrorKind, XiaohongshuCommentsRequest, XiaohongshuRef};

const NOTE_ID: &str = "66f0a1b2c3d4e5f607200001";
const OTHER_NOTE_ID: &str = "66f0a1b2c3d4e5f607299999";
const TOKEN: &str = "ABfakeTOKENcanary4242=";
/// The token without its padding, which no URL encoding changes.
const TOKEN_BODY: &str = "ABfakeTOKENcanary4242";

/// The stored observation of a note's comments: a comment page, then the reply page of its
/// second comment.
fn stored_comments() -> Value {
    stored("comments.json")
}

fn stored_response(kind: &str) -> Value {
    stored_comments()["responses"]
        .as_array()
        .and_then(|responses| responses.iter().find(|response| response["kind"] == kind))
        .cloned()
        .unwrap_or_else(|| panic!("a stored {kind} response"))
}

/// The stored top-level comment at `index` of the comment page.
fn stored_comment(index: usize) -> Value {
    stored_response("page")["body"]["data"]["comments"][index].clone()
}

/// The page facts of the note page: the token value removed.
fn note_page() -> Value {
    stored_comments()["page"].clone()
}

/// A comment ID: a fixed prefix and a five-digit serial.
fn id(serial: u32) -> String {
    format!("00000000000000000c0{serial:05}")
}

/// The stored top-level comment without replies, as comment `serial`.
fn comment(serial: u32) -> Value {
    let mut comment = stored_comment(0);
    comment["id"] = json!(id(serial));
    comment["content"] = json!(format!("评论 {serial}"));
    comment
}

/// The stored reply, as reply `serial` to `target`.
fn reply(serial: u32, target: u32) -> Value {
    let mut reply = stored_comment(1)["sub_comments"][0].clone();
    reply["id"] = json!(id(serial));
    reply["content"] = json!(format!("回复 {serial}"));
    reply["target_comment"]["id"] = json!(id(target));
    reply
}

/// Comment `serial`, carrying reply `inline` and, like the stored comment with replies, more
/// replies after it.
fn expandable(serial: u32, inline: u32) -> Value {
    let mut comment = comment(serial);
    comment["sub_comment_count"] = stored_comment(1)["sub_comment_count"].clone();
    comment["sub_comment_cursor"] = json!(id(inline));
    comment["sub_comment_has_more"] = json!(true);
    comment["sub_comments"] = json!([reply(inline, serial)]);
    comment
}

/// The stored comment page, as the page from `cursor` listing `comments`.
fn page(cursor: &str, comments: &[Value], next: &str, has_more: bool) -> Value {
    let mut page = stored_response("page");
    page["params"]["cursor"] = json!(cursor);
    page["body"]["data"] = json!({"comments": comments, "cursor": next, "has_more": has_more});
    page
}

/// The stored reply page, as the replies of comment `root` from `cursor`.
fn sub_page(root: u32, cursor: &str, replies: &[Value], has_more: bool) -> Value {
    let mut page = stored_response("sub");
    page["params"]["cursor"] = json!(cursor);
    page["params"]["root_comment_id"] = json!(id(root));
    page["body"]["data"] = json!({"comments": replies, "cursor": id(999), "has_more": has_more});
    page
}

/// The stored observation, with `responses` captured.
fn data(responses: &[Value]) -> Value {
    let mut data = stored_comments();
    data["responses"] = json!(responses);
    data
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
                "id": "00000000000000000c000001",
                "author": "豆子",
                "author_id": "00000000000000000000a001",
                "text": "评论 1",
                "likes": "3",
                "published": "2025-10-08T13:06:40+08:00",
                "ip_location": "上海",
                "reply_count": "20",
                "replies": [
                    {"id": "00000000000000000c000101", "author": "拿铁", "author_id": "00000000000000000000a002", "text": "回复 101", "likes": "1", "published": "2025-10-08T14:06:40+08:00", "ip_location": "北京", "reply_to": "00000000000000000c000001"},
                    {"id": "00000000000000000c000102", "author": "拿铁", "author_id": "00000000000000000000a002", "text": "回复 102", "likes": "1", "published": "2025-10-08T14:06:40+08:00", "ip_location": "北京", "reply_to": "00000000000000000c000001"},
                    {"id": "00000000000000000c000103", "author": "拿铁", "author_id": "00000000000000000000a002", "text": "回复 103", "likes": "1", "published": "2025-10-08T14:06:40+08:00", "ip_location": "北京", "reply_to": "00000000000000000c000102"}
                ],
                "replies_has_more": true
            }),
            &json!(true),
            Some(1),
        )
    );
}

/// The targets the page logic expands as each page of `pages` arrives, in turn, and the
/// targets the whole read selects.
fn targets_page_by_page(
    pages: &[Value],
    limit: u16,
    replies: u16,
) -> (Vec<Vec<String>>, Vec<String>) {
    let targets = |pages: &[Value]| {
        let data = serde_json::from_value::<CommentsData>(data(pages)).expect("observation");
        expansion_targets(&data.responses, &request(limit, replies))
    };
    let mut expanded = 0;
    let per_page = (1..=pages.len())
        .map(|read| {
            let due = targets(&pages[..read]).split_off(expanded);
            expanded += due.len();
            due
        })
        .collect();
    (per_page, targets(pages))
}

#[test]
fn expanding_each_page_as_it_arrives_selects_the_comments_the_whole_read_selects() {
    let mut first = (1..=10).map(comment).collect::<Vec<_>>();
    first[0] = expandable(1, 101);
    first[4] = expandable(5, 105);
    let mut second = (11..=20).map(comment).collect::<Vec<_>>();
    second[1] = expandable(12, 121);
    second[6] = expandable(17, 171);
    let pages = [
        page("", &first, &id(10), true),
        page(&id(10), &second, &id(20), true),
    ];
    let ids = |serials: &[u32]| serials.iter().copied().map(id).collect::<Vec<_>>();

    let cases = [(20, 3), (15, 10), (20, 1)]
        .map(|(limit, replies)| targets_page_by_page(&pages, limit, replies));

    assert_eq!(
        cases,
        [
            (vec![ids(&[1, 5]), ids(&[12])], ids(&[1, 5, 12])),
            (vec![ids(&[1, 5]), ids(&[12])], ids(&[1, 5, 12])),
            (vec![ids(&[1]), ids(&[])], ids(&[1])),
        ]
    );
}

#[test]
fn reply_pages_between_comment_pages_are_judged_as_after_them() {
    let [first, second] = two_pages();
    let replies = sub_page(1, &id(101), &[reply(102, 1)], false);
    let after = data(&[first.clone(), second.clone(), replies.clone()]);
    let between = data(&[first, replies, second]);

    let judged = [after, between].map(|observed| judge(&observed, 15, 1));

    assert_eq!(judged[0], judged[1]);
    assert!(judged[0].is_ok(), "{:?}", judged[0]);
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
                "Xiaohongshu comment page 2 continued from cursor `00000000000000000c000009`, not `00000000000000000c000010`"
            ),
            runtime(
                "Xiaohongshu answered comments of note `66f0a1b2c3d4e5f607299999` for xiaohongshu:66f0a1b2c3d4e5f607200001"
            ),
            runtime(
                "Xiaohongshu answered replies of comment `00000000000000000c000012`, which this command did not expand"
            ),
            runtime(
                "Xiaohongshu answered comments of note `66f0a1b2c3d4e5f607299999` for xiaohongshu:66f0a1b2c3d4e5f607200001"
            ),
            runtime(
                "the first Xiaohongshu reply page of comment `00000000000000000c000001` started from cursor ``, not `00000000000000000c000101`"
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
fn a_comment_page_with_a_block_code_is_an_unavailable_note_not_an_empty_list() {
    let mut blocked = data(&[page("", &[], "", false)]);
    blocked["page"]["api_error"] = json!({"path": "/api/sns/web/v2/comment/page", "code": 300_031, "msg": "当前笔记暂时无法浏览"});

    let result = judge(&blocked, 20, 0);

    assert_eq!(
        result,
        Err((
            AttemptErrorKind::Parameter,
            format!(
                "Xiaohongshu note unavailable: xiaohongshu:{NOTE_ID} (300031: 当前笔记暂时无法浏览); the access token may be stale, the note restricted or removed, or the account rate-limited"
            )
        ))
    );
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
    failed["expand_failure"] = json!("no_expand_button for comment 00000000000000000c000001");
    let [first, _] = two_pages();
    let mut waiting = data(&[first]);
    waiting["timed_out"] = json!(true);

    let results = [failed, waiting].map(|data| judge(&data, 10, 1).map(|_| ()));

    assert_eq!(
        results,
        [
            Err((
                AttemptErrorKind::Runtime,
                "the Xiaohongshu page logic could not expand the replies of a comment (no_expand_button for comment 00000000000000000c000001)".to_owned()
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

fn exchange(url: &str, body: &Value) -> Exchange {
    Exchange {
        sequence: 0,
        method: RequestMethod::Get,
        url: Url::parse(url).expect("API URL"),
        status: Some(200),
        request_body: None,
        body: ResponseBody::Received(body.to_string()),
    }
}

#[test]
fn captured_comment_exchanges_are_read_by_their_query_parameters() {
    let [first, _] = two_pages();
    let api = "https://edith.xiaohongshu.com/api/sns/web/v2/comment";
    let exchanges = [
        exchange(
            &format!(
                "{api}/page?note_id={NOTE_ID}&cursor=&top_comment_id=&image_formats=jpg,webp,avif&xsec_token={TOKEN_BODY}%3D"
            ),
            &first["body"],
        ),
        exchange(
            "https://edith.xiaohongshu.com/api/sns/web/v2/user/me",
            &stored("account.json"),
        ),
        exchange(
            &format!(
                "{api}/sub/page?note_id={NOTE_ID}&root_comment_id={}&num=10&cursor={}&image_formats=jpg,webp,avif&top_comment_id=&xsec_token={TOKEN_BODY}%3D",
                id(1),
                id(101)
            ),
            &sub_page(1, &id(101), &[reply(102, 1)], false)["body"],
        ),
    ];
    let observed = CommentsData {
        page: serde_json::from_value(note_page()).expect("page facts"),
        responses: captured_comments(&exchanges),
        ..CommentsData::default()
    };

    let (comments, has_more) = read_comments(&observed, &request(10, 1)).expect("comments");

    assert_eq!(
        (
            has_more,
            comments.len(),
            comments[0]
                .replies
                .iter()
                .map(|reply| reply.id.as_str())
                .collect::<Vec<_>>(),
        ),
        (
            true,
            10,
            vec!["00000000000000000c000101", "00000000000000000c000102"],
        )
    );
}
