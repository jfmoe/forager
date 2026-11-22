//! Relative Markdown links in the scanned documents must resolve.

use std::path::Path;

/// Relative link targets in `markdown` that `exists` rejects, resolved against `directory`.
///
/// Inline links, images, and reference definitions are checked. Fenced blocks and inline code
/// are skipped, a `#fragment` is stripped, and external links (anything with a URL scheme) and
/// pure anchors are not checked.
pub fn broken_links(
    markdown: &str,
    directory: &Path,
    exists: impl Fn(&Path) -> bool,
) -> Vec<String> {
    link_targets(&prose(markdown))
        .into_iter()
        .filter_map(|target| {
            let path = target.split('#').next().unwrap_or_default();
            let checked = !path.is_empty() && !has_scheme(path);
            (checked && !exists(&directory.join(path))).then_some(target)
        })
        .collect()
}

/// `markdown` without fenced blocks and inline code spans.
fn prose(markdown: &str) -> String {
    let mut text = String::new();
    let mut fence: Option<&str> = None;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        let marker = ["```", "~~~"]
            .into_iter()
            .find(|marker| trimmed.starts_with(marker));
        match (fence, marker) {
            (None, Some(marker)) => fence = Some(marker),
            (Some(open), Some(marker)) if open == marker => fence = None,
            (None, None) => {
                text.push_str(&without_code_spans(line));
                text.push('\n');
            }
            _ => {}
        }
    }
    text
}

/// `line` with every backtick-delimited code span removed. A run of backticks opens a span that
/// only a run of the same length closes; an unclosed run stays as text.
fn without_code_spans(line: &str) -> String {
    let mut text = String::new();
    let mut rest = line;
    while let Some(start) = rest.find('`') {
        text.push_str(&rest[..start]);
        let run = rest[start..].len() - rest[start..].trim_start_matches('`').len();
        let after = &rest[start + run..];
        if let Some(end) = closing_run(after, run) {
            rest = &after[end + run..];
        } else {
            text.push_str(&rest[start..start + run]);
            rest = after;
        }
    }
    text.push_str(rest);
    text
}

/// Byte offset in `text` of the first run of exactly `run` backticks.
fn closing_run(text: &str, run: usize) -> Option<usize> {
    let mut offset = 0;
    while let Some(found) = text[offset..].find('`') {
        let start = offset + found;
        let length = text[start..].len() - text[start..].trim_start_matches('`').len();
        if length == run {
            return Some(start);
        }
        offset = start + length;
    }
    None
}

/// The destination of every inline link, image, and reference definition, without its optional
/// title.
fn link_targets(prose: &str) -> Vec<String> {
    let inline = prose.split("](").skip(1);
    let definitions = prose.lines().filter_map(reference_definition);
    inline
        .chain(definitions)
        .filter_map(|rest| {
            let destination = match rest.strip_prefix('<') {
                Some(bracketed) => bracketed.split('>').next()?,
                None => rest.split([')', ' ']).next()?,
            };
            Some(destination.to_owned())
        })
        .collect()
}

/// What follows the label of a reference definition such as `[1]: path "title"`, indented by at
/// most three spaces.
fn reference_definition(line: &str) -> Option<&str> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    let (label, rest) = line
        .trim_start_matches(' ')
        .strip_prefix('[')?
        .split_once("]:")?;
    (indent <= 3 && !label.is_empty()).then(|| rest.trim_start())
}

/// Whether `target` starts with a URL scheme such as `https:` or `mailto:`.
fn has_scheme(target: &str) -> bool {
    target.split_once(':').is_some_and(|(scheme, _)| {
        !scheme.is_empty()
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn broken(markdown: &str) -> Vec<String> {
        let existing = [PathBuf::from("docs/a.md"), PathBuf::from("docs/sub")];
        broken_links(markdown, Path::new("docs"), |path| {
            existing.iter().any(|known| known == path)
        })
    }

    #[test]
    fn reports_a_relative_link_to_a_missing_file() {
        assert_eq!(
            broken("See [b](b.md) and ![x](img/x.png)."),
            ["b.md", "img/x.png"]
        );
    }

    #[test]
    fn accepts_files_directories_and_fragments_that_resolve() {
        assert_eq!(
            broken("[a](a.md), [s](sub), [f](a.md#part), [t](a.md \"title\"), [p](<a.md>)"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn checks_reference_definitions() {
        let markdown = "[x][1] and [y][2]\n\n[1]: missing.md\n   [2]: <a.md> \"title\"\n[3]: https://example.com";
        assert_eq!(broken(markdown), ["missing.md"]);
    }

    #[test]
    fn skips_code_external_links_and_anchors() {
        let markdown = "`[c](missing.md)` and ``[d](`x`)``\n\
                        ```text\n[e](missing.md)\n```\n\
                        [web](https://example.com/x) [mail](mailto:a@b.c) [here](#top)";
        assert_eq!(broken(markdown), Vec::<String>::new());
    }
}
