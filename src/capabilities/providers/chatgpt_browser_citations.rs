//! Rewriting of the citation markers in a ChatGPT Deep Research report. ChatGPT wraps each
//! citation in private-use characters, such as `\u{e200}cite\u{e202}turn0search1\u{e201}`,
//! that mean nothing outside ChatGPT. Each `grouped_webpages` content reference covers one
//! marker by its character span and lists the cited URLs, so markers become `[N]` numbers into
//! one sources list. The ids inside a marker are never matched to URLs: on 2026-10-10 only 3
//! of 12 resolved through the search result groups.

use std::collections::HashMap;
use std::fmt::Write as _;

use super::shape::{Node, ShapeError};
use crate::types::ReportSource;

const MARKER_START: char = '\u{e200}';
const MARKER_END: char = '\u{e201}';
const MARKER_SEPARATOR: char = '\u{e202}';
const GROUPED_WEBPAGES: &str = "grouped_webpages";
const TRACKING_PARAMETER: &str = "utm_source=chatgpt.com";

/// One marker to replace: its character span and the URLs it cites, with the titles its own
/// reference items give.
struct Citation<'a> {
    reference: Node<'a>,
    start: usize,
    end: usize,
    urls: Vec<String>,
    titles: HashMap<String, &'a str>,
}

/// Replaces every marker that a `grouped_webpages` reference covers with the numbers of its
/// URLs, numbered across the report by first appearance, and returns the body and its sources.
/// `body_path` locates the body in shape errors.
///
/// # Errors
///
/// Fails when a reference does not cover a marker or lacks its URLs, or when a marker is left
/// in the body after rewriting.
pub(super) fn rewrite(
    body: &str,
    body_path: &str,
    metadata: &Node<'_>,
) -> Result<(String, Vec<ReportSource>), ShapeError> {
    let mut citations = match metadata.get("content_references") {
        Some(references) => grouped_webpages(&references)?,
        None => Vec::new(),
    };
    citations.sort_by_key(|citation| citation.start);
    let group_titles = metadata
        .get("search_result_groups")
        .map(|groups| search_result_titles(&groups))
        .unwrap_or_default();
    let characters = body.chars().collect::<Vec<_>>();
    let mut rewritten = String::with_capacity(body.len());
    let mut sources = Vec::<ReportSource>::new();
    let mut numbers = HashMap::<String, u64>::new();
    let mut cursor = 0;
    for citation in citations {
        let span = characters
            .get(citation.start..citation.end)
            .filter(|_| citation.start >= cursor);
        let covers_marker = span.is_some_and(|span| {
            span.first() == Some(&MARKER_START) && span.last() == Some(&MARKER_END)
        });
        let Some(span) = span.filter(|_| covers_marker) else {
            return Err(citation.reference.error(&format!(
                "the span {}..{} covers no citation marker of the body",
                citation.start, citation.end
            )));
        };
        rewritten.extend(&characters[cursor..citation.start]);
        let cited = citation
            .urls
            .into_iter()
            .map(|url| {
                *numbers.entry(url.clone()).or_insert_with(|| {
                    let id = sources.len() as u64 + 1;
                    let title = citation
                        .titles
                        .get(&url)
                        .or_else(|| group_titles.get(&url))
                        .map(|title| (*title).to_owned());
                    sources.push(ReportSource { id, title, url });
                    id
                })
            })
            .map(|number| number.to_string())
            .collect::<Vec<_>>();
        let _ = write!(rewritten, "[{}]", cited.join(", "));
        cursor = citation.start + span.len();
    }
    rewritten.extend(&characters[cursor..]);
    if let Some(position) = rewritten
        .chars()
        .position(|character| [MARKER_START, MARKER_END, MARKER_SEPARATOR].contains(&character))
    {
        return Err(ShapeError::at(
            body_path,
            &format!("an unresolved citation marker at character {position} after rewriting"),
        ));
    }
    Ok((rewritten, sources))
}

fn grouped_webpages<'a>(references: &Node<'a>) -> Result<Vec<Citation<'a>>, ShapeError> {
    let mut citations = Vec::new();
    for reference in references.items()? {
        if reference.get("type").and_then(|kind| kind.value.as_str()) != Some(GROUPED_WEBPAGES) {
            continue;
        }
        let start = reference.field("start_idx")?.index()?;
        let end = reference.field("end_idx")?.index()?;
        let mut urls = Vec::new();
        for url in reference.field("safe_urls")?.items()? {
            let url = without_tracking(url.text()?);
            if !urls.contains(&url) {
                urls.push(url);
            }
        }
        if urls.is_empty() {
            return Err(reference.error("a citation without safe URLs"));
        }
        let titles = reference
            .get("items")
            .map(|items| titled_urls(&items))
            .unwrap_or_default();
        citations.push(Citation {
            reference,
            start,
            end,
            urls,
            titles,
        });
    }
    Ok(citations)
}

/// The titles that the entries of the search result groups give their URLs.
fn search_result_titles<'a>(groups: &Node<'a>) -> HashMap<String, &'a str> {
    let mut titles = HashMap::new();
    for group in groups.items().into_iter().flatten() {
        if let Some(entries) = group.get("entries") {
            for (url, title) in titled_urls(&entries) {
                titles.entry(url).or_insert(title);
            }
        }
    }
    titles
}

/// The non-empty titles of the objects in `items`, keyed by their URL without tracking. Titles
/// only name sources, so entries of other shapes are skipped.
fn titled_urls<'a>(items: &Node<'a>) -> HashMap<String, &'a str> {
    let mut titles = HashMap::new();
    for item in items.items().into_iter().flatten() {
        let url = item.value.get("url").and_then(serde_json::Value::as_str);
        let title = item
            .value
            .get("title")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|title| !title.is_empty());
        if let (Some(url), Some(title)) = (url, title) {
            titles.entry(without_tracking(url)).or_insert(title);
        }
    }
    titles
}

/// The URL without the `utm_source=chatgpt.com` query parameter that ChatGPT appends.
fn without_tracking(url: &str) -> String {
    let (address, fragment) = match url.split_once('#') {
        Some((address, fragment)) => (address, Some(fragment)),
        None => (url, None),
    };
    let Some((base, query)) = address.split_once('?') else {
        return url.to_owned();
    };
    let kept = query
        .split('&')
        .filter(|parameter| !parameter.is_empty() && *parameter != TRACKING_PARAMETER)
        .collect::<Vec<_>>();
    let mut cleaned = base.to_owned();
    if !kept.is_empty() {
        cleaned.push('?');
        cleaned.push_str(&kept.join("&"));
    }
    if let Some(fragment) = fragment {
        cleaned.push('#');
        cleaned.push_str(fragment);
    }
    cleaned
}
