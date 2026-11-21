//! Reddit discussion files: a fetched post and the comments its page loaded, as Markdown.

use std::fmt::Write as _;

use crate::types::{PlatformItem, PlatformItemData, RedditThreadOrder};

/// One comment of the page.
#[derive(Debug)]
pub(super) struct Comment {
    /// The comment's ID, without its `t1_` prefix.
    pub(super) id: String,
    /// The reply depth; top-level comments are at 0.
    pub(super) depth: usize,
    pub(super) author: Option<String>,
    pub(super) published: Option<String>,
    pub(super) score: Option<i64>,
    pub(super) permalink: Option<String>,
    /// The body as Markdown; `None` for a deleted or removed comment, which keeps its place so
    /// its replies keep their parent.
    pub(super) text: Option<String>,
}

/// Renders the discussion file: the post's metadata and text, then the page's comments indented
/// by depth with their author, score, time, and permalink, under a line that says they are the
/// first batch the page loaded, in which order they come, and how many of the post's comments
/// they are.
pub(super) fn render(
    item: &PlatformItem,
    comments: &[Comment],
    order: RedditThreadOrder,
) -> String {
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
        "\nThe first batch of comments the post page loaded, in {} order: {} of {} comments{}.",
        order_name(order),
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

/// How the discussion file names an order.
const fn order_name(order: RedditThreadOrder) -> &'static str {
    match order {
        RedditThreadOrder::Top => "Reddit's top",
        RedditThreadOrder::Best => "Reddit's best",
        RedditThreadOrder::New => "Reddit's new",
        RedditThreadOrder::Controversial => "Reddit's controversial",
        RedditThreadOrder::Old => "Reddit's old",
        RedditThreadOrder::Qa => "Reddit's Q&A",
    }
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
