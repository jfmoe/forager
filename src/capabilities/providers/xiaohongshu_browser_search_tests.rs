//! Search observations shaped like the live page facts and `search/notes` exchanges of
//! 2026-10-08 and 2026-11-18, judged into items or failures without a browser.

use chrono::{NaiveDate, NaiveDateTime};
use serde_json::{Value, json};

use super::{Decoded, SearchData, published_date, search_outcome};
use crate::types::{
    AttemptErrorKind, XiaohongshuPublishTime, XiaohongshuSearchOptions, XiaohongshuSort,
};

const RESULTS_URL: &str =
    "https://www.xiaohongshu.com/search_result?keyword=%E5%92%96%E5%95%A1&source=web_explore_feed";
const ROOT_SEARCH_ID: &str = "2fhpxs5cks5vx6nvw1ar4";
const FILTERED_SEARCH_ID: &str = "2fhpxs5cks5vx6nvw1ar4@2fhpxsfv1qadn0lsg9txa";
const MORE_RESULTS: &str = "Xiaohongshu has more results than this command returned; Xiaohongshu search cannot continue from a cursor, so raise --limit (up to 100) to read more in one command";

fn at(date: (i32, u32, u32), time: (u32, u32)) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(date.0, date.1, date.2)
        .and_then(|date| date.and_hms_opt(time.0, time.1, 0))
        .expect("valid local time")
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

fn note_ref(n: u32) -> String {
    format!("xiaohongshu:{}", note_id(n))
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

fn with_page(facts: &Value, timed_out: bool, body_missing: bool) -> Value {
    json!({
        "page": facts,
        "filter_clicks": 0,
        "timed_out": timed_out,
        "body_missing": body_missing,
        "responses": []
    })
}

fn latest_week() -> XiaohongshuSearchOptions {
    XiaohongshuSearchOptions {
        sort: XiaohongshuSort::Latest,
        publish_time: XiaohongshuPublishTime::Week,
        ..XiaohongshuSearchOptions::default()
    }
}

/// Judges `data` for a search for 咖啡 at noon on 2026-03-01.
fn judge(
    data: &Value,
    options: &XiaohongshuSearchOptions,
    limit: u16,
) -> Result<Decoded, (AttemptErrorKind, String)> {
    let data = serde_json::from_value::<SearchData>(data.clone()).expect("search observation");
    search_outcome(&data, "咖啡", options, limit, at((2026, 3, 1), (12, 0)))
        .map_err(|failure| (failure.kind, failure.message))
}

fn refs(decoded: &Decoded) -> Vec<String> {
    decoded
        .items
        .iter()
        .map(|item| item.reference.to_string())
        .collect()
}

fn default_search(data: &Value, limit: u16) -> Result<Decoded, (AttemptErrorKind, String)> {
    judge(data, &XiaohongshuSearchOptions::default(), limit)
}

fn runtime(message: &str) -> Result<Vec<String>, (AttemptErrorKind, String)> {
    Err((AttemptErrorKind::Runtime, message.to_owned()))
}

#[test]
fn the_pages_after_the_last_filter_click_decode_into_note_items() {
    let week = filters("time_descending", "不限", "一周内");
    let data = search_data(
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
    );

    let decoded = judge(&data, &latest_week(), 25).expect("decoded search");

    assert_eq!(
        (
            refs(&decoded),
            serde_json::to_value(&decoded.items[0]).expect("item JSON"),
            decoded.incomplete,
        ),
        (
            vec![note_ref(1), note_ref(2)],
            json!({
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
            }),
            None,
        )
    );
}

#[test]
fn notes_repeated_across_pages_count_once_before_the_limit_truncates() {
    let first = (1..=20).map(note).collect::<Vec<_>>();
    let second = (20..=25).map(note).collect::<Vec<_>>();
    let data = search_data(
        0,
        &[
            default_page(true, &first),
            response(0, 2, ROOT_SEARCH_ID, &Value::Null, false, &second),
        ],
    );

    let decoded = default_search(&data, 21).expect("decoded search");

    assert_eq!(
        (refs(&decoded), decoded.incomplete.as_deref()),
        (
            (1..=21).map(note_ref).collect::<Vec<_>>(),
            Some(MORE_RESULTS)
        )
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
        default_search(&search_data(0, &[default_page(has_more, &items)]), limit)
            .expect("decoded search")
            .incomplete
            .is_some_and(|diagnostic| diagnostic == MORE_RESULTS)
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
    let data = search_data(0, &[default_page(false, &[bad_id, note(1), no_token])]);

    let decoded = default_search(&data, 20).expect("decoded search");

    assert_eq!(
        (refs(&decoded), decoded.skipped.as_deref()),
        (
            vec![note_ref(1)],
            Some("skipped 2 Xiaohongshu notes without a valid note ID or access token")
        )
    );
}

#[test]
fn a_page_whose_notes_are_all_unusable_fails_instead_of_returning_nothing() {
    let mut bad_id = note(1);
    bad_id["id"] = json!("not-a-note-id");

    let result = default_search(&search_data(0, &[default_page(false, &[bad_id])]), 20);

    assert_eq!(
        result.map(|decoded| refs(&decoded)),
        runtime("Xiaohongshu listed 1 notes, but none has a valid note ID and access token")
    );
}

#[test]
fn published_dates_without_a_year_or_shown_as_relative_count_from_the_request_time() {
    let shown = |n: u32, text: &str| {
        let mut card = note(n);
        card["note_card"]["corner_tag_info"] = json!([{"type": "publish_time", "text": text}]);
        card
    };
    let data = search_data(
        0,
        &[default_page(
            false,
            &[shown(1, "01-01"), shown(2, "3天前"), shown(3, "前天")],
        )],
    );

    let decoded = default_search(&data, 20).expect("decoded search");

    assert_eq!(
        decoded
            .items
            .iter()
            .map(|item| item.published.as_deref())
            .collect::<Vec<_>>(),
        [Some("2026-01-01"), Some("2026-02-26"), None]
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
        default_search(&search_data(0, &[default_page(has_more, &items)]), 20)
            .map(|decoded| refs(&decoded))
            .map_err(|(kind, _)| kind)
    });

    assert_eq!(
        outcomes,
        [Ok(vec![]), Ok(vec![]), Err(AttemptErrorKind::Runtime)]
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
    let mut results = cases
        .map(|responses| {
            judge(&search_data(2, &responses), &latest_week(), 40).map(|decoded| refs(&decoded))
        })
        .to_vec();
    let mut other_keyword = response(0, 1, ROOT_SEARCH_ID, &Value::Null, false, &[note(1)]);
    other_keyword["request"]["keyword"] = json!("咖啡豆");
    results
        .push(default_search(&search_data(0, &[other_keyword]), 20).map(|decoded| refs(&decoded)));

    assert_eq!(
        results,
        [
            runtime(
                "Xiaohongshu applied `一天内` for filter_note_time, not the requested condition"
            ),
            runtime("Xiaohongshu answered page 3 where page 2 was due"),
            runtime("the Xiaohongshu search changed its search_id between pages"),
            runtime("Xiaohongshu searched for \"咖啡豆\", not \"咖啡\""),
        ]
    );
}

#[test]
fn a_filter_click_that_never_happened_fails_with_its_reason() {
    let mut unclicked = search_data(0, &[]);
    unclicked["filter_failure"] = json!("no_filter_panel for 排序依据 最新");
    let options = XiaohongshuSearchOptions {
        sort: XiaohongshuSort::Latest,
        ..XiaohongshuSearchOptions::default()
    };

    let result = judge(&unclicked, &options, 20).map(|decoded| refs(&decoded));

    assert_eq!(
        result,
        runtime(
            "the forager-xhs adapter made 0 filter clicks, not 1 (no_filter_panel for 排序依据 最新)"
        )
    );
}

#[test]
fn page_facts_classify_a_search_that_returned_no_results() {
    let mut guest = results_page();
    guest["guest"] = json!(true);
    guest["notice"] = json!("登录后查看搜索结果");
    let mut login_wall = results_page();
    login_wall["notice"] = json!("登录后查看搜索结果");
    let mut blocked = results_page();
    blocked["blocked_status"] = json!(461);
    let link_error = json!({
        "url": "https://www.xiaohongshu.com/website-login/error?error_code=300031&redirectPath=",
        "title": "访问链接异常",
        "guest": false,
        "error_code": "300031",
        "notice": "访问链接异常，请重新登录或返回首页",
        "blocked_status": null
    });
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
        with_page(&login_wall, false, false),
        with_page(&blocked, false, false),
        with_page(&link_error, false, false),
        with_page(&restricted, false, false),
        with_page(&results_page(), true, false),
        with_page(&unknown, true, false),
        with_page(&results_page(), true, true),
    ]
    .map(|data| {
        let (kind, message) = default_search(&data, 20).map(|_| ()).unwrap_err();
        (
            kind,
            message.split(';').next().unwrap_or_default().to_owned(),
        )
    });

    assert_eq!(
        kinds,
        [
            (AttemptErrorKind::Auth, "Xiaohongshu treats the browser session as logged out".to_owned()),
            (AttemptErrorKind::Auth, "Xiaohongshu treats the browser session as logged out".to_owned()),
            (AttemptErrorKind::Auth, "Xiaohongshu answered HTTP 461 and wants a verification".to_owned()),
            (
                AttemptErrorKind::Parameter,
                "Xiaohongshu blocked the page (300031: 访问链接异常，请重新登录或返回首页)".to_owned()
            ),
            (AttemptErrorKind::Parameter, "Xiaohongshu blocked the page (300031: 安全限制)".to_owned()),
            (AttemptErrorKind::Timeout, "Xiaohongshu returned 0 of 1 search pages before the read deadline".to_owned()),
            (
                AttemptErrorKind::Runtime,
                "the forager-xhs adapter stopped at an unexpected page: `验证` (https://www.xiaohongshu.com/website-login/captcha?redirectPath=)".to_owned()
            ),
            (
                AttemptErrorKind::Runtime,
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

    let result = default_search(&with_page(&page, true, false), 20);

    assert_eq!(
        result.map(|_| ()).map_err(|(kind, _)| kind),
        Err(AttemptErrorKind::Timeout)
    );
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

    let result = default_search(&with_page(&page, true, false), 20).map(|decoded| refs(&decoded));

    assert_eq!(
        result,
        Err((
            AttemptErrorKind::Network,
            "Chrome could not load the Xiaohongshu page (ERR_CONNECTION_CLOSED) and had not loaded it again by the read deadline; check the network and retry".to_owned()
        ))
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

    let result = judge(&data, &latest_week(), 20).map(|decoded| refs(&decoded));

    assert_eq!(
        result,
        Err((
            AttemptErrorKind::Timeout,
            "the read deadline passed after 1 of 2 Xiaohongshu filter clicks".to_owned()
        ))
    );
}

#[test]
fn a_month_and_day_take_the_latest_year_that_is_not_in_the_future() {
    let now = at((2026, 3, 1), (12, 0));

    let dates = [
        "07-09",
        "03-01",
        "02-28",
        "2025-06-13",
        "02-29",
        "2025-6-13",
    ]
    .map(|shown| published_date(shown, now));

    assert_eq!(
        dates,
        [
            Some("2025-07-09".to_owned()),
            Some("2026-03-01".to_owned()),
            Some("2026-02-28".to_owned()),
            Some("2025-06-13".to_owned()),
            None,
            None,
        ]
    );
}

#[test]
fn a_relative_time_counts_back_from_now_to_a_local_date() {
    let now = at((2026, 3, 1), (0, 30));

    let dates = [
        "刚刚",
        "5分钟前",
        "45分钟前",
        "1小时前",
        "3天前",
        "昨天",
        "昨天 23:09",
    ]
    .map(|shown| published_date(shown, now));

    assert_eq!(
        dates,
        [
            Some("2026-03-01".to_owned()),
            Some("2026-03-01".to_owned()),
            Some("2026-02-28".to_owned()),
            Some("2026-02-28".to_owned()),
            Some("2026-02-26".to_owned()),
            Some("2026-02-28".to_owned()),
            Some("2026-02-28".to_owned()),
        ]
    );
}

#[test]
fn other_texts_have_no_date() {
    let now = at((2026, 3, 1), (12, 0));

    let dates = [
        "前天",
        "3周前",
        "分钟前",
        "-3天前",
        "3 天前",
        "昨天 25:00",
        "昨天23:09",
        "今天 08:00",
    ]
    .map(|shown| published_date(shown, now));

    assert_eq!(dates, [None, None, None, None, None, None, None, None]);
}
