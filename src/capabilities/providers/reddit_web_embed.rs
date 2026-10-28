//! Reddit embed pages: the page `embed.reddit.com` renders for a post under its subreddit.
//!
//! The page states the post in three places: a screen-view record (`shreddit-screenview-data`)
//! with the post's ID, creation time, type, adult flag, and URL; the rendered title, author, and
//! score; and the body's rich text. Under the wrong subreddit, or for a post it cannot show, the
//! page keeps the record but renders no post.

use chrono::DateTime;
use scraper::Html;
use serde::Deserialize;

use super::markup::reddit_markdown;
use super::{folded_text, selector, unexpected_page};
use crate::providers::shared::{Refusal, utc_second};
use crate::types::{AttemptErrorKind, RedditRef};

/// What a post's embed page says about the post.
pub(super) struct EmbedPost {
    pub(super) title: String,
    pub(super) author: Option<String>,
    pub(super) published: Option<String>,
    pub(super) post_type: Option<String>,
    /// The linked page or the shown media, when the post's URL is not its own page.
    pub(super) link_url: Option<String>,
    pub(super) score: Option<i64>,
    pub(super) nsfw: Option<bool>,
    pub(super) text: Option<String>,
}

#[derive(Deserialize)]
struct ScreenView {
    post: ScreenViewPost,
}

#[derive(Deserialize)]
struct ScreenViewPost {
    id: String,
    url: Option<String>,
    nsfw: Option<bool>,
    /// Milliseconds since the epoch.
    created_timestamp: Option<i64>,
    #[serde(rename = "type")]
    post_type: Option<String>,
}

/// Reads the post from its embed page.
pub(super) fn read_embed_page(html: &str, reference: &RedditRef) -> Result<EmbedPost, Refusal> {
    let document = Html::parse_document(html);
    let Some(record) = document
        .select(&selector("shreddit-screenview-data[data]"))
        .find_map(|element| element.attr("data"))
    else {
        return Err(unexpected_page(&document, "a Reddit embed page"));
    };
    let view = serde_json::from_str::<ScreenView>(record)
        .map_err(|error| Refusal::runtime(format!("invalid Reddit embed page record: {error}")))?;
    let fullname = format!("t3_{}", reference.id());
    if view.post.id != fullname {
        return Err(Refusal::runtime(format!(
            "the Reddit embed page shows {} instead of {fullname}",
            view.post.id
        )));
    }
    let title = document
        .select(&selector("#embed-title"))
        .next()
        .map(folded_text)
        .filter(|title| !title.is_empty());
    let (Some(title), Some(created)) = (title, view.post.created_timestamp) else {
        return Err(Refusal::new(
            AttemptErrorKind::Parameter,
            format!(
                "Reddit post reddit:{reference} is not available: its embed page shows no post; it may be removed, private, or restricted"
            ),
        ));
    };
    let permalink = document
        .select(&selector("#canonical-url-updater[value]"))
        .find_map(|element| element.attr("value"));
    let link_url = view
        .post
        .url
        .filter(|url| permalink.is_none_or(|permalink| !same_page(url, permalink)));
    let author = document
        .select(&selector(
            "[data-testid=\"embed-header\"] a[href*=\"/user/\"]",
        ))
        .next()
        .map(folded_text)
        .filter(|author| !author.is_empty() && author != "[deleted]");
    let score = document
        .select(&selector(
            "a[data-testid=\"upvote\"] faceplate-number[number]",
        ))
        .find_map(|element| element.attr("number")?.parse().ok());
    let text = document
        .select(&selector(&format!(
            "[id=\"{fullname}-post-rtjson-content\"]"
        )))
        .next()
        .and_then(reddit_markdown);
    Ok(EmbedPost {
        title,
        author,
        published: DateTime::from_timestamp_millis(created).map(utc_second),
        post_type: view.post.post_type,
        link_url,
        score,
        nsfw: view.post.nsfw,
        text,
    })
}

/// Returns whether two URLs name the same page, ignoring a trailing `/`.
fn same_page(left: &str, right: &str) -> bool {
    left.trim_end_matches('/') == right.trim_end_matches('/')
}
