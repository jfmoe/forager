//! Plain text from the HTML or JATS markup that platform APIs put in titles, abstracts, and
//! snippets.

// JATS and HTML elements that end a paragraph; every other element is inline.
const BLOCK_ELEMENTS: [&str; 7] = ["p", "title", "sec", "list", "list-item", "br", "div"];

/// Splits JATS or HTML markup into plain-text paragraphs: tags are removed, block
/// elements end a paragraph, entities are decoded, and whitespace is folded.
pub(super) fn markup_paragraphs(markup: &str) -> Vec<String> {
    let mut paragraphs = Vec::new();
    let mut paragraph = String::new();
    let mut rest = markup;
    while let Some(start) = rest.find('<') {
        paragraph.push_str(&decode_entities(&rest[..start]));
        let tail = &rest[start..];
        if let Some(cdata) = tail.strip_prefix("<![CDATA[") {
            let end = cdata.find("]]>").unwrap_or(cdata.len());
            paragraph.push_str(&cdata[..end]);
            rest = cdata.get(end + 3..).unwrap_or_default();
        } else if let Some(end) = tag_end(tail) {
            if BLOCK_ELEMENTS.contains(&element_name(&tail[1..end]).as_str()) {
                push_paragraph(&mut paragraphs, &mut paragraph);
            }
            rest = &tail[end + 1..];
        } else {
            paragraph.push('<');
            rest = &tail[1..];
        }
    }
    paragraph.push_str(&decode_entities(rest));
    push_paragraph(&mut paragraphs, &mut paragraph);
    paragraphs
}

/// Returns the index of the `>` that closes a tag at the start of `text`, skipping quoted
/// attribute values of elements; a `<` that does not start a tag, such as in `p < 0.05`, is text.
fn tag_end(text: &str) -> Option<usize> {
    let next = text[1..].chars().next()?;
    if matches!(next, '!' | '?') {
        return text.find('>');
    }
    if !(next.is_ascii_alphabetic() || next == '/') {
        return None;
    }
    let mut quote = None;
    for (index, character) in text.char_indices().skip(1) {
        match (quote, character) {
            (Some(open), _) if character == open => quote = None,
            (None, '"' | '\'') => quote = Some(character),
            (None, '>') => return Some(index),
            _ => {}
        }
    }
    None
}

/// Returns the lowercase local name of a tag body such as `/jats:p` or `br /`.
fn element_name(tag: &str) -> String {
    let name = tag
        .trim_start_matches('/')
        .split(|character: char| character.is_whitespace() || character == '/')
        .next()
        .unwrap_or_default();
    name.rsplit(':')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn push_paragraph(paragraphs: &mut Vec<String>, paragraph: &mut String) {
    let text = fold_whitespace(paragraph);
    if !text.is_empty() {
        paragraphs.push(text);
    }
    paragraph.clear();
}

fn decode_entities(text: &str) -> String {
    let mut decoded = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        decoded.push_str(&rest[..start]);
        let tail = &rest[start..];
        let entity = tail[1..]
            .find(';')
            .and_then(|end| Some((decode_entity(&tail[1..=end])?, end + 2)));
        if let Some((character, length)) = entity {
            decoded.push(character);
            rest = &tail[length..];
        } else {
            decoded.push('&');
            rest = &tail[1..];
        }
    }
    decoded.push_str(rest);
    decoded
}

/// Decodes a numeric character reference, an XML entity, or `&nbsp;`; any other named entity
/// stays verbatim. Crossref abstracts are XML, and SSRN's search API has only been seen to send
/// these.
fn decode_entity(name: &str) -> Option<char> {
    let code = match name {
        "amp" => return Some('&'),
        "lt" => return Some('<'),
        "gt" => return Some('>'),
        "quot" => return Some('"'),
        "apos" => return Some('\''),
        "nbsp" => return Some('\u{a0}'),
        _ => name.strip_prefix('#')?,
    };
    let value = match code.strip_prefix(['x', 'X']) {
        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
        None => code.parse().ok()?,
    };
    char::from_u32(value)
}

/// Returns markup as one line of plain text, with its paragraphs joined by spaces.
pub(super) fn markup_text(markup: &str) -> String {
    markup_paragraphs(markup).join(" ")
}

pub(super) fn fold_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::markup_paragraphs;

    #[test]
    fn jats_markup_becomes_plain_paragraphs() {
        let paragraphs = markup_paragraphs(
            "<jats:title>Abstract</jats:title>\n<jats:p>A <jats:italic>b</jats:italic> &lt;c&gt; &#8211; &#x2014; p < 0.05 &unknown;</jats:p><jats:sec><jats:p>Two</jats:p></jats:sec>",
        );

        assert_eq!(
            paragraphs,
            ["Abstract", "A b <c> – — p < 0.05 &unknown;", "Two"]
        );
    }

    #[test]
    fn cdata_text_is_kept() {
        let paragraphs = markup_paragraphs("<jats:p><![CDATA[Actual <abstract> text.]]></jats:p>");

        assert_eq!(paragraphs, ["Actual <abstract> text."]);
    }

    #[test]
    fn a_closing_bracket_in_a_quoted_attribute_stays_inside_the_tag() {
        let paragraphs = markup_paragraphs(
            r#"<jats:p>See <jats:ext-link xlink:href="https://example.test/?q=a>b">Evidence</jats:ext-link>.</jats:p>"#,
        );

        assert_eq!(paragraphs, ["See Evidence."]);
    }

    #[test]
    fn named_entities_outside_xml_stay_verbatim() {
        let paragraphs = markup_paragraphs("<p>Risk &ndash; return&nbsp;&#945;</p>");

        assert_eq!(paragraphs, ["Risk &ndash; return α"]);
    }

    #[test]
    fn an_escaped_tag_stays_text() {
        let paragraphs = markup_paragraphs("<p>&lt;p&gt;literal&lt;/p&gt;</p>");

        assert_eq!(paragraphs, ["<p>literal</p>"]);
    }
}
