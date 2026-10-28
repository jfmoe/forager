//! Reddit post and comment bodies to Markdown.
//!
//! Reddit renders a body's rich text as HTML: paragraphs, emphasis, links, inline and block code,
//! quotes, lists, headings, tables, rules, spoilers, and images. The shared markup cleaners fold
//! everything into plain paragraphs, so this converter keeps the structure.

use std::fmt::Write as _;

use scraper::{ElementRef, Node};

const SITE_ORIGIN: &str = "https://www.reddit.com";

/// Converts the element holding a Reddit body to Markdown; `None` when nothing remains.
pub(super) fn reddit_markdown(body: ElementRef<'_>) -> Option<String> {
    let markdown = blocks(body).join("\n\n");
    let markdown = markdown.trim();
    (!markdown.is_empty()).then(|| markdown.to_owned())
}

/// Renders the children of `element` as Markdown blocks; runs of inline content between block
/// elements become paragraphs.
fn blocks(element: ElementRef<'_>) -> Vec<String> {
    let mut rendered_blocks = Vec::new();
    let mut paragraph = String::new();
    for child in element.children() {
        let block = ElementRef::wrap(child).and_then(|child| match child.value().name() {
            "p" => Some(vec![inline_text(child)]),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => Some(vec![heading(child)]),
            "blockquote" => Some(vec![quote(child)]),
            "ul" | "ol" => Some(vec![list(child)]),
            "pre" => Some(vec![code_block(child)]),
            "hr" => Some(vec!["---".to_owned()]),
            "table" => Some(vec![table(child)]),
            "div" | "figure" | "section" | "article" => Some(blocks(child)),
            _ => None,
        });
        match block {
            Some(rendered) => {
                push_block(&mut rendered_blocks, &paragraph);
                paragraph.clear();
                for block in &rendered {
                    push_block(&mut rendered_blocks, block);
                }
            }
            None => match child.value() {
                Node::Text(text) => push_text(&mut paragraph, text),
                Node::Element(_) => {
                    if let Some(child) = ElementRef::wrap(child) {
                        inline_element(child, &mut paragraph);
                    }
                }
                _ => {}
            },
        }
    }
    push_block(&mut rendered_blocks, &paragraph);
    rendered_blocks
}

fn push_block(blocks: &mut Vec<String>, block: &str) {
    let block = block.trim();
    if !block.is_empty() {
        blocks.push(block.to_owned());
    }
}

fn heading(element: ElementRef<'_>) -> String {
    let level = element.value().name()[1..].parse().unwrap_or(1);
    format!("{} {}", "#".repeat(level), inline_text(element))
}

fn quote(element: ElementRef<'_>) -> String {
    blocks(element)
        .join("\n\n")
        .lines()
        .map(|line| {
            if line.is_empty() {
                ">".to_owned()
            } else {
                format!("> {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn list(element: ElementRef<'_>) -> String {
    let ordered = element.value().name() == "ol";
    let mut number = element
        .attr("start")
        .and_then(|start| start.parse::<u64>().ok())
        .unwrap_or(1);
    let mut items = Vec::new();
    for item in element
        .child_elements()
        .filter(|child| child.value().name() == "li")
    {
        let marker = if ordered {
            let marker = format!("{number}. ");
            number += 1;
            marker
        } else {
            "- ".to_owned()
        };
        let indent = " ".repeat(marker.len());
        let content = blocks(item).join("\n");
        let mut lines = content.lines();
        let mut rendered = format!("{marker}{}", lines.next().unwrap_or_default());
        for line in lines {
            rendered.push('\n');
            if !line.is_empty() {
                rendered.push_str(&indent);
                rendered.push_str(line);
            }
        }
        items.push(rendered);
    }
    items.join("\n")
}

fn code_block(element: ElementRef<'_>) -> String {
    let code = element.text().collect::<String>();
    let code = code.trim_end_matches('\n');
    let fence = "`".repeat(longest_backtick_run(code).max(2) + 1);
    format!("{fence}\n{code}\n{fence}")
}

fn table(element: ElementRef<'_>) -> String {
    let rows = element
        .descendent_elements()
        .filter(|row| row.value().name() == "tr")
        .map(|row| {
            row.child_elements()
                .filter(|cell| matches!(cell.value().name(), "th" | "td"))
                .map(|cell| inline_text(cell).replace('|', "\\|").replace('\n', " "))
                .collect::<Vec<_>>()
        })
        .filter(|cells| !cells.is_empty())
        .collect::<Vec<_>>();
    let Some(columns) = rows.iter().map(Vec::len).max() else {
        return String::new();
    };
    let line = |cells: &[String]| {
        let mut line = String::from("|");
        for column in 0..columns {
            let _ = write!(
                line,
                " {} |",
                cells.get(column).map(String::as_str).unwrap_or_default()
            );
        }
        line
    };
    let mut lines = vec![line(&rows[0]), format!("|{}", " --- |".repeat(columns))];
    lines.extend(rows[1..].iter().map(|row| line(row)));
    lines.join("\n")
}

/// Renders an element's inline content as one paragraph of text.
fn inline_text(element: ElementRef<'_>) -> String {
    let mut text = String::new();
    inline_children(element, &mut text);
    text.trim().to_owned()
}

fn inline_children(element: ElementRef<'_>, output: &mut String) {
    for child in element.children() {
        match child.value() {
            Node::Text(text) => push_text(output, text),
            Node::Element(_) => {
                if let Some(child) = ElementRef::wrap(child) {
                    inline_element(child, output);
                }
            }
            _ => {}
        }
    }
}

fn inline_element(element: ElementRef<'_>, output: &mut String) {
    match element.value().name() {
        "br" => {
            let trimmed = output.trim_end_matches(' ').len();
            output.truncate(trimmed);
            output.push('\n');
        }
        "strong" | "b" => wrap(output, element, "**", "**"),
        "em" | "i" => wrap(output, element, "*", "*"),
        "del" | "s" | "strike" => wrap(output, element, "~~", "~~"),
        "span" if has_class(element, "md-spoiler-text") => wrap(output, element, ">!", "!<"),
        "code" => code_span(output, &element.text().collect::<String>()),
        "a" => match element.attr("href").map(absolute_url) {
            Some(href) if inline_text(element).is_empty() || inline_text(element) == href => {
                surround(output, element, &format!("<{href}>"));
            }
            Some(href) => {
                surround(
                    output,
                    element,
                    &format!("[{}]({href})", inline_text(element)),
                );
            }
            None => inline_children(element, output),
        },
        "img" => {
            if let Some(src) = element.attr("src").map(absolute_url) {
                let alt = element.attr("alt").unwrap_or_default().trim();
                let _ = write!(output, "![{alt}]({src})");
            }
        }
        "script" | "style" | "template" | "svg" => {}
        _ => inline_children(element, output),
    }
}

/// Appends text with HTML whitespace folded to single spaces; a space never starts a line.
fn push_text(output: &mut String, text: &str) {
    let mut folded = String::with_capacity(text.len());
    let mut space = false;
    for character in text.chars() {
        if character.is_ascii_whitespace() {
            space = true;
            continue;
        }
        if space {
            folded.push(' ');
            space = false;
        }
        folded.push(character);
    }
    if space {
        folded.push(' ');
    }
    if output.is_empty() || output.ends_with([' ', '\n']) {
        output.push_str(folded.trim_start());
    } else {
        output.push_str(&folded);
    }
}

/// Writes an element's inline content between `open` and `close`.
fn wrap(output: &mut String, element: ElementRef<'_>, open: &str, close: &str) {
    let text = inline_text(element);
    if text.is_empty() {
        surround(output, element, "");
    } else {
        surround(output, element, &format!("{open}{text}{close}"));
    }
}

/// Writes `rendered` for an element, keeping the spaces at the edges of its text outside it, so
/// `In<a> our update</a>` stays two words.
fn surround(output: &mut String, element: ElementRef<'_>, rendered: &str) {
    let is_space = |character: char| character.is_ascii_whitespace();
    if element
        .text()
        .next()
        .is_some_and(|text| text.starts_with(is_space))
    {
        push_text(output, " ");
    }
    output.push_str(rendered);
    if element
        .text()
        .last()
        .is_some_and(|text| text.ends_with(is_space))
    {
        push_text(output, " ");
    }
}

fn code_span(output: &mut String, code: &str) {
    if code.is_empty() {
        return;
    }
    let fence = "`".repeat(longest_backtick_run(code) + 1);
    let padding = if code.starts_with('`') || code.ends_with('`') {
        " "
    } else {
        ""
    };
    let _ = write!(output, "{fence}{padding}{code}{padding}{fence}");
}

/// Resolves a site-relative link, such as `/r/rust/`, against the Reddit site.
fn absolute_url(href: &str) -> String {
    let href = href.trim();
    if href.starts_with('/') && !href.starts_with("//") {
        format!("{SITE_ORIGIN}{href}")
    } else {
        href.to_owned()
    }
}

fn has_class(element: ElementRef<'_>, class: &str) -> bool {
    element
        .attr("class")
        .is_some_and(|classes| classes.split_ascii_whitespace().any(|name| name == class))
}

fn longest_backtick_run(text: &str) -> usize {
    text.split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use scraper::{Html, Selector};

    use super::reddit_markdown;

    fn markdown(html: &str) -> Option<String> {
        let document = Html::parse_fragment(&format!("<div id=\"body\">{html}</div>"));
        let selector = Selector::parse("#body").expect("selector");
        reddit_markdown(document.select(&selector).next().expect("body"))
    }

    #[test]
    fn paragraphs_fold_their_whitespace_and_keep_emphasis_and_links() {
        let html = r#"
      <p>
      <strong>TL;DR:</strong> In<a rpl class="relative
  underline" href="https://www.reddit.com/r/redditdev/comments/1vgbm9c/plans/" rel="noopener nofollow ugc"> our last update</a>, we said we’d come back &amp; <em>more</em>.
    </p><p>
      See <a href="/r/science/">r/science</a> and <a href="https://example.com">https://example.com</a>.
    </p>"#;

        assert_eq!(
            markdown(html).as_deref(),
            Some(
                "**TL;DR:** In [our last update](https://www.reddit.com/r/redditdev/comments/1vgbm9c/plans/), we said we’d come back & *more*.\n\nSee [r/science](https://www.reddit.com/r/science/) and <https://example.com>."
            )
        );
    }

    #[test]
    fn lists_nest_and_number_from_their_start() {
        let html = "<ol start=\"3\"><li><p>First <code>a && b</code></p></li><li><p>Second</p><ul><li><p>Inner</p></li></ul></li></ol>";

        assert_eq!(
            markdown(html).as_deref(),
            Some("3. First `a && b`\n4. Second\n   - Inner")
        );
    }

    #[test]
    fn code_blocks_keep_their_whitespace() {
        let html =
            "<p>Try:</p><pre><code>fn main() {\n    println!(\"a &lt; b\");\n}\n</code></pre>";

        assert_eq!(
            markdown(html).as_deref(),
            Some("Try:\n\n```\nfn main() {\n    println!(\"a < b\");\n}\n```")
        );
    }

    #[test]
    fn quotes_headings_rules_and_strikethrough_keep_their_markers() {
        let html = "<h2>Title</h2><blockquote><p>Quoted</p><p>Again</p></blockquote><hr><p><del>old</del> new</p>";

        assert_eq!(
            markdown(html).as_deref(),
            Some("## Title\n\n> Quoted\n>\n> Again\n\n---\n\n~~old~~ new")
        );
    }

    #[test]
    fn tables_become_pipe_tables_without_empty_rows() {
        let html = r"<table><thead><tr><th><strong>Date</strong></th><th>What | happens</th></tr><tr></tr></thead>
      <tbody><tr><td>October 31</td><td>Requests close.</td></tr></tbody></table>";

        assert_eq!(
            markdown(html).as_deref(),
            Some(
                "| **Date** | What \\| happens |\n| --- | --- |\n| October 31 | Requests close. |"
            )
        );
    }

    #[test]
    fn spoilers_and_images_keep_their_meaning() {
        let html = r#"<p>It was <span class="md-spoiler-text">the butler</span>.</p><figure><img src="https://preview.redd.it/a.png" alt="chart"></figure>"#;

        assert_eq!(
            markdown(html).as_deref(),
            Some("It was >!the butler!<.\n\n![chart](https://preview.redd.it/a.png)")
        );
    }

    #[test]
    fn an_empty_body_yields_nothing() {
        assert_eq!([markdown(""), markdown("<p>  </p>")], [None, None]);
    }
}
