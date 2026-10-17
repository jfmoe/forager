//! Hacker News body HTML to Markdown.
//!
//! Hacker News stores item bodies as a small HTML subset: `<p>` separates paragraphs, `<a>`
//! links, `<i>` italics, and `<pre><code>` code blocks whose whitespace matters. The shared
//! markup cleaners fold whitespace and drop links, so this converter keeps both.

use std::fmt::Write as _;

use super::super::markup::decode_entities;

/// Converts a Hacker News body to Markdown; `None` when nothing remains.
pub(super) fn hn_markdown(html: &str) -> Option<String> {
    let mut converter = Converter::default();
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        converter.text(&rest[..start]);
        let Some(end) = rest[start..].find('>') else {
            converter.text(&rest[start..]);
            rest = "";
            break;
        };
        converter.tag(&rest[start + 1..start + end]);
        rest = &rest[start + end + 1..];
    }
    converter.text(rest);
    let markdown = converter.finish();
    (!markdown.is_empty()).then_some(markdown)
}

#[derive(Default)]
struct Converter {
    output: String,
    /// The open link's href and the text read inside it so far.
    link: Option<(String, String)>,
    in_pre: bool,
}

impl Converter {
    fn text(&mut self, raw: &str) {
        if raw.is_empty() {
            return;
        }
        let decoded = decode_entities(raw);
        match &mut self.link {
            Some((_, text)) => text.push_str(&decoded),
            None => self.output.push_str(&decoded),
        }
    }

    fn tag(&mut self, inner: &str) {
        let inner = inner.trim();
        let (closing, inner) = inner
            .strip_prefix('/')
            .map_or((false, inner), |rest| (true, rest));
        let name_end = inner
            .find(|character: char| character.is_whitespace() || character == '/')
            .unwrap_or(inner.len());
        let name = inner[..name_end].to_ascii_lowercase();
        match (name.as_str(), closing) {
            ("p", false) => self.paragraph_break(),
            ("i" | "em", _) if !self.in_pre => self.text_marker("*"),
            ("pre", false) => {
                self.paragraph_break();
                self.output.push_str("```\n");
                self.in_pre = true;
            }
            ("pre", true) => {
                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
                self.output.push_str("```");
                self.in_pre = false;
                self.paragraph_break();
            }
            ("a", false) => {
                self.close_link();
                self.link = Some((
                    attribute(&inner[name_end..], "href").unwrap_or_default(),
                    String::new(),
                ));
            }
            ("a", true) => self.close_link(),
            _ => {}
        }
    }

    fn text_marker(&mut self, marker: &str) {
        match &mut self.link {
            Some((_, text)) => text.push_str(marker),
            None => self.output.push_str(marker),
        }
    }

    fn paragraph_break(&mut self) {
        self.close_link();
        let trimmed = self.output.trim_end_matches([' ', '\n']).len();
        self.output.truncate(trimmed);
        if !self.output.is_empty() {
            self.output.push_str("\n\n");
        }
    }

    /// Writes the open link. Hacker News shortens long link text with `...`, so text that is
    /// a prefix of the href is replaced by the href itself.
    fn close_link(&mut self) {
        let Some((href, text)) = self.link.take() else {
            return;
        };
        let shown = text.trim();
        let prefix = shown.trim_end_matches("...");
        if href.is_empty() {
            self.output.push_str(shown);
        } else if prefix.is_empty() || href.starts_with(prefix) {
            self.output.push_str(&href);
        } else {
            let _ = write!(self.output, "[{shown}]({href})");
        }
    }

    fn finish(mut self) -> String {
        self.close_link();
        if self.in_pre {
            self.output.push_str("\n```");
        }
        self.output.trim().to_owned()
    }
}

/// Returns the entity-decoded value of a double- or single-quoted attribute.
fn attribute(attributes: &str, name: &str) -> Option<String> {
    let mut rest = attributes;
    while let Some(index) = rest.find('=') {
        let key = rest[..index].trim().rsplit(char::is_whitespace).next()?;
        let value = rest[index + 1..].trim_start();
        let quote = value.chars().next()?;
        let (value, after) = if matches!(quote, '"' | '\'') {
            let end = value[1..].find(quote)? + 1;
            (&value[1..end], &value[end + 1..])
        } else {
            let end = value.find(char::is_whitespace).unwrap_or(value.len());
            (&value[..end], &value[end..])
        };
        if key.eq_ignore_ascii_case(name) {
            return Some(decode_entities(value));
        }
        rest = after;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::hn_markdown;

    #[test]
    fn paragraphs_become_blank_line_separated() {
        assert_eq!(
            hn_markdown("First line.<p>Second paragraph.<p>Third.</p>").as_deref(),
            Some("First line.\n\nSecond paragraph.\n\nThird.")
        );
    }

    #[test]
    fn links_keep_their_href_and_shortened_text_gives_way_to_it() {
        let html = concat!(
            r#"See <a href="https:&#x2F;&#x2F;example.com&#x2F;a&#x2F;very&#x2F;long&#x2F;path" rel="nofollow">https:&#x2F;&#x2F;example.com&#x2F;a&#x2F;very&#x2F;l...</a>"#,
            r#" and <a href="https://rust-lang.org/">the Rust site</a>"#,
            r#" or <a href="https://example.org">https://example.org</a>."#
        );

        assert_eq!(
            hn_markdown(html).as_deref(),
            Some(
                "See https://example.com/a/very/long/path and [the Rust site](https://rust-lang.org/) or https://example.org."
            )
        );
    }

    #[test]
    fn italics_become_emphasis() {
        assert_eq!(
            hn_markdown("This is <i>really</i> it.").as_deref(),
            Some("This is *really* it.")
        );
    }

    #[test]
    fn code_blocks_keep_their_whitespace_and_decode_entities() {
        let html = "Try:<p><pre><code>  fn main() {\n      println!(&quot;a &lt; b&quot;);\n  }\n</code></pre>Done.";

        assert_eq!(
            hn_markdown(html).as_deref(),
            Some("Try:\n\n```\n  fn main() {\n      println!(\"a < b\");\n  }\n```\n\nDone.")
        );
    }

    #[test]
    fn entities_decode_and_stray_ampersands_survive() {
        assert_eq!(
            hn_markdown("it&#x27;s &quot;fine&quot; &amp; &#34;ok&#34; &lt;3 AT&T").as_deref(),
            Some("it's \"fine\" & \"ok\" <3 AT&T")
        );
    }

    #[test]
    fn an_empty_body_yields_nothing() {
        assert_eq!([hn_markdown(""), hn_markdown("<p> </p>")], [None, None]);
    }
}
