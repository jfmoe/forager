//! SSRN native search execution, page verification, and pagination.

use serde::Deserialize;

use super::{ROUTE, SsrnBrowser, fold_whitespace, iso_date, labeled_value, names, runtime};
use crate::catalog::PlatformOperation;
use crate::providers::execution::execute_anonymous;
use crate::providers::opencli::{self, EnvelopeStatus};
use crate::providers::shared::{other_platform_message, parameter_error};
use crate::types::{
    ContentDepth, PlatformItem, PlatformItemData, PlatformRef, PlatformSearchOptions,
    PlatformSearchOutcome, PlatformSearchRequest, ProviderError, SsrnItemData, SsrnRef,
};

use crate::types::{
    SsrnDatePreset, SsrnSearchMode, SsrnSearchOptions, SsrnSearchScope, SsrnSort, SsrnSortOrder,
};

const NATIVE_PAGE_SIZE: u64 = 50;
const RESULTS_HOST: &str = "papers.ssrn.com";
const RESULTS_PATH: &str = "/searchresults.cfm";
const SNIPPET_SEPARATOR: &str = " … ";

/// Returns whether the route can run the request; it never starts a process.
pub(crate) fn search_support(request: &PlatformSearchRequest) -> Result<(), String> {
    match &request.options {
        PlatformSearchOptions::Ssrn(options) => {
            let defaults = crate::types::SsrnSearchOptions::default();
            let unsupported = [
                (
                    options.scope == SsrnSearchScope::Bibliographic,
                    "--scope bibliographic",
                ),
                (options.affiliation.is_some(), "--affiliation"),
                (
                    options.published != defaults.published,
                    "--published-from/--published-to",
                ),
                (
                    options.created != defaults.created,
                    "--created-from/--created-to",
                ),
                (
                    options.updated != defaults.updated,
                    "--updated-from/--updated-to",
                ),
                (options.has_abstract, "--has-abstract"),
                (options.work_type.is_some(), "--type"),
                (options.orcid.is_some(), "--orcid"),
                (options.funder.is_some(), "--funder"),
                (
                    matches!(
                        options.sort,
                        SsrnSort::Published
                            | SsrnSort::Created
                            | SsrnSort::Updated
                            | SsrnSort::Citations
                    ),
                    "--sort",
                ),
                (
                    options.sort == SsrnSort::Relevance && options.order == SsrnSortOrder::Asc,
                    "--order asc with --sort relevance",
                ),
            ]
            .into_iter()
            .filter_map(|(bad, name)| bad.then_some(name))
            .collect::<Vec<_>>();
            if !unsupported.is_empty() {
                return Err(format!(
                    "ssrn_browser does not support {}",
                    unsupported.join(", ")
                ));
            }
            page_offset(request).map(|_| ())
        }
        PlatformSearchOptions::Arxiv(_) | PlatformSearchOptions::Scholar(_) => {
            Err(other_platform_message(ROUTE, request.options.platform()))
        }
    }
}

impl SsrnBrowser {
    /// Reads the native results page that holds the requested offset. A page never spans two
    /// native pages, so it can be shorter than the limit; the next offset follows the results
    /// this page consumed.
    pub(crate) async fn search(
        &self,
        request: &PlatformSearchRequest,
    ) -> Result<PlatformSearchOutcome, ProviderError> {
        search_support(request).map_err(parameter_error)?;
        let offset = page_offset(request).map_err(parameter_error)?;
        let position = NativePosition::of(offset);
        let PlatformSearchOptions::Ssrn(options) = &request.options else {
            unreachable!("support checked")
        };
        let criteria = NativeCriteria::new(options);
        let command = self.command(
            "search",
            vec![
                ("query", request.query.clone()),
                ("page", position.page.to_string()),
                ("scope", criteria.scope.into()),
                ("mode", criteria.mode.into()),
                ("author", criteria.author.into()),
                ("date", criteria.date.into()),
                ("sort", criteria.sort.clone()),
            ],
        );
        let command = &command;
        let execution = execute_anonymous(
            self.settings(PlatformOperation::Search),
            move |deadline| async move {
                let envelope =
                    opencli::run::<ResultsPage>(command, &self.limiter, deadline).await?;
                read_results(
                    envelope.status,
                    &envelope.data,
                    &request.query,
                    position,
                    options,
                )
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
    search_state: Option<SearchState>,
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
    options: &SsrnSearchOptions,
) -> Result<VerifiedPage, String> {
    check_results_address(page, query, position.page)?;
    NativeCriteria::new(options).check(page, query, position.page, status)?;
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

#[cfg(test)]
mod tests {
    use super::{
        EnvelopeStatus, NativePosition, RawResult, ResultRange, ResultsPage, read_results,
    };

    #[test]
    fn crossref_only_criteria_and_reversed_relevance_are_rejected() {
        for value in [
            serde_json::json!({"scope":"bibliographic"}),
            serde_json::json!({"affiliation":"U"}),
            serde_json::json!({"published":{"from":"2024-01-01"}}),
            serde_json::json!({"created":{"to":"2024-01-01"}}),
            serde_json::json!({"updated":{"from":"2024-01-01"}}),
            serde_json::json!({"has_abstract":true}),
            serde_json::json!({"work_type":"posted-content"}),
            serde_json::json!({"orcid":"0000-0002-1825-0097"}),
            serde_json::json!({"funder":"10.13039/100000001"}),
            serde_json::json!({"sort":"published"}),
            serde_json::json!({"order":"asc"}),
        ] {
            let request = crate::types::PlatformSearchRequest {
                query: "momentum".into(),
                limit: 10,
                page: None,
                options: crate::types::PlatformSearchOptions::Ssrn(
                    serde_json::from_value(value.clone()).expect("criteria"),
                ),
            };
            assert!(super::search_support(&request).is_err(), "{value}");
        }
    }

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

    fn default_state(page: u64) -> super::SearchState {
        super::SearchState {
            scope: "title-abstract-keywords".into(),
            mode: "fuzzy".into(),
            author: String::new(),
            date: "All Time".into(),
            sort: Some("Relevancy".into()),
            request_url: format!(
                "https://api.ssrn.com/papers/v1/papers/search/advanced?text=dual+momentum&text_fields=title-abstract-keywords&search_mode=fuzzy&authors=&date=all_time&sort_by=&page={page}"
            ),
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
            search_state: Some(default_state(native_page)),
        }
    }

    /// Returns the consumed IDs and the next offset for `offset` and `limit`.
    fn consume(page: &ResultsPage, offset: u64, limit: u16) -> (Vec<String>, Option<u64>) {
        let verified = read_results(
            EnvelopeStatus::Ok,
            page,
            "dual momentum",
            NativePosition::of(offset),
            &crate::types::SsrnSearchOptions::default(),
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
            &crate::types::SsrnSearchOptions::default(),
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
            search_state: Some(default_state(1)),
            ..ResultsPage::default()
        };

        let result = read_results(
            EnvelopeStatus::NoResults,
            &empty,
            "dual  momentum",
            NativePosition::of(0),
            &crate::types::SsrnSearchOptions::default(),
        );

        assert_eq!(
            result.map(|page| (page.results.len(), page.has_next_page)),
            Ok((0, false))
        );
    }

    #[test]
    fn changed_controls_or_completed_requests_cannot_validate_old_results() {
        for field in [
            "scope",
            "mode",
            "author",
            "date",
            "sort",
            "request_url",
            "missing",
        ] {
            let mut page_facts = page(1, 1, 1, 1, false);
            let state = page_facts.search_state.as_mut().expect("state");
            match field {
                "scope" => state.scope = "title".into(),
                "mode" => state.mode = "boolean".into(),
                "author" => state.author = "Antonacci".into(),
                "date" => state.date = "Last Week".into(),
                "sort" => state.sort = Some("Title, A-Z".into()),
                "request_url" => {
                    state.request_url = state.request_url.replace("sort_by=", "sort_by=title-asc");
                }
                "missing" => page_facts.search_state = None,
                _ => unreachable!(),
            }
            assert!(check(&page_facts, 0).is_err(), "{field}");
        }
    }

    #[test]
    fn each_condition_must_match_the_completed_request_even_for_empty_pages() {
        for (from, to) in [
            ("text=dual+momentum", "text=other"),
            ("text_fields=title-abstract-keywords", "text_fields=title"),
            ("search_mode=fuzzy", "search_mode=boolean"),
            ("authors=", "authors=other"),
            ("date=all_time", "date=last_week"),
            ("sort_by=", "sort_by=downloads-desc"),
            ("page=1", "page=2"),
        ] {
            let mut empty = page(1, 1, 1, 1, false);
            empty.results.clear();
            let state = empty.search_state.as_mut().expect("state");
            state.request_url = state.request_url.replace(from, to);
            assert!(
                read_results(
                    EnvelopeStatus::NoResults,
                    &empty,
                    "dual momentum",
                    NativePosition::of(0),
                    &crate::types::SsrnSearchOptions::default()
                )
                .is_err(),
                "{from}"
            );
        }
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
}

#[derive(Debug, Deserialize)]
struct SearchState {
    scope: String,
    mode: String,
    author: String,
    date: String,
    sort: Option<String>,
    request_url: String,
}

struct NativeCriteria<'a> {
    scope: &'static str,
    mode: &'static str,
    author: &'a str,
    date: &'static str,
    date_label: &'static str,
    sort: String,
    sort_label: &'static str,
}

impl<'a> NativeCriteria<'a> {
    fn new(options: &'a SsrnSearchOptions) -> Self {
        let (date, date_label) = match options.date.unwrap_or(SsrnDatePreset::AllTime) {
            SsrnDatePreset::AllTime => ("all_time", "All Time"),
            SsrnDatePreset::LastWeek => ("last_week", "Last Week"),
            SsrnDatePreset::LastMonth => ("last_month", "Last Month"),
            SsrnDatePreset::Last3Months => ("last_3_months", "Last 3 Months"),
            SsrnDatePreset::Last6Months => ("last_6_months", "Last 6 Months"),
            SsrnDatePreset::LastYear => ("last_year", "Last Year"),
            SsrnDatePreset::Last2Years => ("last_2_years", "Last 2 Years"),
            SsrnDatePreset::Last3Years => ("last_3_years", "Last 3 Years"),
        };
        let ascending = options.order == SsrnSortOrder::Asc;
        let (metric, sort_label) = match options.sort {
            SsrnSort::Relevance => ("", "Relevancy"),
            SsrnSort::Downloads => (
                "downloads",
                if ascending {
                    "Downloads, Ascending"
                } else {
                    "Downloads, Descending"
                },
            ),
            SsrnSort::Posted => (
                "approval_date",
                if ascending {
                    "Date posted, Ascending"
                } else {
                    "Date posted, Descending"
                },
            ),
            SsrnSort::Title => (
                "title",
                if ascending {
                    "Title, A-Z"
                } else {
                    "Title, Z-A"
                },
            ),
            _ => unreachable!("support checked"),
        };
        Self {
            scope: match options.scope {
                SsrnSearchScope::All => "title-abstract-keywords",
                SsrnSearchScope::Title => "title",
                SsrnSearchScope::FullText => "title-abstract-keywords-fulltext",
                SsrnSearchScope::Bibliographic => unreachable!("support checked"),
            },
            mode: match options.mode.unwrap_or(SsrnSearchMode::Fuzzy) {
                SsrnSearchMode::Fuzzy => "fuzzy",
                SsrnSearchMode::Boolean => "boolean",
            },
            author: options.author.as_deref().unwrap_or(""),
            date,
            date_label,
            sort: if metric.is_empty() {
                String::new()
            } else {
                format!("{metric}-{}", if ascending { "asc" } else { "desc" })
            },
            sort_label,
        }
    }

    fn check(
        &self,
        page: &ResultsPage,
        query: &str,
        number: u64,
        status: EnvelopeStatus,
    ) -> Result<(), String> {
        let state = page
            .search_state
            .as_ref()
            .ok_or("the SSRN adapter returned no effective search state")?;
        for (name, actual, expected) in [
            ("scope", state.scope.as_str(), self.scope),
            ("mode", state.mode.as_str(), self.mode),
            ("author", state.author.as_str(), self.author),
            ("date", state.date.as_str(), self.date_label),
        ] {
            if actual != expected {
                return Err(format!(
                    "the SSRN page has {name} {actual:?}, expected {expected:?}"
                ));
            }
        }
        if state.sort.as_deref() != Some(self.sort_label)
            && !(status == EnvelopeStatus::NoResults && state.sort.is_none())
        {
            return Err("the SSRN page has a different sort order".into());
        }
        let request = reqwest::Url::parse(&state.request_url)
            .ok()
            .filter(|url| {
                url.scheme() == "https"
                    && url.host_str() == Some("api.ssrn.com")
                    && url.path() == "/papers/v1/papers/search/advanced"
            })
            .ok_or("the SSRN page has no completed native search request")?;
        let address = reqwest::Url::parse(&page.url).map_err(|error| error.to_string())?;
        let number = number.to_string();
        for (name, expected, default) in [
            ("text", query, ""),
            ("text_fields", self.scope, "title-abstract-keywords"),
            ("search_mode", self.mode, "fuzzy"),
            ("authors", self.author, ""),
            ("date", self.date, "all_time"),
            ("sort_by", self.sort.as_str(), ""),
            ("page", number.as_str(), "1"),
        ] {
            let actual = request
                .query_pairs()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.into_owned());
            if actual.as_deref().map(fold_whitespace) != Some(fold_whitespace(expected)) {
                return Err(format!("the completed SSRN request has different {name}"));
            }
            let key = if name == "text" { "term" } else { name };
            let actual = address
                .query_pairs()
                .find(|(name, _)| name == key)
                .map_or_else(|| default.to_owned(), |(_, value)| value.into_owned());
            if fold_whitespace(&actual) != fold_whitespace(expected) {
                return Err(format!("the SSRN results URL has different {key}"));
            }
        }
        Ok(())
    }
}
