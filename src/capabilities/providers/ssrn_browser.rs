//! The `ssrn_browser` route: SSRN search results and paper pages, read in the user's own Chrome
//! through the forager OpenCLI adapter for `ssrn`.
//!
//! The adapter's JavaScript only reads page facts. This module checks that the page is the one
//! the request asked for and normalizes the facts into items.

use std::time::Duration;

use serde::Deserialize;

use crate::catalog::{PlatformOperation, ProviderId, ProviderTransport, registration};
use crate::config::ProcessRouteRuntimeConfig;
use crate::net::{AttemptFailure, RetryPolicy};
use crate::providers::execution::{ExecutionSettings, execute_anonymous};
use crate::providers::opencli::{self, EnvelopeStatus, OpenCliCommand};
use crate::providers::shared::{other_platform_message, parameter_error};
use crate::rate_limit::RateLimiter;
use crate::types::{
    AttemptErrorKind, AttemptTarget, ContentDepth, Deadline, Platform, PlatformFetchOutcome,
    PlatformFetchRequest, PlatformItem, PlatformItemData, PlatformRef, PlatformSearchOptions,
    PlatformSearchOutcome, PlatformSearchRequest, ProviderError, SsrnItemData, SsrnRef,
};

const ROUTE: ProviderId = ProviderId::SsrnBrowser;
/// Results on one native SSRN results page.
const NATIVE_PAGE_SIZE: u64 = 50;
const RESULTS_HOST: &str = "papers.ssrn.com";
const RESULTS_PATH: &str = "/searchresults.cfm";
const SNIPPET_SEPARATOR: &str = " … ";

/// Returns whether the route can run the request; it never starts a process.
pub(crate) fn search_support(request: &PlatformSearchRequest) -> Result<(), String> {
    match request.options {
        PlatformSearchOptions::Ssrn(_) => page_offset(request).map(|_| ()),
        PlatformSearchOptions::Arxiv(_) => {
            Err(other_platform_message(ROUTE, request.options.platform()))
        }
    }
}

/// Returns whether the route can fetch at the requested depth; it never starts a process.
pub(crate) fn fetch_support(request: &PlatformFetchRequest) -> Result<(), String> {
    match request.depth {
        ContentDepth::Metadata | ContentDepth::Abstract => Ok(()),
        depth => Err(format!(
            "{} cannot fetch at depth `{}`",
            ROUTE.name(),
            depth.as_str()
        )),
    }
}

pub(crate) struct SsrnBrowser {
    config: ProcessRouteRuntimeConfig,
    limiter: RateLimiter,
    deadline: Deadline,
}

impl SsrnBrowser {
    pub(crate) fn new(
        config: ProcessRouteRuntimeConfig,
        limiter: RateLimiter,
        deadline: Deadline,
    ) -> Self {
        Self {
            config,
            limiter,
            deadline,
        }
    }

    /// Reads the native results page that holds the requested offset. A page never spans two
    /// native pages, so it can be shorter than the limit; the next offset follows the results
    /// this page consumed.
    pub(crate) async fn search(
        &self,
        request: &PlatformSearchRequest,
    ) -> Result<PlatformSearchOutcome, ProviderError> {
        if !matches!(request.options, PlatformSearchOptions::Ssrn(_)) {
            return Err(parameter_error(other_platform_message(
                ROUTE,
                request.options.platform(),
            )));
        }
        let offset = page_offset(request).map_err(parameter_error)?;
        let position = NativePosition::of(offset);
        let command = self.command(
            "search",
            vec![
                ("query", request.query.clone()),
                ("page", position.page.to_string()),
            ],
        );
        let command = &command;
        let execution = execute_anonymous(
            self.settings(PlatformOperation::Search),
            move |deadline| async move {
                let envelope =
                    opencli::run::<ResultsPage>(command, &self.limiter, deadline).await?;
                read_results(envelope.status, &envelope.data, &request.query, position)
                    .map(|page| (None, page))
                    .map_err(runtime)
            },
        )
        .await?;
        let (consumed, next_offset) = execution.value.take(offset, request.limit);
        Ok(PlatformSearchOutcome {
            items: consumed.iter().map(SearchResult::to_item).collect(),
            next_page: next_offset.map(|offset| offset.to_string()),
            attempts: execution.attempts,
            diagnostic: execution.diagnostic,
        })
    }

    /// Reads the paper page and checks that it shows the requested paper.
    pub(crate) async fn fetch(
        &self,
        request: &PlatformFetchRequest,
    ) -> Result<PlatformFetchOutcome, ProviderError> {
        let PlatformRef::Ssrn(requested) = &request.reference else {
            return Err(parameter_error(other_platform_message(
                ROUTE,
                request.reference.platform(),
            )));
        };
        let command = self.command("paper", vec![("id", requested.id().to_owned())]);
        let command = &command;
        let execution = execute_anonymous(
            self.settings(PlatformOperation::Fetch),
            move |deadline| async move {
                let envelope = opencli::run::<PaperPage>(command, &self.limiter, deadline).await?;
                let item = read_paper(envelope.status, &envelope.data, requested)?;
                if request.depth == ContentDepth::Abstract && item.depth != ContentDepth::Abstract {
                    return Err(AttemptFailure {
                        kind: AttemptErrorKind::Quality,
                        status: None,
                        message: format!("the SSRN page has no abstract for ssrn:{requested}"),
                    });
                }
                Ok((None, item))
            },
        )
        .await?;
        Ok(PlatformFetchOutcome {
            item: execution.value,
            content_urls: Vec::new(),
            attempts: execution.attempts,
            diagnostic: execution.diagnostic,
        })
    }

    fn command(
        &self,
        command: &'static str,
        options: Vec<(&'static str, String)>,
    ) -> OpenCliCommand<'_> {
        let ProviderTransport::OpenCli(adapter) = registration(ROUTE).transport else {
            unreachable!("ssrn_browser registers an OpenCLI transport");
        };
        OpenCliCommand {
            executable: &self.config.command,
            adapter,
            command,
            options,
        }
    }

    // The route never retries: a failed browser operation falls through to the next route.
    fn settings(&self, operation: PlatformOperation) -> ExecutionSettings {
        ExecutionSettings {
            provider: ROUTE.name(),
            target: AttemptTarget::platform(Platform::Ssrn.as_str(), operation.as_str()),
            retry_policy: RetryPolicy::new(1, 1.0, Duration::ZERO),
            deadline: self.deadline,
            attempt_timeout: Duration::from_secs(self.config.timeout_seconds),
            verbose: false,
            timeout_message: "OpenCLI command timed out",
            model: None,
            transport: Some("process"),
            endpoint_host: None,
            breaker_event: None,
        }
    }
}

fn runtime(message: String) -> AttemptFailure {
    AttemptFailure {
        kind: AttemptErrorKind::Runtime,
        status: None,
        message,
    }
}

fn page_offset(request: &PlatformSearchRequest) -> Result<u64, String> {
    request.page.as_deref().map_or(Ok(0), |page| {
        page.parse::<u64>()
            .map_err(|_| format!("invalid SSRN page position `{page}`"))
    })
}

/// Where an absolute result offset falls on SSRN's native results pages.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativePosition {
    /// The 1-based native page number.
    page: u64,
    /// The index of the offset on that page.
    start: u64,
}

impl NativePosition {
    fn of(offset: u64) -> Self {
        Self {
            page: offset / NATIVE_PAGE_SIZE + 1,
            start: offset % NATIVE_PAGE_SIZE,
        }
    }
}

/// The facts the adapter reads from a results page.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ResultsPage {
    url: String,
    /// The query in the page's own search box.
    term: Option<String>,
    /// The page number the pagination marks as current.
    current_page: Option<String>,
    /// The page's range text, such as `Displaying results 51 to 100 of 10000`.
    range: Option<String>,
    /// Whether the pagination offers an enabled next-page link.
    next_page: bool,
    results: Vec<RawResult>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawResult {
    url: String,
    title: String,
    authors: Vec<String>,
    /// The detail line, such as `Number of pages: 27 • Posted: 08 Sep 2019`.
    details: String,
    snippets: Vec<String>,
}

/// A results page that matches the request.
#[derive(Debug)]
struct VerifiedPage {
    results: Vec<SearchResult>,
    has_next_page: bool,
    start: usize,
}

impl VerifiedPage {
    /// Returns at most `limit` results from the page start, and the offset of the next page
    /// when results remain on this native page or a next native page exists.
    fn take(&self, offset: u64, limit: u16) -> (&[SearchResult], Option<u64>) {
        let end = self
            .start
            .saturating_add(usize::from(limit))
            .min(self.results.len());
        let consumed = &self.results[self.start.min(end)..end];
        let has_next = end < self.results.len() || self.has_next_page;
        let next_offset =
            (has_next && !consumed.is_empty()).then(|| offset + consumed.len() as u64);
        (consumed, next_offset)
    }
}

#[derive(Debug)]
struct SearchResult {
    reference: SsrnRef,
    title: String,
    authors: Vec<String>,
    posted: Option<String>,
    snippet: Option<String>,
}

impl SearchResult {
    fn to_item(&self) -> PlatformItem {
        PlatformItem {
            url: self.reference.canonical_url(),
            reference: PlatformRef::Ssrn(self.reference.clone()),
            depth: if self.snippet.is_some() {
                ContentDepth::Snippet
            } else {
                ContentDepth::Metadata
            },
            title: self.title.clone(),
            authors: self.authors.clone(),
            published: self.posted.as_deref().and_then(iso_date),
            data: PlatformItemData::Ssrn(SsrnItemData {
                snippet: self.snippet.clone(),
                posted: self.posted.clone(),
                ..SsrnItemData::default()
            }),
        }
    }
}

/// Checks that the page is the requested query and native page, and reads its results. A page
/// that is neither a recognizable results page nor the site's own empty-result notice fails.
fn read_results(
    status: EnvelopeStatus,
    page: &ResultsPage,
    query: &str,
    position: NativePosition,
) -> Result<VerifiedPage, String> {
    check_results_address(page, query, position.page)?;
    if status == EnvelopeStatus::NoResults {
        if !page.results.is_empty() {
            return Err("the SSRN page reports no results but lists some".into());
        }
        return Ok(VerifiedPage {
            results: Vec::new(),
            has_next_page: false,
            start: 0,
        });
    }
    let range = page
        .range
        .as_deref()
        .and_then(ResultRange::parse)
        .ok_or_else(|| "the SSRN page shows no recognizable result range".to_owned())?;
    let expected_first = (position.page - 1) * NATIVE_PAGE_SIZE + 1;
    if range.first != expected_first || range.count() != page.results.len() as u64 {
        return Err(format!(
            "the SSRN page shows results {} to {} with {} listed, not page {}",
            range.first,
            range.last,
            page.results.len(),
            position.page
        ));
    }
    if position.start >= page.results.len() as u64 {
        return Err(format!(
            "SSRN page {} has {} results; the requested position is past them",
            position.page,
            page.results.len()
        ));
    }
    let results = page
        .results
        .iter()
        .map(SearchResult::read)
        .collect::<Result<_, _>>()?;
    Ok(VerifiedPage {
        results,
        has_next_page: page.next_page && range.last < range.total,
        start: usize::try_from(position.start).unwrap_or(usize::MAX),
    })
}

fn check_results_address(page: &ResultsPage, query: &str, native_page: u64) -> Result<(), String> {
    let url = reqwest::Url::parse(&page.url)
        .ok()
        .filter(|url| url.host_str() == Some(RESULTS_HOST) && url.path() == RESULTS_PATH)
        .ok_or_else(|| format!("the adapter read `{}`, not an SSRN results page", page.url))?;
    let parameter = |name: &str| {
        url.query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    };
    let expected = fold_whitespace(query);
    let url_term = parameter("term").map(|term| fold_whitespace(&term));
    let box_term = page.term.as_deref().map(fold_whitespace);
    if url_term.as_deref() != Some(expected.as_str()) || box_term.as_deref() != Some(&expected) {
        return Err(format!(
            "the SSRN page searched for {:?}, not {expected:?}",
            box_term.or(url_term).unwrap_or_default()
        ));
    }
    let url_page = parameter("page").map_or(Some(1), |value| value.parse::<u64>().ok());
    let marked_page = page
        .current_page
        .as_deref()
        .map(|value| value.trim().parse::<u64>().ok());
    if url_page != Some(native_page)
        || marked_page.is_some_and(|marked| marked != Some(native_page))
    {
        return Err(format!("the SSRN page is not results page {native_page}"));
    }
    Ok(())
}

/// The `Displaying results <first> to <last> of <total>` line of a results page.
#[derive(Debug, Eq, PartialEq)]
struct ResultRange {
    first: u64,
    last: u64,
    total: u64,
}

impl ResultRange {
    fn parse(text: &str) -> Option<Self> {
        let numbers = text
            .split(|character: char| !character.is_ascii_digit() && character != ',')
            .filter(|part| part.chars().any(|character| character.is_ascii_digit()))
            .map(|part| part.replace(',', "").parse::<u64>().ok())
            .collect::<Option<Vec<_>>>()?;
        let [first, last, total] = numbers[..] else {
            return None;
        };
        (1 <= first && first <= last && last <= total).then_some(Self { first, last, total })
    }

    fn count(&self) -> u64 {
        self.last - self.first + 1
    }
}

impl SearchResult {
    fn read(raw: &RawResult) -> Result<Self, String> {
        let reference = SsrnRef::parse(&raw.url)
            .map_err(|_| format!("an SSRN result links to `{}`, not a paper page", raw.url))?;
        let title = fold_whitespace(&raw.title);
        if title.is_empty() {
            return Err(format!("the SSRN result for ssrn:{reference} has no title"));
        }
        let snippets = raw
            .snippets
            .iter()
            .map(|snippet| fold_whitespace(snippet))
            .filter(|snippet| !snippet.is_empty())
            .collect::<Vec<_>>();
        Ok(Self {
            reference,
            title,
            authors: names(&raw.authors),
            posted: labeled_value(raw.details.split('•'), "Posted:"),
            snippet: (!snippets.is_empty()).then(|| snippets.join(SNIPPET_SEPARATOR)),
        })
    }
}

/// The facts the adapter reads from a paper page. For `no_results`, the adapter found the
/// site's notice that the paper is unavailable, in `notice`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct PaperPage {
    url: String,
    canonical_url: Option<String>,
    doi: Option<String>,
    title: String,
    authors: Vec<String>,
    abstract_paragraphs: Vec<String>,
    /// The note line parts, such as `37 Pages`, `Posted: 19 Apr 2012`.
    notes: Vec<String>,
    /// The line such as `Date Written: October 1, 2016`.
    date_written: Option<String>,
    notice: Option<String>,
}

fn read_paper(
    status: EnvelopeStatus,
    page: &PaperPage,
    requested: &SsrnRef,
) -> Result<PlatformItem, AttemptFailure> {
    if status == EnvelopeStatus::NoResults {
        return Err(AttemptFailure {
            kind: AttemptErrorKind::Parameter,
            status: None,
            message: format!(
                "SSRN paper not available: ssrn:{requested} ({})",
                page.notice.as_deref().map_or("no notice", str::trim)
            ),
        });
    }
    let shown = page
        .canonical_url
        .as_deref()
        .and_then(|url| SsrnRef::parse(url).ok())
        .or_else(|| page.doi.as_deref().and_then(SsrnRef::from_doi))
        .ok_or_else(|| runtime(format!("the page `{}` shows no SSRN abstract ID", page.url)))?;
    if &shown != requested {
        return Err(runtime(format!(
            "the SSRN page shows ssrn:{shown} for ssrn:{requested}"
        )));
    }
    let title = fold_whitespace(&page.title);
    if title.is_empty() {
        return Err(runtime(format!(
            "the SSRN page for ssrn:{requested} has no title"
        )));
    }
    let paragraphs = page
        .abstract_paragraphs
        .iter()
        .map(|paragraph| fold_whitespace(paragraph))
        .filter(|paragraph| !paragraph.is_empty())
        .collect::<Vec<_>>();
    let abstract_text = (!paragraphs.is_empty()).then(|| paragraphs.join("\n\n"));
    let posted = labeled_value(page.notes.iter().map(String::as_str), "Posted:");
    Ok(PlatformItem {
        url: requested.canonical_url(),
        reference: PlatformRef::Ssrn(requested.clone()),
        depth: if abstract_text.is_some() {
            ContentDepth::Abstract
        } else {
            ContentDepth::Metadata
        },
        title,
        authors: names(&page.authors),
        published: posted.as_deref().and_then(iso_date),
        data: PlatformItemData::Ssrn(SsrnItemData {
            abstract_text,
            last_revised: labeled_value(page.notes.iter().map(String::as_str), "Last revised:"),
            date_written: page
                .date_written
                .as_deref()
                .and_then(|line| labeled_value([line], "Date Written:")),
            posted,
            ..SsrnItemData::default()
        }),
    })
}

fn names(authors: &[String]) -> Vec<String> {
    authors
        .iter()
        .map(|author| fold_whitespace(author))
        .filter(|author| !author.is_empty())
        .collect()
}

/// Returns the text after `label` in the first part that starts with it, as the page shows it.
fn labeled_value<'a>(parts: impl IntoIterator<Item = &'a str>, label: &str) -> Option<String> {
    parts.into_iter().find_map(|part| {
        let value = fold_whitespace(part.trim().strip_prefix(label)?);
        (!value.is_empty()).then_some(value)
    })
}

/// Converts an SSRN page date such as `08 Sep 2019` to `2019-09-08`, or a lone year to itself.
fn iso_date(value: &str) -> Option<String> {
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    fn year(text: &str) -> Option<&str> {
        (text.len() == 4 && text.bytes().all(|byte| byte.is_ascii_digit())).then_some(text)
    }
    let parts = value.split_whitespace().collect::<Vec<_>>();
    match parts[..] {
        [only] => year(only).map(str::to_owned),
        [day, month, year_text] => {
            let day = day
                .parse::<u8>()
                .ok()
                .filter(|day| (1..=31).contains(day))?;
            let month = month.get(..3)?.to_ascii_lowercase();
            let month = MONTHS.iter().position(|name| *name == month)? + 1;
            Some(format!("{}-{month:02}-{day:02}", year(year_text)?))
        }
        _ => None,
    }
}

fn fold_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::{
        EnvelopeStatus, NativePosition, RawResult, ResultRange, ResultsPage, iso_date, read_results,
    };

    #[test]
    fn offsets_map_to_a_native_page_and_a_start_on_it() {
        let positions = [0, 40, 49, 50, 120].map(NativePosition::of);

        assert_eq!(
            positions.map(|position| (position.page, position.start)),
            [(1, 0), (1, 40), (1, 49), (2, 0), (3, 20)]
        );
    }

    fn result(id: u64) -> RawResult {
        RawResult {
            url: format!("https://papers.ssrn.com/sol3/papers.cfm?abstract_id={id}"),
            title: format!("Paper {id}"),
            authors: vec!["A. Author".into()],
            details: "Number of pages: 27 • Posted: 08 Sep 2019".into(),
            snippets: vec!["a <em>b</em>".into()],
        }
    }

    fn page(native_page: u64, first: u64, last: u64, total: u64, next: bool) -> ResultsPage {
        let suffix = if native_page == 1 {
            String::new()
        } else {
            format!("&page={native_page}")
        };
        ResultsPage {
            url: format!("https://papers.ssrn.com/searchresults.cfm?term=dual+momentum{suffix}"),
            term: Some("dual momentum".into()),
            current_page: Some(native_page.to_string()),
            range: Some(format!("Displaying results {first} to {last} of {total}")),
            next_page: next,
            results: (first..=last).map(result).collect(),
        }
    }

    /// Returns the consumed IDs and the next offset for `offset` and `limit`.
    fn consume(page: &ResultsPage, offset: u64, limit: u16) -> (Vec<String>, Option<u64>) {
        let verified = read_results(
            EnvelopeStatus::Ok,
            page,
            "dual momentum",
            NativePosition::of(offset),
        )
        .expect("page");
        let (consumed, next_offset) = verified.take(offset, limit);
        (
            consumed
                .iter()
                .map(|result| result.reference.id().to_owned())
                .collect(),
            next_offset,
        )
    }

    #[test]
    fn a_page_starting_mid_page_stops_at_the_native_page_end() {
        let (ids, next_offset) = consume(&page(1, 1, 50, 10_000, true), 40, 20);

        assert_eq!(
            (
                ids.len(),
                ids.first().cloned(),
                ids.last().cloned(),
                next_offset
            ),
            (10, Some("41".into()), Some("50".into()), Some(50))
        );
    }

    #[test]
    fn a_limit_over_the_native_page_size_returns_one_native_page() {
        let (ids, next_offset) = consume(&page(1, 1, 50, 10_000, true), 0, 100);

        assert_eq!((ids.len(), next_offset), (50, Some(50)));
    }

    #[test]
    fn the_last_native_page_ends_the_results() {
        let (ids, next_offset) = consume(&page(3, 101, 120, 120, false), 110, 20);

        assert_eq!((ids.len(), next_offset), (10, None));
    }

    #[test]
    fn a_short_consumption_within_the_page_leaves_a_next_page() {
        let (ids, next_offset) = consume(&page(3, 101, 120, 120, false), 100, 5);

        assert_eq!((ids.len(), next_offset), (5, Some(105)));
    }

    fn check(page: &ResultsPage, offset: u64) -> Result<(), String> {
        read_results(
            EnvelopeStatus::Ok,
            page,
            "dual momentum",
            NativePosition::of(offset),
        )
        .map(|_| ())
    }

    #[test]
    fn a_page_for_another_query_is_rejected() {
        let mut stale = page(1, 1, 50, 100, true);
        stale.term = Some("old query".into());

        assert!(check(&stale, 0).is_err());
    }

    #[test]
    fn a_page_for_another_page_number_is_rejected() {
        let result = check(&page(1, 1, 50, 100, true), 50);

        assert_eq!(
            result,
            Err("the SSRN page is not results page 2".to_owned())
        );
    }

    #[test]
    fn a_page_without_a_result_range_is_rejected() {
        let mut challenge = page(1, 1, 50, 100, true);
        challenge.range = None;
        challenge.results.clear();

        assert!(check(&challenge, 0).is_err());
    }

    #[test]
    fn a_range_that_disagrees_with_the_listed_results_is_rejected() {
        let mut partial = page(1, 1, 50, 100, true);
        partial.results.truncate(10);

        assert!(check(&partial, 0).is_err());
    }

    #[test]
    fn a_page_on_another_host_is_rejected() {
        let mut other = page(1, 1, 50, 100, true);
        other.url = "https://www.ssrn.com/searchresults.cfm?term=dual+momentum".into();

        assert!(check(&other, 0).is_err());
    }

    #[test]
    fn a_no_results_page_for_the_query_is_empty() {
        let empty = ResultsPage {
            url: "https://papers.ssrn.com/searchresults.cfm?term=dual%20momentum".into(),
            term: Some("dual momentum".into()),
            ..ResultsPage::default()
        };

        let result = read_results(
            EnvelopeStatus::NoResults,
            &empty,
            "dual  momentum",
            NativePosition::of(0),
        );

        assert_eq!(
            result.map(|page| (page.results.len(), page.has_next_page)),
            Ok((0, false))
        );
    }

    #[test]
    fn result_ranges_parse_with_thousands_separators() {
        let ranges = [
            "Displaying results 51 to 100 of 10000",
            "Displaying results 1 to 3 of 3",
            "Displaying results 1 to 50 of 12,345",
            "No results.",
            "Displaying results 5 to 1 of 3",
        ]
        .map(ResultRange::parse);

        assert_eq!(
            ranges,
            [
                Some(ResultRange {
                    first: 51,
                    last: 100,
                    total: 10_000
                }),
                Some(ResultRange {
                    first: 1,
                    last: 3,
                    total: 3
                }),
                Some(ResultRange {
                    first: 1,
                    last: 50,
                    total: 12_345
                }),
                None,
                None,
            ]
        );
    }

    #[test]
    fn page_dates_become_iso_dates_at_their_precision() {
        let dates = [
            "08 Sep 2019",
            "5 Jul 2000",
            "1994",
            "Sept 2019",
            "31 Foo 2019",
        ]
        .map(iso_date);

        assert_eq!(
            dates,
            [
                Some("2019-09-08".to_owned()),
                Some("2000-07-05".to_owned()),
                Some("1994".to_owned()),
                None,
                None
            ]
        );
    }
}
