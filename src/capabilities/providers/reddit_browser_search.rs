//! Reddit search: opening the search page whose URL carries the options, projecting its
//! results, scrolling natively while the page lists fewer than `limit`, and judging the
//! projection: page facts, result decoding, repeats, and what the results leave out.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use reqwest::Url;
use serde::Deserialize;

use super::{
    Observation, POLL, PageFacts, Projection, ROUTE, RedditBrowser, SITE_ORIGIN, blocked, capture,
    ended_without_content, is_page, project, projection_script, runtime, server_failure,
    site_timestamp, site_url, unexpected_page, unfinished,
};
use crate::catalog::PlatformOperation;
use crate::net::{AttemptFailure, combine_diagnostics};
use crate::providers::browser::{InputError, InputFailure, Page};
use crate::providers::browser_route::PageReading;
use crate::providers::execution::execute_anonymous;
use crate::providers::shared::{other_platform_message, parameter_error};
use crate::types::{
    ContentDepth, PlatformItem, PlatformItemData, PlatformRef, PlatformSearchOptions,
    PlatformSearchOutcome, PlatformSearchRequest, ProviderError, RedditItemData, RedditRef,
    RedditSearchOptions, RedditSnippetSource,
};

/// The most native scrolls one search makes; the first screen lists about seven posts, and each
/// scroll loads about seven more.
const MAX_SCROLLS: u32 = 6;
/// How long a scroll waits for more results before the next one.
const SCROLL_WAIT: Duration = Duration::from_secs(4);

/// Returns whether the route can run the search; it never starts a process. Reddit search
/// pages continue only by scrolling, so the route issues no cursor and runs only a first page.
pub(crate) fn search_support(request: &PlatformSearchRequest) -> Result<(), String> {
    if !matches!(request.options, PlatformSearchOptions::Reddit(_)) {
        return Err(other_platform_message(ROUTE, request.options.platform()));
    }
    if request.page.is_some() {
        return Err(format!(
            "{} cannot continue a search from a page position; it issues no cursor",
            ROUTE.name()
        ));
    }
    Ok(())
}

impl RedditBrowser {
    /// Opens the search page for the request and reads the results that `limit` needs in one
    /// browser command.
    pub(crate) async fn search(
        &self,
        request: &PlatformSearchRequest,
    ) -> Result<PlatformSearchOutcome, ProviderError> {
        search_support(request).map_err(parameter_error)?;
        let PlatformSearchOptions::Reddit(options) = &request.options else {
            unreachable!("support checked")
        };
        let target = SearchTarget::new(request.query.trim(), options);
        let limit = usize::from(request.limit);
        let execution = execute_anonymous(
            self.settings(PlatformOperation::Search.as_str()),
            |deadline| {
                let target = &target;
                async move {
                    let observed = self
                        .browser
                        .read(capture, deadline, async |page| {
                            observe_search(page, target, limit).await
                        })
                        .await?;
                    Ok((None, search_outcome(&observed, target, limit)?))
                }
            },
        )
        .await?;
        let decoded = execution.value;
        Ok(PlatformSearchOutcome {
            items: decoded.items,
            next_page: None,
            attempts: execution.attempts,
            diagnostic: combine_diagnostics(
                [execution.diagnostic, decoded.skipped, decoded.incomplete]
                    .into_iter()
                    .flatten(),
            ),
        })
    }
}

/// The search page a request opens: its URL and the path the page must stay on.
#[derive(Debug)]
struct SearchTarget {
    url: String,
    path: String,
}

impl SearchTarget {
    fn new(query: &str, options: &RedditSearchOptions) -> Self {
        let path = match &options.subreddit {
            Some(subreddit) => format!("/r/{subreddit}/search/"),
            None => "/search/".to_owned(),
        };
        let mut url = Url::parse(SITE_ORIGIN)
            .and_then(|origin| origin.join(&path))
            .expect("a subreddit name is a valid path segment");
        {
            let mut query_pairs = url.query_pairs_mut();
            query_pairs.append_pair("q", query);
            if options.subreddit.is_some() {
                query_pairs.append_pair("restrict_sr", "1");
            } else {
                query_pairs.append_pair("type", "posts");
            }
            query_pairs.append_pair("sort", options.sort.as_str());
            if let Some(time) = options.time {
                query_pairs.append_pair("t", time.as_str());
            }
        }
        Self {
            url: url.into(),
            path,
        }
    }
}

/// What one projection of a search page shows.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct SearchProjection {
    page: PageFacts,
    /// Whether the page marks the search as having no results.
    no_results: bool,
    /// Whether the page has the lazily loaded fragment of its next page of results.
    next_page: bool,
    results: Vec<ResultRecord>,
}

impl Projection for SearchProjection {
    fn page_mut(&mut self) -> &mut PageFacts {
        &mut self.page
    }
}

impl SearchProjection {
    /// Takes a later projection's facts and markers, and appends the records it shows that no
    /// earlier projection did.
    fn absorb(&mut self, later: Self) {
        let Self {
            page,
            no_results,
            next_page,
            results,
        } = later;
        self.page = page;
        self.no_results = no_results;
        self.next_page = next_page;
        for record in results {
            if !self.results.contains(&record) {
                self.results.push(record);
            }
        }
    }
}

/// One search result as the page renders it.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct ResultRecord {
    /// The result's `data-faceplate-tracking-context`, a JSON record of the post.
    #[serde(deserialize_with = "Option::deserialize")]
    tracking: Option<String>,
    /// The `href` of the post title link.
    #[serde(deserialize_with = "Option::deserialize")]
    permalink: Option<String>,
    /// The `ts` of the post's `faceplate-timeago`.
    #[serde(deserialize_with = "Option::deserialize")]
    published: Option<String>,
    /// The counter row's numbers, each with the text that labels it.
    counts: Vec<CountRecord>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct CountRecord {
    /// The `number` attribute of a `faceplate-number`.
    #[serde(deserialize_with = "Option::deserialize")]
    number: Option<String>,
    /// The text of the element around it, such as `124 votes`.
    label: String,
}

/// What one search read observed.
type SearchObservation = Observation<SearchProjection>;

// The results, the no-results marker, and the next page fragment; nothing about the account.
const SEARCH_PROJECTION: &str = r#"
  const results = [...document.querySelectorAll('search-telemetry-tracker[view-events="search/view/post"]')].map((unit) => {
    const link = unit.querySelector('a[data-testid="post-title"][href]');
    const time = unit.querySelector('faceplate-timeago[ts]');
    return {
      tracking: unit.getAttribute('data-faceplate-tracking-context'),
      permalink: link ? link.getAttribute('href') : null,
      published: time ? time.getAttribute('ts') : null,
      counts: [...unit.querySelectorAll('[data-testid="search-counter-row"] faceplate-number[number]')].map((number) => ({
        number: number.getAttribute('number'),
        label: number.parentElement ? number.parentElement.textContent : '',
      })),
    };
  });
  return {
    page,
    no_results: document.querySelector('[view-events="search/view/no_results"]') !== null,
    next_page: document.querySelector('faceplate-partial[src*="/svc/shreddit/"][src*="search/"][src*="cursor="]') !== null,
    results,
  };"#;

// Where the wheel turns to reach the end of the results, and how far that is.
const SCROLL_PLAN: &str = r"(() => {
  const root = document.scrollingElement || document.documentElement;
  return { x: root.clientWidth / 2, y: root.clientHeight / 2, distance: root.scrollHeight - root.clientHeight - root.scrollTop };
})()";

#[derive(Deserialize)]
struct ScrollPlan {
    x: f64,
    y: f64,
    distance: f64,
}

/// How far a search read has come.
enum SearchState {
    /// The page shows no final state yet.
    Waiting,
    /// The page shows results, which scrolling may add to.
    Listed,
    /// The page reached a state that ends the read.
    Ended,
}

fn search_state(projection: &SearchProjection, document_status: Option<u16>) -> SearchState {
    let page = &projection.page;
    if blocked(page, document_status).is_some() || projection.no_results {
        SearchState::Ended
    } else if !projection.results.is_empty() {
        SearchState::Listed
    } else if ended_without_content(page, document_status) {
        SearchState::Ended
    } else {
        SearchState::Waiting
    }
}

/// Drives one search: opens the search page and projects it every half second until it reaches
/// a final state, scrolling natively while it lists fewer than `limit` posts and has a next
/// page, at most [`MAX_SCROLLS`] times.
async fn observe_search(
    page: &mut Page,
    target: &SearchTarget,
    limit: usize,
) -> Result<SearchObservation, AttemptFailure> {
    let script = projection_script(SEARCH_PROJECTION);
    let mut reading = PageReading::new(page);
    reading.navigate(&target.url).await?;
    let mut merged: Option<SearchProjection> = None;
    let mut document_status = None;
    let mut scrolls = 0;
    // When the last scroll happened, and how many posts the page listed before it.
    let mut scrolled: Option<(Instant, usize)> = None;
    let mut ended = false;
    while reading.page().reading() {
        if let Some(projection) = project::<SearchProjection>(&mut reading, &script).await? {
            match &mut merged {
                Some(merged) => merged.absorb(projection),
                None => merged = Some(projection),
            }
        }
        if let Some(current) = &merged {
            document_status =
                super::document_status(reading.exchanges(), &target.path, &current.page.url);
            match search_state(current, document_status) {
                SearchState::Waiting => {}
                SearchState::Ended => {
                    ended = true;
                    break;
                }
                SearchState::Listed => {
                    let listed = listed_posts(&current.results);
                    let due = scrolled
                        .is_none_or(|(at, before)| listed > before || at.elapsed() >= SCROLL_WAIT);
                    if listed >= limit || !current.next_page || (scrolls >= MAX_SCROLLS && due) {
                        ended = true;
                        break;
                    }
                    if due {
                        scrolled = Some((Instant::now(), listed));
                        scrolls += 1;
                        if !scroll_to_end(&mut reading).await? {
                            break;
                        }
                        continue;
                    }
                }
            }
        }
        reading.page().rest(POLL).await;
    }
    let timed_out = !ended
        && merged.as_ref().is_none_or(|projection| {
            matches!(
                search_state(projection, document_status),
                SearchState::Waiting
            )
        });
    Ok(Observation::new(
        merged,
        &reading,
        document_status,
        timed_out,
    ))
}

/// Turns the wheel toward the end of the results. Returns `false` once the read deadline has
/// passed. A page that briefly answers no script leaves the next due scroll to turn it.
///
/// # Errors
///
/// Returns a transport failure, or a scroll script failure that lasted several reads.
async fn scroll_to_end(reading: &mut PageReading<'_>) -> Result<bool, AttemptFailure> {
    let Some(plan) = reading.evaluate::<ScrollPlan>(SCROLL_PLAN).await? else {
        return Ok(true);
    };
    if plan.distance < 1.0 {
        return Ok(true);
    }
    match reading.page().wheel((plan.x, plan.y), plan.distance).await {
        Ok(()) => Ok(true),
        Err(InputError::Transport(failure)) => Err(failure),
        Err(InputError::Failed(missed)) => Ok(missed.failure != InputFailure::DeadlinePassed),
    }
}

/// How many different posts `records` list.
fn listed_posts(records: &[ResultRecord]) -> usize {
    records
        .iter()
        .filter_map(|record| tracking(record).and_then(|tracking| post_ref(&tracking)))
        .map(|reference| reference.id().to_owned())
        .collect::<HashSet<_>>()
        .len()
}

/// The decoded items and the diagnostics that say what they leave out.
#[derive(Debug)]
struct Decoded {
    items: Vec<PlatformItem>,
    skipped: Option<String>,
    incomplete: Option<String>,
}

/// Judges one search read: classifies the page facts, and decodes the results, without repeats
/// and at most `limit`.
fn search_outcome(
    observed: &SearchObservation,
    target: &SearchTarget,
    limit: usize,
) -> Result<Decoded, AttemptFailure> {
    let projection = &observed.projection;
    let facts = &projection.page;
    if let Some(failure) = blocked(facts, observed.document_status) {
        return Err(failure);
    }
    if !projection.results.is_empty() {
        return decode(projection, limit);
    }
    if projection.no_results {
        return Ok(Decoded {
            items: Vec::new(),
            skipped: None,
            incomplete: None,
        });
    }
    if let Some(failure) = server_failure(facts, observed.document_status) {
        return Err(failure);
    }
    let on_search_page = is_page(&facts.url, &target.path);
    if observed.timed_out {
        return Err(unfinished(
            facts,
            on_search_page,
            "the Reddit search page showed no results before the read deadline",
        ));
    }
    if !on_search_page {
        return Err(unexpected_page(facts));
    }
    Err(runtime(
        "the Reddit search page shows neither results nor a no-results notice; the page format may have changed".into(),
    ))
}

/// Decodes the results in the order the page lists them, skips those that do not name a post,
/// delivers each post once, and keeps at most `limit`.
fn decode(projection: &SearchProjection, limit: usize) -> Result<Decoded, AttemptFailure> {
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    let mut skipped = 0_usize;
    for record in &projection.results {
        let Some(item) = decode_result(record) else {
            skipped += 1;
            continue;
        };
        if seen.insert(item.url.clone()) {
            items.push(item);
        }
    }
    if items.is_empty() {
        return Err(runtime(format!(
            "none of the {} Reddit search results could be read; the page format may have changed",
            projection.results.len()
        )));
    }
    let read = items.len();
    items.truncate(limit);
    Ok(Decoded {
        items,
        skipped: (skipped > 0)
            .then(|| format!("skipped {skipped} Reddit search results that could not be read")),
        incomplete: (read < limit && projection.next_page).then(|| {
            format!(
                "Reddit has more results than the {read} this command read; Reddit search issues no cursor to continue from, so narrow the query, the subreddit, or the time window to reach them"
            )
        }),
    })
}

/// Reads one result; `None` when its tracking record does not name a post.
fn decode_result(record: &ResultRecord) -> Option<PlatformItem> {
    let tracking = tracking(record)?;
    let reference = post_ref(&tracking)?;
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
    let (score, comments) = counts(&record.counts);
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
        published: record.published.as_deref().and_then(site_timestamp),
        data: PlatformItemData::Reddit(RedditItemData {
            subreddit: tracking.subreddit.and_then(|subreddit| subreddit.name),
            permalink: record.permalink.as_deref().and_then(site_url),
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

/// Reads a result's score and comment count from its counter row, where the word after each
/// number labels it.
fn counts(counts: &[CountRecord]) -> (Option<i64>, Option<u64>) {
    let (mut score, mut comments) = (None, None);
    for count in counts {
        let value = count.number.as_deref().unwrap_or_default();
        if count.label.contains("vote") {
            score = score.or_else(|| value.parse().ok());
        } else if count.label.contains("comment") {
            comments = comments.or_else(|| value.parse().ok());
        }
    }
    (score, comments)
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

fn tracking(record: &ResultRecord) -> Option<Tracking> {
    serde_json::from_str(record.tracking.as_deref()?).ok()
}

fn post_ref(tracking: &Tracking) -> Option<RedditRef> {
    RedditRef::from_fullname(&tracking.post.id)
}

#[cfg(test)]
#[path = "reddit_browser_search_tests.rs"]
mod tests;
