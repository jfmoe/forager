//! Search observations shaped like the live page facts and `search/notes` exchanges of
//! 2026-10-08 and 2026-11-18, judged into items or failures without a browser.

use chrono::{NaiveDate, NaiveDateTime};
use reqwest::Url;
use serde_json::{Value, json};

use super::{
    Decoded, Exchange, Observed, RequestMethod, ResponseBody, SearchData, SearchProgress,
    filter_choices, published_date, search_data as captured_search_data, search_outcome,
    wants_another_page,
};
use crate::types::{
    AttemptErrorKind, XiaohongshuNoteType, XiaohongshuPublishTime, XiaohongshuSearchOptions,
    XiaohongshuSort,
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
fn a_blocked_search_is_never_an_empty_result() {
    let blocks = [
        ("notice", json!("请求太频繁")),
        ("blocked_status", json!(471)),
        ("guest", json!(true)),
        (
            "api_error",
            json!({"path": "/api/sns/web/v1/search/notes", "code": 300_017, "msg": null}),
        ),
        (
            "api_error",
            json!({"path": "/api/sns/web/v1/search/notes", "code": 999_999, "msg": "未知错误"}),
        ),
    ];

    let kinds = blocks.map(|(fact, value)| {
        let mut data = search_data(0, &[default_page(false, &[])]);
        data["page"][fact] = value;
        default_search(&data, 20)
            .map(|decoded| refs(&decoded))
            .map_err(|(kind, _)| kind)
    });

    assert_eq!(
        kinds,
        [
            Err(AttemptErrorKind::Auth),
            Err(AttemptErrorKind::Auth),
            Err(AttemptErrorKind::Auth),
            Err(AttemptErrorKind::Parameter),
            Err(AttemptErrorKind::Runtime),
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
            "the Xiaohongshu page logic made 0 filter clicks, not 1 (no_filter_panel for 排序依据 最新)"
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
                "the Xiaohongshu page logic stopped at an unexpected page: `验证` (https://www.xiaohongshu.com/website-login/captcha?redirectPath=)".to_owned()
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
            "the browser could not load the Xiaohongshu page (ERR_CONNECTION_CLOSED) and had not loaded it again by the read deadline; check the network and retry".to_owned()
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

#[test]
fn a_logged_out_session_and_risk_control_ask_for_a_browser_login() {
    let mut guest = results_page();
    guest["guest"] = json!(true);
    let mut blocked = results_page();
    blocked["blocked_status"] = json!(461);

    let hinted = [guest, blocked].map(|page| {
        let (kind, message) = default_search(&with_page(&page, false, false), 20)
            .map(|_| ())
            .unwrap_err();
        (
            kind,
            message.contains("run `forager browser login xiaohongshu`"),
        )
    });

    assert_eq!(
        hinted,
        [
            (AttemptErrorKind::Auth, true),
            (AttemptErrorKind::Auth, true)
        ]
    );
}

#[test]
fn a_read_deadline_counts_as_a_timeout_only_on_a_results_page_of_either_layout() {
    let page = |url: &str, title: &str| {
        let mut page = results_page();
        page["url"] = json!(url);
        page["title"] = json!(title);
        page
    };
    let pages = [
        page(
            "https://www.xiaohongshu.com/search_result_ai?keyword=%25E5%2592%2596%25E5%2595%25A1&source=web_explore_feed",
            "咖啡 - 小红书搜索",
        ),
        page(
            "https://www.xiaohongshu.com/explore",
            "小红书 - 你的生活兴趣社区",
        ),
    ];

    let results = pages.map(|page| {
        default_search(&with_page(&page, true, false), 20).map(|decoded| refs(&decoded))
    });

    assert_eq!(
        results,
        [
            Err((
                AttemptErrorKind::Timeout,
                "Xiaohongshu returned 0 of 1 search pages before the read deadline".to_owned()
            )),
            runtime(
                "the Xiaohongshu page logic stopped at an unexpected page: `小红书 - 你的生活兴趣社区` (https://www.xiaohongshu.com/explore)"
            ),
        ]
    );
}

#[test]
fn a_search_that_could_not_be_entered_fails_with_its_reason_and_page() {
    let mut data = with_page(
        &json!({"url": "https://www.xiaohongshu.com/explore", "title": "小红书 - 你的生活兴趣社区"}),
        false,
        false,
    );
    data["search_failure"] = json!("no_target for the search box");

    let result = default_search(&data, 20).map(|decoded| refs(&decoded));

    assert_eq!(
        result,
        runtime(
            "the Xiaohongshu page logic could not search from `小红书 - 你的生活兴趣社区` (https://www.xiaohongshu.com/explore) (no_target for the search box)"
        )
    );
}

#[test]
fn each_non_default_condition_is_one_filter_panel_option() {
    let options = XiaohongshuSearchOptions {
        sort: XiaohongshuSort::MostCommented,
        note_type: XiaohongshuNoteType::Video,
        publish_time: XiaohongshuPublishTime::HalfYear,
    };

    assert_eq!(
        (
            filter_choices(&XiaohongshuSearchOptions::default()),
            filter_choices(&options),
            filter_choices(&latest_week()),
        ),
        (
            vec![],
            vec![
                ("排序依据", "最多评论"),
                ("笔记类型", "视频"),
                ("发布时间", "半年内")
            ],
            vec![("排序依据", "最新"), ("发布时间", "一周内")],
        )
    );
}

fn exchange(path: &str, request: &Value, body: &Value) -> Exchange {
    Exchange {
        sequence: 0,
        method: RequestMethod::Post,
        url: Url::parse(&format!("https://edith.xiaohongshu.com{path}")).expect("API URL"),
        status: Some(200),
        request_body: Some(request.to_string()),
        body: ResponseBody::Received(body.to_string()),
    }
}

/// Numbers `exchanges` in the order their requests were sent.
fn sent_in_order(mut exchanges: Vec<Exchange>) -> Vec<Exchange> {
    for (sequence, exchange) in (0..).zip(&mut exchanges) {
        exchange.sequence = sequence;
    }
    exchanges
}

fn search_exchange(page: u32, search_id: &str, filters: &Value) -> Exchange {
    let response = response(0, page, search_id, filters, true, &[note(page)]);
    exchange(
        "/api/sns/web/v2/search/notes",
        &response["request"],
        &response["body"],
    )
}

#[test]
fn captured_search_responses_count_the_filter_clicks_made_before_them() {
    let week = filters("time_descending", "不限", "一周内");
    let onebox = exchange(
        "/api/sns/web/v1/search/onebox",
        &json!({"keyword": "咖啡", "search_id": ROOT_SEARCH_ID}),
        &json!({"code": 0, "success": false, "msg": "成功", "data": {}}),
    );
    let observed = Observed {
        page: serde_json::from_value(results_page()).expect("page facts"),
        exchanges: sent_in_order(vec![
            onebox,
            search_exchange(1, ROOT_SEARCH_ID, &Value::Null),
            search_exchange(2, ROOT_SEARCH_ID, &Value::Null),
            search_exchange(1, FILTERED_SEARCH_ID, &week),
            search_exchange(2, FILTERED_SEARCH_ID, &week),
        ]),
        timed_out: false,
        body_missing: false,
    };
    let progress = SearchProgress {
        filter_clicks: 1,
        marks: vec![3],
        ..SearchProgress::default()
    };
    let options = XiaohongshuSearchOptions {
        publish_time: XiaohongshuPublishTime::Week,
        sort: XiaohongshuSort::Latest,
        ..XiaohongshuSearchOptions::default()
    };

    let data = captured_search_data(observed, progress).expect("search observation");
    let clicks = data
        .responses
        .iter()
        .map(|response| (response.click, response.request.page))
        .collect::<Vec<_>>();
    // One click set both conditions here, so the judgement expects two.
    let judged = search_outcome(&data, "咖啡", &options, 40, at((2026, 3, 1), (12, 0)))
        .map(|decoded| refs(&decoded))
        .map_err(|failure| failure.message);

    assert_eq!(
        (clicks, judged),
        (
            vec![(0, 1), (0, 2), (1, 1), (1, 2)],
            Err("the Xiaohongshu page logic made 1 filter clicks, not 2".to_owned())
        )
    );
}

#[test]
fn a_read_deadline_before_the_search_was_submitted_is_a_timeout_or_a_load_failure() {
    let home =
        json!({"url": "https://www.xiaohongshu.com/explore", "title": "小红书 - 你的生活兴趣社区"});
    let mut error_page = home.clone();
    error_page["load_error"] = json!("ERR_CONNECTION_CLOSED");

    let kinds = [home, error_page].map(|page| {
        let mut data = with_page(&page, true, false);
        data["before_search"] = json!(true);
        default_search(&data, 20)
            .map(|_| ())
            .map_err(|(kind, message)| (kind, message.contains("before the search was submitted")))
    });

    assert_eq!(
        kinds,
        [
            Err((AttemptErrorKind::Timeout, true)),
            Err((AttemptErrorKind::Network, false))
        ]
    );
}

#[test]
fn a_read_that_timed_out_before_the_query_was_submitted_is_marked_before_search() {
    let observed = |timed_out| Observed {
        page: serde_json::from_value(results_page()).expect("page facts"),
        exchanges: Vec::new(),
        timed_out,
        body_missing: false,
    };
    let submitted = SearchProgress {
        submitted: true,
        ..SearchProgress::default()
    };

    let marks = [
        (observed(true), SearchProgress::default()),
        (observed(true), submitted),
        (observed(false), SearchProgress::default()),
    ]
    .map(|(observed, progress)| {
        captured_search_data(observed, progress)
            .expect("search observation")
            .before_search
    });

    assert_eq!(marks, [true, false, false]);
}

#[test]
fn the_results_are_read_until_the_needed_pages_have_answered() {
    let page = |page: u32, has_more: bool| {
        let response = response(
            0,
            page,
            ROOT_SEARCH_ID,
            &Value::Null,
            has_more,
            &[note(page)],
        );
        exchange(
            "/api/sns/web/v2/search/notes",
            &response["request"],
            &response["body"],
        )
    };
    // The first screen requests its second page by itself.
    let auto = sent_in_order(vec![page(1, true), page(2, true)]);
    let last = sent_in_order(vec![page(1, true), page(2, false)]);

    let wanted = [
        wants_another_page(&auto, 0, 2),
        wants_another_page(&auto, 0, 3),
        wants_another_page(&last, 0, 3),
        wants_another_page(&auto, 1, 2),
    ];

    assert_eq!(wanted, [false, true, false, true]);
}

#[test]
fn a_response_to_a_request_sent_before_a_filter_click_is_no_answer_to_it() {
    let latest = filters("time_descending", "不限", "不限");
    // The first screen's own second page was requested before the click but finished during
    // the hover, after the read last polled, so it ended after the click.
    let mut exchanges = sent_in_order(vec![
        search_exchange(1, ROOT_SEARCH_ID, &Value::Null),
        search_exchange(2, ROOT_SEARCH_ID, &Value::Null),
        search_exchange(1, FILTERED_SEARCH_ID, &latest),
        search_exchange(2, FILTERED_SEARCH_ID, &latest),
    ]);
    exchanges[2].sequence = 3;
    exchanges[3].sequence = 4;
    let observed = Observed {
        page: serde_json::from_value(results_page()).expect("page facts"),
        exchanges,
        timed_out: false,
        body_missing: false,
    };
    let progress = SearchProgress {
        filter_clicks: 1,
        marks: vec![2],
        ..SearchProgress::default()
    };
    let options = XiaohongshuSearchOptions {
        sort: XiaohongshuSort::Latest,
        ..XiaohongshuSearchOptions::default()
    };

    let data = captured_search_data(observed, progress).expect("search observation");
    let judged = search_outcome(&data, "咖啡", &options, 40, at((2026, 3, 1), (12, 0)))
        .map(|decoded| refs(&decoded))
        .map_err(|failure| failure.message);

    assert_eq!(judged, Ok(vec![note_ref(1), note_ref(2)]));
}
