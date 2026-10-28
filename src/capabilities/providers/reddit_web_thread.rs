//! Reddit discussions: the comment-tree fragment of a post and its Markdown file.
//!
//! The fragment holds the post's first page of comments in Reddit's `top` order, each a
//! `shreddit-comment` element in reading order with its depth, author, score, and time; deeper
//! and later comments load from `more-comments` fragments, which the route does not follow. The
//! tree element names the post's permalink and its whole comment count.

use std::collections::HashMap;
use std::fmt::Write as _;

use scraper::Html;

use super::markup::reddit_markdown;
use super::{SITE_ORIGIN, selector, site_timestamp, unexpected_page};
use crate::providers::shared::Refusal;
use crate::types::{AttemptErrorKind, PlatformItem, PlatformItemData, RedditRef};

// The fragment of a post that does not exist renders the comment route without a tree.
const COMMENT_ROUTE_MARKER: &str = "routeName=\"top_comments\"";
const MORE_COMMENTS_PATH: &str = "/svc/shreddit/more-comments/";

/// The first page of a post's comments, with what the tree says about the post.
pub(super) struct CommentsPage {
    /// The post page path with its subreddit and slug, such as `/r/rust/comments/<id>/<slug>/`.
    pub(super) permalink_path: String,
    pub(super) subreddit: String,
    /// The post's whole comment count.
    pub(super) total: u64,
    /// The page's comments in reading order.
    pub(super) comments: Vec<Comment>,
    /// Whether the page leaves comments to load later.
    pub(super) has_more: bool,
}

/// One comment of the page.
pub(super) struct Comment {
    /// The comment's ID, without its `t1_` prefix.
    id: String,
    /// The reply depth; top-level comments are at 0.
    depth: usize,
    author: Option<String>,
    published: Option<String>,
    score: Option<i64>,
    permalink: Option<String>,
    /// The body as Markdown; `None` for a deleted or removed comment, which keeps its place so
    /// its replies keep their parent.
    text: Option<String>,
}

/// Reads the comment tree of `reference` from its fragment.
pub(super) fn read_comments_page(
    html: &str,
    reference: &RedditRef,
) -> Result<CommentsPage, Refusal> {
    let document = Html::parse_document(html);
    let Some(tree) = document
        .select(&selector("shreddit-comment-tree#comment-tree"))
        .next()
    else {
        if html.contains(COMMENT_ROUTE_MARKER) {
            return Err(Refusal::new(
                AttemptErrorKind::Parameter,
                format!("Reddit post not found: reddit:{reference}"),
            ));
        }
        return Err(unexpected_page(&document, "a Reddit comment page"));
    };
    let fullname = format!("t3_{}", reference.id());
    if tree.attr("post-id") != Some(fullname.as_str()) {
        return Err(Refusal::runtime(format!(
            "the Reddit comment page shows {} instead of {fullname}",
            tree.attr("post-id").unwrap_or("no post")
        )));
    }
    let permalink_path = tree.attr("permalink").unwrap_or_default();
    let subreddit = subreddit(permalink_path, reference).ok_or_else(|| {
        Refusal::runtime(format!(
            "the Reddit comment page gives no post page for {fullname}: `{permalink_path}`"
        ))
    })?;
    let total = tree
        .attr("totalcomments")
        .and_then(|count| count.parse().ok())
        .ok_or_else(|| {
            Refusal::runtime(format!(
                "the Reddit comment page gives no comment count for {fullname}"
            ))
        })?;
    let bodies = document
        .select(&selector(
            "[slot=\"comment\"][id$=\"-comment-rtjson-content\"]",
        ))
        .filter_map(|body| {
            let id = body.attr("id")?.strip_suffix("-comment-rtjson-content")?;
            Some((id.to_owned(), reddit_markdown(body)))
        })
        .collect::<HashMap<_, _>>();
    let mut comments = Vec::new();
    for element in document.select(&selector("shreddit-comment[thingid]")) {
        let Some(thing) = element.attr("thingid") else {
            continue;
        };
        let Some(id) = thing.strip_prefix("t1_") else {
            continue;
        };
        let deleted = element.attr("is-comment-deleted").is_some();
        let author = element
            .attr("author")
            .filter(|author| {
                element.attr("is-author-deleted").is_none() && !author.starts_with('[')
            })
            .map(str::to_owned);
        comments.push(Comment {
            id: id.to_owned(),
            depth: element
                .attr("depth")
                .and_then(|depth| depth.parse().ok())
                .unwrap_or_default(),
            author,
            published: element.attr("created").and_then(site_timestamp),
            score: element.attr("score").and_then(|score| score.parse().ok()),
            permalink: element
                .attr("permalink")
                .map(|path| format!("{SITE_ORIGIN}{path}")),
            text: (!deleted)
                .then(|| bodies.get(thing).cloned().flatten())
                .flatten(),
        });
    }
    let has_more = document
        .select(&selector("faceplate-partial[src]"))
        .filter_map(|partial| partial.attr("src"))
        .any(|src| src.contains(MORE_COMMENTS_PATH));
    Ok(CommentsPage {
        permalink_path: permalink_path.to_owned(),
        subreddit,
        total,
        comments,
        has_more,
    })
}

/// Returns the subreddit of a post page path such as `/r/<sub>/comments/<id>/<slug>/`; a post
/// on a user profile belongs to the profile's `u_<name>` subreddit.
fn subreddit(path: &str, reference: &RedditRef) -> Option<String> {
    let segments = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    match segments.as_slice() {
        ["r", subreddit, "comments", id, ..] if *id == reference.id() => {
            Some((*subreddit).to_owned())
        }
        ["user", name, "comments", id, ..] if *id == reference.id() => Some(format!("u_{name}")),
        _ => None,
    }
}

/// Renders the discussion file: the post's metadata and text, then the page's comments indented
/// by depth with their author, score, time, and permalink.
pub(super) fn render(item: &PlatformItem, comments: &[Comment]) -> String {
    let PlatformItemData::Reddit(data) = &item.data else {
        unreachable!("the reddit route builds reddit items")
    };
    let mut markdown = format!(
        "# {}\n\n- Ref: `{}` <{}>\n",
        item.title.as_deref().unwrap_or_default(),
        item.reference,
        item.url
    );
    if let Some(permalink) = &data.permalink {
        let _ = writeln!(markdown, "- Permalink: <{permalink}>");
    }
    if let Some(subreddit) = &data.subreddit {
        let _ = writeln!(markdown, "- Subreddit: r/{subreddit}");
    }
    let author = item
        .authors
        .first()
        .map_or_else(|| "[deleted]".to_owned(), |author| format!("u/{author}"));
    let _ = writeln!(markdown, "- Author: {author}");
    if let Some(link) = &data.link_url {
        let _ = writeln!(markdown, "- Link: <{link}>");
    }
    if let Some(score) = data.score {
        let _ = writeln!(markdown, "- Score: {score}");
    }
    if let Some(total) = data.comments {
        let _ = writeln!(markdown, "- Comments: {total}");
    }
    if let Some(published) = &item.published {
        let _ = writeln!(markdown, "- Published: {published}");
    }
    if let Some(text) = &data.text {
        let _ = writeln!(markdown, "\n{text}");
    }
    markdown.push_str("\n## Comments\n");
    if comments.is_empty() {
        markdown.push_str("\nNo comments.\n");
        return markdown;
    }
    let _ = writeln!(
        markdown,
        "\nThe first page of comments in Reddit's top order: {} of {} comments{}.",
        comments.len(),
        data.comments.unwrap_or_default(),
        if data.has_more_comments == Some(true) {
            "; Reddit has more that this file does not hold"
        } else {
            ""
        }
    );
    for comment in comments {
        write_comment(&mut markdown, comment);
    }
    markdown
}

fn write_comment(markdown: &mut String, comment: &Comment) {
    let indent = "  ".repeat(comment.depth);
    let author = comment
        .author
        .as_ref()
        .map_or_else(|| "[deleted]".to_owned(), |author| format!("u/{author}"));
    let mut heading = format!("**{author}**");
    if let Some(score) = comment.score {
        let _ = write!(heading, " · {score} points");
    }
    if let Some(published) = &comment.published {
        let _ = write!(heading, " · {published}");
    }
    match &comment.permalink {
        Some(permalink) => {
            let _ = write!(heading, " · [t1_{}]({permalink})", comment.id);
        }
        None => {
            let _ = write!(heading, " · `t1_{}`", comment.id);
        }
    }
    let _ = writeln!(markdown, "\n{indent}- {heading}\n");
    let text = comment.text.as_deref().unwrap_or("*[deleted or removed]*");
    for line in text.lines() {
        if line.is_empty() {
            markdown.push('\n');
        } else {
            let _ = writeln!(markdown, "{indent}  {line}");
        }
    }
}
