//! YouTube configuration for `youtube_api` and Data API records derived from the stored samples,
//! shared by the `platform youtube` tests.

use serde_json::{Value, json};

use super::samples::sample_json;

/// The fixture API key; tests assert it never leaves the request header.
pub(crate) const KEY: &str = "yt-fixture-key-0123456789";

/// Points `youtube_api` at the fixture server with one key and one attempt per request.
pub(crate) fn config(url: &str) -> String {
    format!(
        "[providers.youtube_api]\nurl = \"{url}/youtube/v3\"\nkeys = [\"{KEY}\"]\n\n[retry]\nmax_attempts = 1\n"
    )
}

/// The stored first search result as the video `video_id` titled `title` with `description`.
pub(crate) fn search_result(video_id: &str, title: &str, description: &str) -> Value {
    let mut result = sample_json("youtube_api/search.json")["items"][0].clone();
    result["etag"] = json!(format!("etag-{video_id}"));
    result["id"]["videoId"] = json!(video_id);
    let snippet = &mut result["snippet"];
    snippet["title"] = json!(title);
    snippet["description"] = json!(description);
    snippet["thumbnails"]["default"]["url"] =
        json!(format!("https://i.ytimg.com/vi/{video_id}/default.jpg"));
    result
}

/// The stored details of a commented video as those of `video_id`, lasting `duration`, with
/// `views` and, when given, `comments`.
pub(crate) fn video_stats(
    video_id: &str,
    duration: &str,
    views: &str,
    comments: Option<&str>,
) -> Value {
    let mut video = sample_json("youtube_api/video-details.json")["items"][1].clone();
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

/// The stored first top-level comment as comment `id` by `author` saying `text`, with `likes`;
/// the author's channel ID is `UC` and the author name padded with `_` to 22 characters.
pub(crate) fn comment(id: &str, author: &str, text: &str, likes: u64) -> Value {
    let mut comment =
        sample_json("youtube_api/comment-threads.json")["items"][0]["snippet"]["topLevelComment"]
            .clone();
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
    let mut thread = sample_json("youtube_api/comment-threads.json")["items"][0].clone();
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
