//! YouTube fixtures: configuration for `youtube_api`, Data API bodies derived from the stored
//! samples, and request and output readers shared by the `platform youtube` tests.

use std::collections::BTreeMap;
use std::process::Output;

use serde_json::{Value, json};

use super::Response;
use super::youtube_api_samples::{self as samples, stored};

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

pub(crate) fn search_page(items: &[Value], next_page_token: Option<&str>) -> Response {
    json(&samples::search_page(items, next_page_token))
}

/// The stored first search result as the video `video_id` titled `title` with `description`.
pub(crate) fn search_result(video_id: &str, title: &str, description: &str) -> Value {
    let mut result = stored("search.json")["items"][0].clone();
    result["etag"] = json!(format!("etag-{video_id}"));
    result["id"]["videoId"] = json!(video_id);
    let snippet = &mut result["snippet"];
    snippet["title"] = json!(title);
    snippet["description"] = json!(description);
    snippet["thumbnails"]["default"]["url"] =
        json!(format!("https://i.ytimg.com/vi/{video_id}/default.jpg"));
    result
}

pub(crate) fn videos_page(items: &[Value]) -> Response {
    json(&samples::videos_page(items))
}

/// The stored details of a commented video as those of `video_id`, lasting `duration`, with
/// `views` and, when given, `comments`.
pub(crate) fn video_stats(
    video_id: &str,
    duration: &str,
    views: &str,
    comments: Option<&str>,
) -> Value {
    let mut video = stored("video-details.json")["items"][1].clone();
    video["etag"] = json!(format!("etag-{video_id}"));
    video["id"] = json!(video_id);
    video["contentDetails"]["duration"] = json!(duration);
    let statistics = &mut video["statistics"];
    statistics["viewCount"] = json!(views);
    match comments {
        Some(comments) => statistics["commentCount"] = json!(comments),
        None => {
            statistics
                .as_object_mut()
                .expect("statistics")
                .remove("commentCount");
        }
    }
    video
}

pub(crate) fn zoo_video(description: &str) -> Value {
    samples::zoo_video(description)
}

pub(crate) fn api_error(
    status: u16,
    reason: &str,
    info_reason: Option<&str>,
    message: &str,
) -> Response {
    Response::json(
        status,
        &samples::api_error(status, reason, info_reason, message).to_string(),
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

pub(crate) fn comment_threads_page(items: &[Value], next_page_token: Option<&str>) -> Response {
    json(&samples::comment_threads_page(items, next_page_token))
}

/// The stored first top-level comment as comment `id` by `author` saying `text`, with `likes`;
/// the author's channel ID is `UC` and the author name padded with `_` to 22 characters.
pub(crate) fn comment(id: &str, author: &str, text: &str, likes: u64) -> Value {
    let mut comment =
        stored("comment-threads.json")["items"][0]["snippet"]["topLevelComment"].clone();
    comment["etag"] = json!(format!("etag-{id}"));
    comment["id"] = json!(id);
    let snippet = &mut comment["snippet"];
    snippet["textDisplay"] = json!(text);
    snippet["textOriginal"] = json!(text);
    snippet["authorDisplayName"] = json!(format!("@{author}"));
    snippet["authorChannelUrl"] = json!(format!("http://www.youtube.com/@{author}"));
    snippet["authorChannelId"] = json!({"value": format!("UC{author:_<22}")});
    snippet["likeCount"] = json!(likes);
    comment
}

/// The stored first thread holding `top`, its reply count, and the replies it embeds.
pub(crate) fn comment_thread(top: &Value, total_reply_count: u64, replies: &[Value]) -> Value {
    let mut thread = stored("comment-threads.json")["items"][0].clone();
    thread["etag"] = json!(format!("thread-{}", top["id"].as_str().unwrap_or_default()));
    thread["id"] = top["id"].clone();
    thread["snippet"]["topLevelComment"] = top.clone();
    thread["snippet"]["totalReplyCount"] = json!(total_reply_count);
    let fields = thread.as_object_mut().expect("thread");
    if replies.is_empty() {
        fields.remove("replies");
    } else {
        fields.insert("replies".to_owned(), json!({"comments": replies}));
    }
    thread
}

/// The "Essence of linear algebra" playlist, which holds videos of its own channel.
pub(crate) const LINEAR_ALGEBRA: &str = "PLZHQObOWTQDPD3MizzM2xVFitgF8hE_ab";
pub(crate) const THREE_BLUE_ONE_BROWN: &str = "UCYO_jab_esuFRV4b17AJtAw";
pub(crate) const THREE_BLUE_ONE_BROWN_UPLOADS: &str = "UUYO_jab_esuFRV4b17AJtAw";

pub(crate) fn playlist_items_page(items: &[Value], next_page_token: Option<&str>) -> Response {
    json(&samples::playlist_items_page(items, next_page_token))
}

pub(crate) fn playlist_item(
    playlist: &str,
    position: u64,
    video_id: &str,
    title: &str,
    description: &str,
) -> Value {
    samples::playlist_item(playlist, position, video_id, title, description)
}

pub(crate) fn unreadable_playlist_item(
    playlist: &str,
    position: u64,
    video_id: &str,
    title: &str,
) -> Value {
    samples::unreadable_playlist_item(playlist, position, video_id, title)
}

pub(crate) fn channels_page(channels: &[(&str, &str)]) -> Response {
    json(&samples::channels_page(channels))
}
