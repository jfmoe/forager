//! Note page observations derived from the stored note page observation, judged into a note
//! or a failure without a browser.

use serde_json::{Value, json};

use super::{NoteData, note_outcome, without_token};
use crate::providers::xiaohongshu_browser::tests::stored;
use crate::redact::CREDENTIAL_MASK;
use crate::types::{AccessToken, AttemptErrorKind, FullTextSource, XiaohongshuRef};

const NOTE_ID: &str = "66f0a1b2c3d4e5f607100001";
const TOKEN: &str = "ABfakeTOKENcanary4242=";
/// The token without its padding, which no URL encoding changes.
const TOKEN_BODY: &str = "ABfakeTOKENcanary4242";
const IMAGE_NOTE_MARKDOWN: &str = "# 手冲咖啡入门\n\n第一次手冲就成功了 #咖啡[话题]# #手冲[话题]#\n\n标签：咖啡，手冲\n\n![](http://sns-webpic-qc.xhscdn.com/202610081306/a1/1040g2sg31!nd_dft_wlteh_webp_3)\n![](https://sns-webpic-qc.xhscdn.com/202610081306/a2/1040g2sg32!nd_dft_wlteh_webp_3)";

/// The stored observation of the image note's page.
fn stored_note() -> Value {
    stored("note.json")
}

/// The stored image note, as the page logic picks it from
/// `__INITIAL_STATE__.note.noteDetailMap[<id>].note`.
fn image_note() -> Value {
    stored_note()["note"].clone()
}

/// The stored observation, showing `note`.
fn note_data(note: &Value) -> Value {
    let mut data = stored_note();
    data["note"] = note.clone();
    data
}

fn requested() -> XiaohongshuRef {
    XiaohongshuRef::from_note_id(NOTE_ID).expect("note ID")
}

fn token() -> AccessToken {
    AccessToken::new(TOKEN).expect("access token")
}

/// Judges `data` for the requested note, with its body when `full_text` is set.
fn judge(
    data: &Value,
    full_text: bool,
) -> Result<(Value, Option<String>), (AttemptErrorKind, String)> {
    let data = serde_json::from_value::<NoteData>(data.clone()).expect("note observation");
    note_outcome(&data, &requested(), &token(), full_text)
        .map(|(item, source)| {
            let body = match source {
                FullTextSource::Native(body) => Some(body),
                FullTextSource::Urls(_) => None,
            };
            (serde_json::to_value(&item).expect("item JSON"), body)
        })
        .map_err(|failure| (failure.kind, failure.message))
}

fn failure(data: &Value) -> (AttemptErrorKind, String) {
    judge(data, true).map(|_| ()).unwrap_err()
}

#[test]
fn metadata_reads_the_note_page_into_the_note_fields() {
    let judged = judge(&note_data(&image_note()), false);

    assert_eq!(
        judged,
        Ok((
            json!({
                "ref": "xiaohongshu:66f0a1b2c3d4e5f607100001",
                "url": "https://www.xiaohongshu.com/explore/66f0a1b2c3d4e5f607100001",
                "depth": "metadata",
                "title": "手冲咖啡入门",
                "authors": ["豆子"],
                "published": "2025-10-08T13:06:40+08:00",
                "updated": "2025-10-08T14:06:40+08:00",
                "note_type": "image",
                "author_id": "00000000000000000000a001",
                "likes": "1.2万",
                "collects": "356",
                "comments": "89",
                "shares": "12",
                "tags": ["咖啡", "手冲"],
                "images": [
                    {"url": "http://sns-webpic-qc.xhscdn.com/202610081306/a1/1040g2sg31!nd_dft_wlteh_webp_3", "width": 1080, "height": 1440},
                    {"url": "https://sns-webpic-qc.xhscdn.com/202610081306/a2/1040g2sg32!nd_dft_wlteh_webp_3", "width": 1080, "height": 1080}
                ],
                "video": null,
                "ip_location": "上海",
                "access_url": "https://www.xiaohongshu.com/explore/66f0a1b2c3d4e5f607100001?xsec_token=ABfakeTOKENcanary4242=&xsec_source=pc_search"
            }),
            None
        ))
    );
}

#[test]
fn full_text_builds_the_native_note_body() {
    let body = judge(&note_data(&image_note()), true).map(|(_, body)| body);

    assert_eq!(body, Ok(Some(IMAGE_NOTE_MARKDOWN.to_owned())));
}

#[test]
fn a_note_without_a_title_text_or_images_is_a_quality_failure() {
    let mut empty = image_note();
    empty["title"] = json!("");
    empty["desc"] = json!(" ");
    empty["imageList"] = json!([]);

    assert_eq!(
        failure(&note_data(&empty)),
        (
            AttemptErrorKind::Quality,
            "the Xiaohongshu note xiaohongshu:66f0a1b2c3d4e5f607100001 has no title, text, or images".to_owned()
        )
    );
}

#[test]
fn a_video_note_reports_its_duration_and_size_but_never_its_signed_stream_url() {
    let mut video = image_note();
    video["type"] = json!("video");
    video["imageList"] = json!([{"urlDefault": "http://sns-webpic-qc.xhscdn.com/202610081306/v1/cover!nd_dft_wlteh_webp_3", "width": 1080, "height": 1920}]);
    video["video"] = json!({
        "capa": {"duration": 37},
        "media": {"stream": {
            "av1": [],
            "h264": [{"width": 1080, "height": 1920, "duration": 37_000, "size": 9_437_184, "masterUrl": "http://sns-video-bd.xhscdn.com/stream/79/110/259/01e6f0a1.mp4?sign=5f0c0ffee&t=6705a1b2"}],
            "h265": [{"width": 720, "height": 1280, "masterUrl": "http://sns-video-bd.xhscdn.com/stream/79/110/114/01e6f0a2.mp4?sign=5f0c0ffee&t=6705a1b2"}]
        }}
    });

    let (item, body) = judge(&note_data(&video), true).expect("video note");
    let body = body.expect("native body");

    assert_eq!(
        (
            body.ends_with("（视频笔记，时长 37 秒，视频文件未下载）"),
            &item["note_type"],
            &item["video"],
        ),
        (
            true,
            &json!("video"),
            &json!({"duration_seconds": 37, "width": 1080, "height": 1920}),
        ),
        "{body}"
    );
    for printed in [body, item.to_string()] {
        assert!(
            !printed.contains("sns-video-bd") && !printed.contains("sign="),
            "{printed}"
        );
    }
}

#[test]
fn a_page_that_shows_another_note_is_a_runtime_failure() {
    let mut other = image_note();
    other["noteId"] = json!("66f0a1b2c3d4e5f607199999");

    assert_eq!(
        failure(&note_data(&other)),
        (
            AttemptErrorKind::Runtime,
            "the Xiaohongshu page shows note `66f0a1b2c3d4e5f607199999` for xiaohongshu:66f0a1b2c3d4e5f607100001".to_owned()
        )
    );
}

#[test]
fn an_unavailable_note_names_the_likely_causes() {
    let unavailable = json!({
        "page": {
            "url": "https://www.xiaohongshu.com/404?source=note&error_code=300031&error_msg=%E5%BD%93%E5%89%8D%E7%AC%94%E8%AE%B0%E6%9A%82%E6%97%B6%E6%97%A0%E6%B3%95%E6%B5%8F%E8%A7%88",
            "title": "小红书 - 你的生活兴趣社区",
            "guest": false,
            "error_code": "300031",
            "notice": "当前笔记暂时无法浏览",
            "blocked_status": null
        },
        "note": null,
        "timed_out": false
    });

    assert_eq!(
        failure(&unavailable),
        (
            AttemptErrorKind::Parameter,
            "Xiaohongshu note unavailable: xiaohongshu:66f0a1b2c3d4e5f607100001 (300031: 当前笔记暂时无法浏览); the access token may be stale, the note restricted or removed, or the account rate-limited".to_owned()
        )
    );
}

#[test]
fn a_page_without_the_note_is_a_timeout_only_while_it_stays_on_the_note_page() {
    let mut waiting = note_data(&Value::Null);
    waiting["timed_out"] = json!(true);
    let elsewhere = json!({
        "page": {
            "url": "https://www.xiaohongshu.com/website-login/captcha?redirectPath=https%3A%2F%2Fwww.xiaohongshu.com%2Fexplore",
            "title": "验证",
            "guest": false,
            "error_code": null,
            "notice": null,
            "blocked_status": null
        },
        "note": null,
        "timed_out": true
    });

    assert_eq!(
        [waiting, elsewhere].map(|data| failure(&data)),
        [
            (
                AttemptErrorKind::Timeout,
                "the Xiaohongshu note page showed no note before the read deadline".to_owned()
            ),
            (
                AttemptErrorKind::Runtime,
                "the Xiaohongshu page logic stopped at an unexpected page: `验证` (https://www.xiaohongshu.com/website-login/captcha?redirectPath=https%3A%2F%2Fwww.xiaohongshu.com%2Fexplore)".to_owned()
            ),
        ]
    );
}

#[test]
fn a_read_deadline_on_the_browser_load_error_page_is_a_network_failure() {
    let mut data = note_data(&Value::Null);
    data["timed_out"] = json!(true);
    data["page"]["title"] = json!("www.xiaohongshu.com");
    data["page"]["load_error"] = json!("ERR_TIMED_OUT");

    assert_eq!(
        failure(&data),
        (
            AttemptErrorKind::Network,
            "the browser could not load the Xiaohongshu page (ERR_TIMED_OUT) and had not loaded it again by the read deadline; check the network and retry".to_owned()
        )
    );
}

#[test]
fn the_access_token_appears_only_in_the_access_url_of_a_success() {
    let (mut item, _) = judge(&note_data(&image_note()), false).expect("note");
    let access = item
        .as_object_mut()
        .expect("item object")
        .remove("access_url");

    assert_eq!(
        (access, item.to_string().contains(TOKEN_BODY),),
        (
            Some(json!(format!(
                "https://www.xiaohongshu.com/explore/{NOTE_ID}?xsec_token={TOKEN}&xsec_source=pc_search"
            ))),
            false
        )
    );
}

#[test]
fn a_failure_that_quotes_the_access_token_masks_it() {
    let mut unavailable = note_data(&Value::Null);
    unavailable["page"]["error_code"] = json!("300017");
    unavailable["page"]["notice"] = json!(format!("访问链接异常 xsec_token {TOKEN}"));
    let mut redirected = note_data(&Value::Null);
    redirected["timed_out"] = json!(true);
    redirected["page"]["url"] = json!(format!(
        "https://www.xiaohongshu.com/website-login/captcha?redirectPath=https%3A%2F%2Fwww.xiaohongshu.com%2Fexplore%2F{NOTE_ID}%3Fxsec_token%3D{TOKEN_BODY}%253D"
    ));

    let messages = [unavailable, redirected].map(|data| {
        let data = serde_json::from_value::<NoteData>(data).expect("note observation");
        let failure = note_outcome(&data, &requested(), &token(), false)
            .map(|_| ())
            .unwrap_err();
        without_token(failure, &token()).message
    });

    for message in &messages {
        assert!(
            !message.contains(TOKEN_BODY) && message.contains(CREDENTIAL_MASK),
            "{message}"
        );
    }
}
