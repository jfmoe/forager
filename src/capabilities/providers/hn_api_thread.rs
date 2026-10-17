//! Hacker News discussions: the Algolia item tree, its comment order, and its Markdown file.

use std::fmt::Write as _;

use serde::Deserialize;

use super::hn_markup::hn_markdown;
use crate::types::{HnItemData, HnThreadOrder, PlatformItem, PlatformItemData};

// Each comment level nests an object in an array. Real discussions stay far below 250 levels;
// the bound keeps decoding and rendering off the end of the stack.
const MAX_JSON_NESTING: usize = 500;

/// One node of an Algolia item tree. Algolia leaves deleted and dead comments out and orders
/// children by ID, which is posting time.
#[derive(Deserialize)]
pub(super) struct ThreadNode {
    pub(super) id: u64,
    #[serde(rename = "type")]
    pub(super) item_type: String,
    pub(super) author: Option<String>,
    pub(super) created_at_i: Option<i64>,
    pub(super) text: Option<String>,
    pub(super) title: Option<String>,
    pub(super) url: Option<String>,
    pub(super) points: Option<u64>,
    pub(super) parent_id: Option<u64>,
    pub(super) story_id: Option<u64>,
    #[serde(default)]
    children: Vec<ThreadNode>,
}

/// Decodes an Algolia item tree, rejecting nesting too deep to walk safely.
pub(super) fn decode_tree(body: &str) -> Result<ThreadNode, String> {
    if json_nesting(body) > MAX_JSON_NESTING {
        return Err(format!(
            "the discussion nests deeper than {} levels",
            MAX_JSON_NESTING / 2
        ));
    }
    let mut deserializer = serde_json::Deserializer::from_str(body);
    deserializer.disable_recursion_limit();
    let tree = ThreadNode::deserialize(&mut deserializer).map_err(|error| error.to_string())?;
    deserializer.end().map_err(|error| error.to_string())?;
    Ok(tree)
}

/// Returns the deepest object and array nesting outside strings.
fn json_nesting(body: &str) -> usize {
    let (mut depth, mut deepest) = (0_usize, 0_usize);
    let (mut in_string, mut escaped) = (false, false);
    for byte in body.bytes() {
        if in_string {
            match (escaped, byte) {
                (true, _) => escaped = false,
                (false, b'\\') => escaped = true,
                (false, b'"') => in_string = false,
                _ => {}
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth += 1;
                deepest = deepest.max(depth);
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    deepest
}

/// A discussion tree in delivery order.
pub(super) struct ThreadRoot {
    node: ThreadNode,
}

impl ThreadRoot {
    /// Orders the tree. With the root's `ranked` replies, top-level comments follow that rank:
    /// replies the tree lacks (not yet indexed) are skipped and replies the rank lacks come
    /// last by time. Without it, and below the top level, comments are in time order.
    pub(super) fn new(mut node: ThreadNode, ranked: Option<&[u64]>) -> (Self, HnThreadOrder) {
        for child in &mut node.children {
            sort_by_time(child);
        }
        node.children.sort_by_key(|child| child.id);
        let order = match ranked {
            Some(ranked) => {
                let mut rest = std::mem::take(&mut node.children);
                let mut ordered = Vec::with_capacity(rest.len());
                for id in ranked {
                    if let Some(position) = rest.iter().position(|child| child.id == *id) {
                        ordered.push(rest.remove(position));
                    }
                }
                ordered.append(&mut rest);
                node.children = ordered;
                HnThreadOrder::RankedTopLevel
            }
            None => HnThreadOrder::Chronological,
        };
        (Self { node }, order)
    }

    pub(super) fn node(&self) -> &ThreadNode {
        &self.node
    }

    /// Returns how many comments the discussion holds below the root.
    pub(super) fn comment_count(&self) -> usize {
        count(&self.node) - 1
    }

    /// Renders the discussion file: the root's metadata and body, then every comment indented
    /// by depth with its author, time, and ref.
    pub(super) fn render(&self, item: &PlatformItem) -> String {
        let PlatformItemData::Hn(data) = &item.data else {
            unreachable!("the hn route builds hn items")
        };
        let mut markdown = String::new();
        let author = item.authors.first().map(String::as_str);
        match &item.title {
            Some(title) => {
                let _ = writeln!(markdown, "# {title}\n");
            }
            None => {
                let _ = writeln!(
                    markdown,
                    "# Comment by {}\n",
                    author.unwrap_or("an unknown user")
                );
            }
        }
        let _ = writeln!(markdown, "- Ref: `{}` <{}>", item.reference, item.url);
        write_metadata(&mut markdown, item, data);
        if let Some(text) = &data.text {
            let _ = writeln!(markdown, "\n{text}");
        }
        markdown.push_str("\n## Comments\n");
        if self.node.children.is_empty() {
            markdown.push_str("\nNo comments.\n");
        }
        for child in &self.node.children {
            write_comment(&mut markdown, child, 0);
        }
        markdown
    }
}

fn write_metadata(markdown: &mut String, item: &PlatformItem, data: &HnItemData) {
    if let Some(link) = &data.link_url {
        let _ = writeln!(markdown, "- Link: <{link}>");
    }
    if let Some(story) = &data.story_ref {
        match &data.story_title {
            Some(title) => {
                let _ = writeln!(markdown, "- Story: {title} `{story}`");
            }
            None => {
                let _ = writeln!(markdown, "- Story: `{story}`");
            }
        }
    }
    if let Some(author) = item.authors.first() {
        let _ = writeln!(markdown, "- Author: {author}");
    }
    if let Some(points) = data.points {
        let _ = writeln!(markdown, "- Points: {points}");
    }
    if let Some(comments) = data.comments {
        let _ = writeln!(markdown, "- Comments: {comments}");
    }
    if let Some(published) = &item.published {
        let _ = writeln!(markdown, "- Published: {published}");
    }
}

fn write_comment(markdown: &mut String, node: &ThreadNode, depth: usize) {
    let indent = "  ".repeat(depth);
    let published = super::published(node.created_at_i);
    let _ = writeln!(
        markdown,
        "\n{indent}- **{}** · {} · `hn:{}`\n",
        node.author.as_deref().unwrap_or("[unknown]"),
        published.as_deref().unwrap_or("undated"),
        node.id
    );
    let text = node
        .text
        .as_deref()
        .and_then(hn_markdown)
        .unwrap_or_else(|| "*[no text]*".to_owned());
    for line in text.lines() {
        if line.is_empty() {
            markdown.push('\n');
        } else {
            let _ = writeln!(markdown, "{indent}  {line}");
        }
    }
    for child in &node.children {
        write_comment(markdown, child, depth + 1);
    }
}

fn sort_by_time(node: &mut ThreadNode) {
    node.children.sort_by_key(|child| child.id);
    for child in &mut node.children {
        sort_by_time(child);
    }
}

fn count(node: &ThreadNode) -> usize {
    1 + node.children.iter().map(count).sum::<usize>()
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::{MAX_JSON_NESTING, decode_tree, json_nesting};

    #[test]
    fn nesting_ignores_brackets_inside_strings() {
        assert_eq!(json_nesting(r#"{"a":[{"text":"[[{{\"]]"}]}"#), 3);
    }

    #[test]
    fn a_tree_deeper_than_the_bound_is_rejected_before_decoding() {
        let body = "[".repeat(MAX_JSON_NESTING + 1);

        assert_eq!(
            decode_tree(&body).err(),
            Some("the discussion nests deeper than 250 levels".to_owned())
        );
    }

    #[test]
    fn a_tree_deeper_than_the_default_serde_limit_decodes() {
        let levels = 100;
        let mut body = String::new();
        for id in 1..=levels {
            let _ = write!(
                body,
                r#"{{"id":{id},"type":"comment","author":"a","created_at_i":1,"text":"t","children":["#
            );
        }
        body.push_str(&"]}".repeat(levels));

        let tree = decode_tree(&body).expect("decode deep tree");

        let mut depth = 1;
        let mut node = &tree;
        while let Some(child) = node.children.first() {
            depth += 1;
            node = child;
        }
        assert_eq!(depth, levels);
    }
}
