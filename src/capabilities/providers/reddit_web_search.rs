//! Reddit search: the results fragment the site's search page loads.
//!
//! One fragment page lists about seven posts. Each result is a `search-telemetry-tracker` that
//! Reddit marks as a post view; its tracking record carries the post's ID, title, flags,
//! subreddit, author, and the excerpt the search shows, and the result renders the post's time,
//! score, and comment count. The next page is a lazily loaded fragment whose URL carries an
//! opaque `cursor`.
//!
//! A search reads pages until it has `limit` posts. A page position is the upstream cursor of a
//! page (none for the first) and how many of its posts an earlier page of results already
//! delivered, so a cursor never skips a post the previous page left undelivered.

use std::collections::HashSet;

use reqwest::Url;
use scraper::{ElementRef, Html};
use serde::Deserialize;

use super::{RedditWeb, SITE_ORIGIN, Step, folded_text, selector, site_timestamp, unexpected_page};
use crate::providers::shared::{Refusal, Trail, other_platform_message, parameter_error};
use crate::types::{
    AttemptErrorKind, ContentDepth, PlatformItem, PlatformItemData, PlatformRef,
    PlatformSearchOptions, PlatformSearchOutcome, PlatformSearchRequest, ProviderError,
    RedditItemData, RedditRef, RedditSearchOptions, RedditSnippetSource,
};

/// The most fragment pages one search reads; a full page of 25 needs four.
const MAX_PAGES: usize = 5;
const RESULT_UNIT: &str = "search-telemetry-tracker[view-events=\"search/view/post\"]";
const NO_RESULTS_UNIT: &str = "[view-events=\"search/view/no_results\"]";

/// Returns whether the route can run the search; it never sends a request.
pub(crate) fn search_support(request: &PlatformSearchRequest) -> Result<(), String> {
    if !matches!(request.options, PlatformSearchOptions::Reddit(_)) {
        return Err(other_platform_message(
            super::ROUTE,
            request.options.platform(),
        ));
    }
    PagePosition::parse(request.page.as_deref()).map(|_| ())
}

/// Where a page of results starts: the upstream page and how many of its posts to pass over.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct PagePosition {
    cursor: Option<String>,
    skip: usize,
}

impl PagePosition {
    /// Parses `<skip>` or `<skip>.<cursor>`; no position is the first page.
    fn parse(page: Option<&str>) -> Result<Self, String> {
        let Some(page) = page else {
            return Ok(Self::default());
        };
        let invalid = || format!("invalid Reddit search page position `{page}`");
        let (skip, cursor) = page.split_once('.').map_or((page, None), |(skip, cursor)| {
            (skip, Some(cursor.to_owned()))
        });
        let skip = skip.parse().map_err(|_| invalid())?;
        if cursor.as_deref().is_some_and(str::is_empty) {
            return Err(invalid());
        }
        Ok(Self { cursor, skip })
    }

    fn encode(&self) -> String {
        match &self.cursor {
            Some(cursor) => format!("{}.{cursor}", self.skip),
            None => self.skip.to_string(),
        }
    }
}

impl RedditWeb {
    pub(crate) async fn search(
        &self,
        request: &PlatformSearchRequest,
    ) -> Result<PlatformSearchOutcome, ProviderError> {
        let PlatformSearchOptions::Reddit(options) = &request.options else {
            return Err(parameter_error(other_platform_message(
                super::ROUTE,
                request.options.platform(),
            )));
        };
        let mut position = PagePosition::parse(request.page.as_deref()).map_err(parameter_error)?;
        let url = search_url(&self.config.url, options);
        let limit = usize::from(request.limit);
        let mut trail = Trail::default();
        let (mut items, mut seen, mut used) = (Vec::new(), HashSet::new(), HashSet::new());
        let mut skipped = 0;
        let mut next_page = None;
        for page_number in 1..=MAX_PAGES {
            let mut query = search_query(&request.query, options);
            if let Some(cursor) = &position.cursor {
                query.push(("cursor", cursor.clone()));
            }
            let page = trail.required(
                self.send(Step::Search, &url, &query, read_search_page)
                    .await,
            )?;
            skipped += page.skipped;
            let undelivered = page
                .items
                .into_iter()
                .enumerate()
                .skip(position.skip)
                .find_map(|(index, item)| {
                    if items.len() == limit {
                        return Some(index);
                    }
                    if seen.insert(item.reference.to_string()) {
                        items.push(item);
                    }
                    None
                });
            if let Some(index) = undelivered {
                next_page = Some(PagePosition {
                    cursor: position.cursor,
                    skip: index,
                });
                break;
            }
            let Some(cursor) = page
                .next_cursor
                .filter(|cursor| used.insert(cursor.clone()))
            else {
                break;
            };
            position = PagePosition {
                cursor: Some(cursor),
                skip: 0,
            };
            if items.len() == limit || page_number == MAX_PAGES {
                next_page = Some(position);
                break;
            }
        }
        if skipped > 0 {
            trail.note(format!(
                "skipped {skipped} Reddit search results that could not be read"
            ));
        }
        Ok(PlatformSearchOutcome {
            items,
            next_page: next_page.map(|position| position.encode()),
            diagnostic: trail.diagnostic(),
            attempts: trail.attempts,
        })
    }
}

fn search_url(base: &str, options: &RedditSearchOptions) -> String {
    let base = base.trim_end_matches('/');
    match &options.subreddit {
        Some(subreddit) => format!("{base}/svc/shreddit/r/{subreddit}/search/"),
        None => format!("{base}/svc/shreddit/search/"),
    }
}

fn search_query(query: &str, options: &RedditSearchOptions) -> Vec<(&'static str, String)> {
    let mut parameters = vec![
        ("q", query.to_owned()),
        ("type", "posts".to_owned()),
        ("sort", options.sort.as_str().to_owned()),
    ];
    if let Some(time) = options.time {
        parameters.push(("t", time.as_str().to_owned()));
    }
    parameters
}

/// One fragment page of results.
struct SearchPage {
    items: Vec<PlatformItem>,
    /// Results the page lists but that could not be read.
    skipped: usize,
    next_cursor: Option<String>,
}

#[derive(Deserialize)]
struct Tracking {
    post: TrackedPost,
    subreddit: Option<Named>,
    profile: Option<Named>,
    search: Option<TrackedSearch>,
}

#[derive(Deserialize)]
struct TrackedPost {
    id: String,
    title: Option<String>,
    nsfw: Option<bool>,
    spoiler: Option<bool>,
}

#[derive(Deserialize)]
struct Named {
    name: Option<String>,
}

#[derive(Deserialize)]
struct TrackedSearch {
    snippet: Option<String>,
    snippet_id: Option<String>,
}

fn read_search_page(html: &str) -> Result<SearchPage, Refusal> {
    let document = Html::parse_document(html);
    let units = document.select(&selector(RESULT_UNIT)).collect::<Vec<_>>();
    if units.is_empty() {
        if document.select(&selector(NO_RESULTS_UNIT)).next().is_some() {
            return Ok(SearchPage {
                items: Vec::new(),
                skipped: 0,
                next_cursor: None,
            });
        }
        return Err(unexpected_page(&document, "a Reddit search results page"));
    }
    let listed = units.len();
    let items = units
        .into_iter()
        .filter_map(search_item)
        .collect::<Vec<_>>();
    if items.is_empty() {
        return Err(Refusal::new(
            AttemptErrorKind::Runtime,
            format!(
                "none of the {listed} Reddit search results could be read; the page format may have changed"
            ),
        ));
    }
    Ok(SearchPage {
        skipped: listed - items.len(),
        items,
        next_cursor: next_cursor(&document),
    })
}

/// Reads one result; `None` when its tracking record does not name a post.
fn search_item(unit: ElementRef<'_>) -> Option<PlatformItem> {
    let tracking =
        serde_json::from_str::<Tracking>(unit.attr("data-faceplate-tracking-context")?).ok()?;
    let reference = RedditRef::from_fullname(&tracking.post.id)?;
    let subreddit = tracking.subreddit.and_then(|subreddit| subreddit.name);
    let permalink = unit
        .select(&selector("a[data-testid=\"post-title\"][href]"))
        .find_map(|link| link.attr("href"))
        .filter(|href| href.starts_with('/'))
        .map(|href| format!("{SITE_ORIGIN}{href}"));
    let (snippet, snippet_source) = tracking
        .search
        .and_then(|search| {
            let snippet = search
                .snippet
                .filter(|snippet| !snippet.trim().is_empty())?;
            let source = match search.snippet_id.as_deref() {
                Some(id) if id.starts_with("t1_") => Some(RedditSnippetSource::Comment),
                Some(id) if id == tracking.post.id => Some(RedditSnippetSource::Post),
                _ => None,
            };
            Some((Some(snippet), source))
        })
        .unwrap_or_default();
    let (score, comments) = counts(unit);
    Some(PlatformItem {
        url: reference.canonical_url(),
        reference: PlatformRef::Reddit(reference),
        depth: if snippet.is_some() {
            ContentDepth::Snippet
        } else {
            ContentDepth::Metadata
        },
        title: tracking.post.title.filter(|title| !title.is_empty()),
        authors: tracking
            .profile
            .and_then(|profile| profile.name)
            .into_iter()
            .collect(),
        published: unit
            .select(&selector("faceplate-timeago[ts]"))
            .find_map(|time| time.attr("ts").and_then(site_timestamp)),
        data: PlatformItemData::Reddit(RedditItemData {
            subreddit,
            permalink,
            score,
            comments,
            nsfw: tracking.post.nsfw,
            spoiler: tracking.post.spoiler,
            snippet,
            snippet_source,
            ..RedditItemData::default()
        }),
    })
}

/// Reads a result's score and comment count from its counter row, where each number is labeled
/// by the word after it.
fn counts(unit: ElementRef<'_>) -> (Option<i64>, Option<u64>) {
    let (mut score, mut comments) = (None, None);
    for number in unit.select(&selector(
        "[data-testid=\"search-counter-row\"] faceplate-number[number]",
    )) {
        let Some(label) = number.parent().and_then(ElementRef::wrap).map(folded_text) else {
            continue;
        };
        let value = number.attr("number").unwrap_or_default();
        if label.contains("vote") {
            score = score.or_else(|| value.parse().ok());
        } else if label.contains("comment") {
            comments = comments.or_else(|| value.parse().ok());
        }
    }
    (score, comments)
}

/// Returns the cursor of the lazily loaded next page, when the page has one.
fn next_cursor(document: &Html) -> Option<String> {
    document
        .select(&selector("faceplate-partial[src]"))
        .filter_map(|partial| partial.attr("src"))
        .filter(|src| src.contains("/svc/shreddit/") && src.contains("search/"))
        .find_map(|src| {
            let url = Url::parse(SITE_ORIGIN).ok()?.join(src).ok()?;
            url.query_pairs()
                .find(|(name, _)| name == "cursor")
                .map(|(_, cursor)| cursor.into_owned())
                .filter(|cursor| !cursor.is_empty())
        })
}

#[cfg(test)]
mod tests {
    use super::PagePosition;

    #[test]
    fn page_positions_round_trip_and_reject_malformed_input() {
        let positions = ["3", "0.eyJjIjoxfQ==", "2.a.b"].map(|page| {
            PagePosition::parse(Some(page)).map(|position| (position.encode(), position))
        });
        let invalid =
            ["", "x", "-1.abc", "1."].map(|page| PagePosition::parse(Some(page)).is_err());

        assert_eq!(
            positions.map(|parsed| parsed.map(|(encoded, _)| encoded)),
            [
                Ok("3".to_owned()),
                Ok("0.eyJjIjoxfQ==".to_owned()),
                Ok("2.a.b".to_owned())
            ]
        );
        assert_eq!(invalid, [true; 4]);
    }
}
