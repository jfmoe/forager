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
