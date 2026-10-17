//! YouTube fixtures: configuration for `youtube_api`, resource builders in the shapes the Data
//! API v3 returned on 2026-10-17, and request and output readers shared by the
//! `platform youtube` tests.

use std::collections::BTreeMap;
use std::process::Output;

use serde_json::{Value, json};

use super::Response;

/// The fixture API key; tests assert it never leaves the request header.
pub(crate) const KEY: &str = "yt-fixture-key-0123456789";

/// Points `youtube_api` at the fixture server with one key and one attempt per request.
pub(crate) fn config(url: &str) -> String {
    format!(
        "[providers.youtube_api]\nurl = \"{url}/youtube/v3\"\nkeys = [\"{KEY}\"]\n\n[retry]\nmax_attempts = 1\n"
    )
}

pub(crate) fn json(body: &Value) -> Response {
    Response::json(200, &body.to_string())
}

/// A `search.list` response holding `items`, with `next_page_token` when another page exists.
pub(crate) fn search_page(items: &[Value], next_page_token: Option<&str>) -> Response {
    let mut page = json!({
        "kind": "youtube#searchListResponse",
        "etag": "search-etag",
        "regionCode": "US",
        "pageInfo": {"totalResults": 1_000_000, "resultsPerPage": items.len()},
        "items": items
    });
    if let Some(token) = next_page_token {
        page["nextPageToken"] = json!(token);
    }
    json(&page)
}

/// One `search.list` video result.
pub(crate) fn search_result(video_id: &str, title: &str, description: &str) -> Value {
    json!({
        "kind": "youtube#searchResult",
        "etag": format!("etag-{video_id}"),
        "id": {"kind": "youtube#video", "videoId": video_id},
        "snippet": {
            "publishedAt": "2024-04-07T12:53:54Z",
            "channelId": "UCYO_jab_esuFRV4b17AJtAw",
            "title": title,
            "description": description,
            "thumbnails": {"default": {"url": format!("https://i.ytimg.com/vi/{video_id}/default.jpg"), "width": 120, "height": 90}},
            "channelTitle": "3Blue1Brown",
            "liveBroadcastContent": "none",
            "publishTime": "2024-04-07T12:53:54Z"
        }
    })
}

/// A `videos.list` response holding `items`.
pub(crate) fn videos_page(items: &[Value]) -> Response {
    json(&json!({
        "kind": "youtube#videoListResponse",
        "etag": "videos-etag",
        "items": items,
        "pageInfo": {"totalResults": items.len(), "resultsPerPage": items.len()}
    }))
}

/// The `contentDetails` and `statistics` of a video, as search enrichment requests them.
pub(crate) fn video_stats(
    video_id: &str,
    duration: &str,
    views: &str,
    comments: Option<&str>,
) -> Value {
    let mut statistics = json!({"viewCount": views, "likeCount": "91777", "favoriteCount": "0"});
    if let Some(comments) = comments {
        statistics["commentCount"] = json!(comments);
    }
    json!({
        "kind": "youtube#video",
        "etag": format!("etag-{video_id}"),
        "id": video_id,
        "contentDetails": {
            "duration": duration,
            "dimension": "2d",
            "definition": "hd",
            "caption": "true",
            "licensedContent": true,
            "contentRating": {},
            "projection": "rectangular"
        },
        "statistics": statistics
    })
}

/// The full `videos.list` resource of "Me at the zoo", as fetch requests it.
pub(crate) fn zoo_video(description: &str) -> Value {
    json!({
        "kind": "youtube#video",
        "etag": "etag-jNQXAC9IVRw",
        "id": "jNQXAC9IVRw",
        "snippet": {
            "publishedAt": "2005-04-24T03:31:52Z",
            "channelId": "UC4QobU6STFB0P71PMvOGN5A",
            "title": "Me at the zoo",
            "description": description,
            "thumbnails": {},
            "channelTitle": "jawed",
            "tags": ["me at the zoo"],
            "categoryId": "1",
            "liveBroadcastContent": "none",
            "defaultLanguage": "en",
            "localized": {"title": "Me at the zoo", "description": description},
            "defaultAudioLanguage": "en"
        },
        "contentDetails": {
            "duration": "PT19S",
            "dimension": "2d",
            "definition": "hd",
            "caption": "true",
            "licensedContent": true,
            "contentRating": {},
            "projection": "rectangular"
        },
        "status": {
            "uploadStatus": "processed",
            "privacyStatus": "public",
            "license": "youtube",
            "embeddable": true,
            "publicStatsViewable": false,
            "madeForKids": false
        },
        "statistics": {
            "viewCount": "444694156",
            "likeCount": "20083234",
            "favoriteCount": "0",
            "commentCount": "10641589"
        }
    })
}

/// A Data API error body with one `errors[]` reason, and an `ErrorInfo` reason when given.
pub(crate) fn api_error(
    status: u16,
    reason: &str,
    info_reason: Option<&str>,
    message: &str,
) -> Response {
    let mut details = Vec::new();
    if let Some(info_reason) = info_reason {
        details.push(json!({
            "@type": "type.googleapis.com/google.rpc.ErrorInfo",
            "reason": info_reason,
            "domain": "googleapis.com",
            "metadata": {"service": "youtube.googleapis.com"}
        }));
    }
    Response::json(
        status,
        &json!({
            "error": {
                "code": status,
                "message": message,
                "errors": [{"message": message, "domain": "global", "reason": reason}],
                "status": if status == 400 { "INVALID_ARGUMENT" } else { "PERMISSION_DENIED" },
                "details": details
            }
        })
        .to_string(),
    )
}

pub(crate) fn payload(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "parse JSON stdout: {error}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// Returns the path of a recorded request.
pub(crate) fn path(request: &str) -> String {
    let target = request.split_whitespace().nth(1).expect("request target");
    target.split('?').next().unwrap_or_default().to_owned()
}

/// Returns the decoded query parameters of a recorded request.
pub(crate) fn query(request: &str) -> BTreeMap<String, String> {
    let target = request.split_whitespace().nth(1).expect("request target");
    reqwest::Url::parse(&format!("http://fixture.test{target}"))
        .expect("request URL")
        .query_pairs()
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}

/// Returns the value of a request header, matched case-insensitively.
pub(crate) fn header(request: &str, name: &str) -> Option<String> {
    request
        .split("\r\n\r\n")
        .next()
        .unwrap_or_default()
        .lines()
        .skip(1)
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.trim()
                .eq_ignore_ascii_case(name)
                .then(|| value.trim().to_owned())
        })
}

pub(crate) fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Builds the pairs of a recorded query for comparison.
pub(crate) fn pairs(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}
