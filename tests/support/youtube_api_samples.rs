//! YouTube Data API bodies derived from the stored samples in `tests/samples/youtube_api/`, shared
//! by the command contract tests and, through the route's in-crate tests, the crate; the
//! including module provides the sample store's reader as `samples`. A variant changes only what
//! its test is about, so every other field stays as the sample has it.

use serde_json::{Value, json};

use super::samples::sample;

/// The stored body `name`, as the Data API answered it.
pub(crate) fn stored(name: &str) -> Value {
    serde_json::from_str(&sample(&format!("youtube_api/{name}")))
        .unwrap_or_else(|error| panic!("stored youtube_api/{name}: {error}"))
}

/// The stored list response `name` holding `items`, with `next_page_token` when another page
/// exists.
fn page(name: &str, items: &[Value], next_page_token: Option<&str>) -> Value {
    let mut page = stored(name);
    page["items"] = json!(items);
    page["pageInfo"]["resultsPerPage"] = json!(items.len());
    let fields = page.as_object_mut().expect("list response");
    match next_page_token {
        Some(token) => {
            fields.insert("nextPageToken".to_owned(), json!(token));
        }
        None => {
            fields.remove("nextPageToken");
        }
    }
    page
}

/// The stored `search.list` page holding `items`.
pub(crate) fn search_page(items: &[Value], next_page_token: Option<&str>) -> Value {
    page("search.json", items, next_page_token)
}

/// The stored `videos.list` response holding `items`.
pub(crate) fn videos_page(items: &[Value]) -> Value {
    let mut page = page("video.json", items, None);
    page["pageInfo"]["totalResults"] = json!(items.len());
    page
}

/// The stored video, "Me at the zoo", with `description` as its description.
pub(crate) fn zoo_video(description: &str) -> Value {
    let mut video = stored("video.json")["items"][0].clone();
    video["snippet"]["description"] = json!(description);
    video["snippet"]["localized"]["description"] = json!(description);
    video
}

/// The stored `commentThreads.list` page holding `items`.
pub(crate) fn comment_threads_page(items: &[Value], next_page_token: Option<&str>) -> Value {
    let mut page = page("comment-threads.json", items, next_page_token);
    page["pageInfo"]["totalResults"] = json!(items.len());
    page
}

/// The stored `playlistItems.list` page holding `items`.
pub(crate) fn playlist_items_page(items: &[Value], next_page_token: Option<&str>) -> Value {
    page("playlist-items.json", items, next_page_token)
}

/// The stored first playlist entry as entry `position` of `playlist`, the video `video_id`
/// titled `title` with `description`.
pub(crate) fn playlist_item(
    playlist: &str,
    position: u64,
    video_id: &str,
    title: &str,
    description: &str,
) -> Value {
    let mut item = stored("playlist-items.json")["items"][0].clone();
    item["etag"] = json!(format!("etag-{video_id}"));
    item["id"] = json!(format!("item-{position}"));
    let snippet = &mut item["snippet"];
    snippet["title"] = json!(title);
    snippet["description"] = json!(description);
    snippet["playlistId"] = json!(playlist);
    snippet["position"] = json!(position);
    snippet["resourceId"]["videoId"] = json!(video_id);
    item["contentDetails"]["videoId"] = json!(video_id);
    item
}
