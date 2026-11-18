//! The `xiaohongshu_browser` fetch against a fake `opencli` executable: the adapter command line,
//! the delivery of the native full text, and the access token rules. The judgement of the page
//! facts is unit-tested with the route.

mod support;

use std::process::Output;

use serde_json::{Value, json};

use support::RunEnvironment;
use support::opencli::{FakeOpenCli, XHS_CONTRACT};

const NOTE_ID: &str = "66f0a1b2c3d4e5f607100001";
const TOKEN: &str = "ABfakeTOKENcanary4242=";
/// The token without its padding, which no URL encoding changes.
const TOKEN_BODY: &str = "ABfakeTOKENcanary4242";

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

fn enabled(fake: &FakeOpenCli) -> RunEnvironment {
    RunEnvironment::new(&fake.config("[\"xiaohongshu_browser\"]"))
}

fn fetch(environment: &RunEnvironment, input: &str, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "xiaohongshu", "fetch", input];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

#[test]
fn a_note_without_an_access_token_fails_before_starting_opencli() {
    let fake = FakeOpenCli::contract_envelope(XHS_CONTRACT, "ok", &json!({}));
    let environment = enabled(&fake);
    let inputs = [
        format!("xiaohongshu:{NOTE_ID}"),
        format!("https://www.xiaohongshu.com/explore/{NOTE_ID}"),
        format!("https://www.xiaohongshu.com/discovery/item/{NOTE_ID}?xsec_source=pc_search"),
    ];

    let outcomes = inputs.map(|input| {
        let output = fetch(&environment, &input, &[]);
        (output.status.code(), stderr(&output))
    });

    let hint = "xiaohongshu fetch needs the note's access token: pass the `access_url` of a xiaohongshu search result, or the full note URL with its `xsec_token` copied from the browser";
    for (code, stderr) in &outcomes {
        assert_eq!(*code, Some(2), "{stderr}");
        assert!(stderr.contains(hint), "{stderr}");
    }
    assert_eq!(fake.calls().len(), 0);
}

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

/// The page facts of the note page, as the adapter reports them: the token value removed.
fn note_page() -> Value {
    json!({
        "url": format!("https://www.xiaohongshu.com/explore/{NOTE_ID}?xsec_source=pc_search"),
        "title": "手冲咖啡入门 - 小红书",
        "guest": false,
        "error_code": null,
        "notice": null,
        "blocked_status": null
    })
}

/// An image note as `__INITIAL_STATE__.note.noteDetailMap[<id>].note` holds it.
fn image_note() -> Value {
    json!({
        "noteId": NOTE_ID,
        "type": "normal",
        "title": "手冲咖啡入门",
        "desc": "第一次手冲就成功了 #咖啡[话题]# #手冲[话题]#",
        "time": 1_759_900_000_000_u64,
        "lastUpdateTime": 1_759_903_600_000_u64,
        "ipLocation": "上海",
        "user": {"userId": "5ff0e6410000000001008400", "nickname": "豆子", "avatar": "https://sns-avatar-qc.xhscdn.com/avatar.jpg"},
        "interactInfo": {"liked": false, "likedCount": "1.2万", "collected": false, "collectedCount": "356", "commentCount": "89", "shareCount": "12"},
        "tagList": [
            {"id": "5be00ac2000000000101ecbc", "name": "咖啡", "type": "topic"},
            {"id": "5be00ac2000000000101ecbd", "name": "手冲", "type": "topic"}
        ],
        "imageList": [
            {"urlDefault": "http://sns-webpic-qc.xhscdn.com/202610081306/a1/1040g2sg31!nd_dft_wlteh_webp_3", "width": 1080, "height": 1440},
            {"urlDefault": "", "width": 1080, "height": 1440},
            {"urlDefault": "https://sns-webpic-qc.xhscdn.com/202610081306/a2/1040g2sg32!nd_dft_wlteh_webp_3", "width": 1080, "height": 1080}
        ],
        "xsecToken": TOKEN
    })
}

fn note_data(note: &Value) -> Value {
    json!({"page": note_page(), "note": note, "timed_out": false})
}

fn fake(data: &Value) -> FakeOpenCli {
    FakeOpenCli::contract_envelope(XHS_CONTRACT, "ok", data)
}

#[test]
fn metadata_reads_the_note_page_into_the_note_fields() {
    let fake = fake(&note_data(&image_note()));
    let environment = enabled(&fake);

    let output = fetch(&environment, &access_url(), &["--depth", "metadata"]);

    assert_eq!(
        (output.status.code(), payload(&output)),
        (
            Some(0),
            json!({
                "platform": "xiaohongshu",
                "provider": "xiaohongshu_browser",
                "ref": "xiaohongshu:66f0a1b2c3d4e5f607100001",
                "url": "https://www.xiaohongshu.com/explore/66f0a1b2c3d4e5f607100001",
                "depth": "metadata",
                "title": "手冲咖啡入门",
                "authors": ["豆子"],
                "published": "2025-10-08T13:06:40+08:00",
                "updated": "2025-10-08T14:06:40+08:00",
                "note_type": "image",
                "author_id": "5ff0e6410000000001008400",
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
            })
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
            "note",
            "--id",
            NOTE_ID,
            "--xsec-token",
            TOKEN
        ]
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

const IMAGE_NOTE_MARKDOWN: &str = "# 手冲咖啡入门\n\n第一次手冲就成功了 #咖啡[话题]# #手冲[话题]#\n\n标签：咖啡，手冲\n\n![](http://sns-webpic-qc.xhscdn.com/202610081306/a1/1040g2sg31!nd_dft_wlteh_webp_3)\n![](https://sns-webpic-qc.xhscdn.com/202610081306/a2/1040g2sg32!nd_dft_wlteh_webp_3)";

#[test]
fn full_text_writes_the_native_note_body_without_any_web_fetch_provider() {
    let fake = fake(&note_data(&image_note()));
    let environment = enabled(&fake);
    let content_dir = tempfile::tempdir().expect("content directory");
    let content_dir_text = content_dir.path().display().to_string();

    let output = fetch(
        &environment,
        &access_url(),
        &["--content-dir", &content_dir_text],
    );
    let payload = payload(&output);
    let path = content_dir
        .path()
        .join("xiaohongshu-66f0a1b2c3d4e5f607100001.md");

    assert_eq!(
        (
            output.status.code(),
            &payload["depth"],
            &payload["content_url"],
            &payload["content_provider"],
            &payload["content_path"],
            &payload["content_len"],
        ),
        (
            Some(0),
            &json!("full_text"),
            &json!("https://www.xiaohongshu.com/explore/66f0a1b2c3d4e5f607100001"),
            &json!("xiaohongshu_browser"),
            &json!(path.display().to_string()),
            &json!(IMAGE_NOTE_MARKDOWN.chars().count()),
        ),
        "stderr: {}",
        stderr(&output)
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("read the full text"),
        IMAGE_NOTE_MARKDOWN
    );
}

#[test]
fn full_text_in_content_format_prints_the_body_and_writes_no_file() {
    let fake = fake(&note_data(&image_note()));
    let environment = enabled(&fake);
    let content_dir = tempfile::tempdir().expect("content directory");
    let content_dir_text = content_dir.path().display().to_string();

    let output = fetch(
        &environment,
        &access_url(),
        &["--format", "content", "--content-dir", &content_dir_text],
    );

    assert_eq!(
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            std::fs::read_dir(content_dir.path())
                .expect("list the content directory")
                .count(),
        ),
        (Some(0), format!("{IMAGE_NOTE_MARKDOWN}\n"), 0),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_full_text_that_cannot_be_written_is_a_runtime_failure() {
    let fake = fake(&note_data(&image_note()));
    let environment = enabled(&fake);
    let blocker = tempfile::NamedTempFile::new().expect("a file where the directory should be");
    let content_dir = blocker.path().join("notes").display().to_string();

    let (code, kind, message) = failure(&fetch(
        &environment,
        &access_url(),
        &["--content-dir", &content_dir],
    ));

    assert_eq!(
        (
            code,
            kind,
            message.starts_with("cannot write the full text to ")
        ),
        (Some(4), json!("runtime"), true),
        "{message}"
    );
}

#[test]
fn an_unavailable_note_names_the_likely_causes_and_is_never_retried() {
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
    let fake = fake(&unavailable);
    let environment = enabled(&fake);

    let result = failure(&fetch(&environment, &access_url(), &[]));

    assert_eq!(
        (result, fake.calls().len()),
        (
            (
                Some(4),
                json!("parameter"),
                "Xiaohongshu note unavailable: xiaohongshu:66f0a1b2c3d4e5f607100001 (300031: 当前笔记暂时无法浏览); the access token may be stale, the note restricted or removed, or the account rate-limited".to_owned()
            ),
            1
        )
    );
}

fn printed(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn the_access_token_appears_only_in_the_access_url_of_a_success() {
    let fake = fake(&note_data(&image_note()));
    let environment = enabled(&fake);

    let output = fetch(
        &environment,
        &access_url(),
        &["--depth", "metadata", "--verbose"],
    );
    let mut payload = payload(&output);
    let access = payload
        .as_object_mut()
        .expect("result object")
        .remove("access_url");

    assert_eq!(
        (
            output.status.code(),
            access == Some(json!(access_url())),
            payload.to_string().contains(TOKEN),
            stderr(&output).contains(TOKEN),
        ),
        (Some(0), true, false, false)
    );
}

#[test]
fn failures_never_print_the_access_token() {
    let echoed = FakeOpenCli::failing(
        "COMMAND_EXEC",
        &format!("forager-xhs/note failed for --xsec-token {TOKEN} on note {NOTE_ID}"),
        1,
    );
    let mut misshapen = note_data(&image_note());
    misshapen["page"]["guest"] = json!(TOKEN);
    let misshapen = fake(&misshapen);
    let mut unavailable = note_data(&Value::Null);
    unavailable["page"]["error_code"] = json!("300017");
    unavailable["page"]["notice"] = json!(format!("访问链接异常 xsec_token {TOKEN}"));
    let unavailable = fake(&unavailable);
    let mut redirected = note_data(&Value::Null);
    redirected["page"]["url"] = json!(format!(
        "https://www.xiaohongshu.com/website-login/captcha?redirectPath=https%3A%2F%2Fwww.xiaohongshu.com%2Fexplore%2F{NOTE_ID}%3Fxsec_token%3D{TOKEN_BODY}%253D"
    ));
    let redirected = fake(&redirected);

    let outputs = [&echoed, &misshapen, &unavailable, &redirected].map(|fake| {
        let output = fetch(&enabled(fake), &access_url(), &["--verbose"]);
        (
            output.status.code(),
            payload(&output)["provider_attempts"]
                .as_array()
                .map(Vec::len),
            printed(&output),
        )
    });
    let malformed = fetch(
        &enabled(&echoed),
        &format!("https://www.xiaohongshu.com/explore/{NOTE_ID}?xsec_token={TOKEN}%zz"),
        &["--verbose"],
    );

    for (code, attempts, printed) in &outputs {
        assert_eq!((*code, *attempts), (Some(4), Some(1)), "{printed}");
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
