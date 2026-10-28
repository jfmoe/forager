//! SSRN native search: the request conditions, the check of the request the adapter sent, and
//! the decoding and pagination of the native search API's answer.

use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;
use serde_json::Value;

use super::{ROUTE, SsrnBrowser, names, runtime};
use crate::catalog::PlatformOperation;
use crate::net::AttemptFailure;
use crate::providers::execution::execute_anonymous;
use crate::providers::markup::markup_text;
use crate::providers::opencli::{EnvelopeStatus, Window};
use crate::providers::shared::{other_platform_message, parameter_error};
use crate::types::{
    AttemptErrorKind, ContentDepth, PlatformItem, PlatformItemData, PlatformRef,
    PlatformSearchOptions, PlatformSearchOutcome, PlatformSearchRequest, ProviderError,
    SsrnDatePreset, SsrnItemData, SsrnRef, SsrnSearchMode, SsrnSearchOptions, SsrnSearchScope,
    SsrnSort, SsrnSortOrder,
};

const NATIVE_PAGE_SIZE: u64 = 50;
/// SSRN lists at most this many results for a search, whatever its total.
const RESULT_LIMIT: u64 = 10_000;
const SEARCH_HOST: &str = "api.ssrn.com";
const SEARCH_PATH: &str = "/papers/v1/papers/search/advanced";
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
        PlatformSearchOptions::Arxiv(_)
        | PlatformSearchOptions::Scholar(_)
        | PlatformSearchOptions::Xiaohongshu(_)
        | PlatformSearchOptions::X(_)
        | PlatformSearchOptions::Hn(_)
        | PlatformSearchOptions::Polymarket(_)
        | PlatformSearchOptions::Reddit(_)
        | PlatformSearchOptions::Youtube(_) => {
            Err(other_platform_message(ROUTE, request.options.platform()))
        }
    }
}

impl SsrnBrowser {
    /// Requests the native results page that holds the requested offset. A page never spans two
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
        let command = self.opencli.command(
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
            Window::Background,
        );
        let command = &command;
        let execution = execute_anonymous(
            self.settings(PlatformOperation::Search),
            move |deadline| async move {
                let envelope = self.opencli.run::<SearchAnswer>(command, deadline).await?;
                read_results(
                    envelope.status,
                    &envelope.data,
                    &request.query,
                    position,
                    options,
                )
                .map(|page| (None, page))
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
    let offset = request.page.as_deref().map_or(Ok(0), |page| {
        page.parse::<u64>()
            .map_err(|_| format!("invalid SSRN page position `{page}`"))
    })?;
    if offset >= RESULT_LIMIT {
        return Err(format!(
            "SSRN page position {offset} is past the {RESULT_LIMIT} results SSRN lists"
        ));
    }
    Ok(offset)
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

/// The facts the adapter reports about the native search request it sent.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SearchAnswer {
    /// The address the adapter requested, with every native condition.
    request_url: String,
    http_status: Option<u16>,
    content_type: Option<String>,
    /// The parsed JSON body, or the body text when it is not JSON.
    body: Value,
    /// The error the browser raised when the request could not complete.
    network_error: Option<String>,
}

/// The native search API's answer: one page of `papers` out of `total`.
#[derive(Debug, Deserialize)]
struct NativeResults {
    total: u64,
    papers: Vec<NativePaper>,
}

// The API sends `null` for fields it has no value for, such as the snippets of a title search.
#[derive(Debug, Deserialize)]
struct NativePaper {
    id: u64,
    /// HTML, with query terms marked as `<em>`.
    title: Option<String>,
    authors: Option<Vec<NativeAuthor>>,
    /// When SSRN posted the paper, as an RFC 3339 UTC time.
    approved_date: Option<String>,
    revision_date: Option<String>,
    snippets: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct NativeAuthor {
    full_name: Option<String>,
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
    posted: Option<NaiveDate>,
    last_revised: Option<NaiveDate>,
    snippet: Option<String>,
}

impl SearchResult {
    fn read(paper: NativePaper) -> Result<Self, String> {
        let reference = SsrnRef::parse(&format!("ssrn:{}", paper.id))
            .map_err(|_| format!("an SSRN result has the ID {}, not a paper ID", paper.id))?;
        let title = markup_text(paper.title.as_deref().unwrap_or_default());
        if title.is_empty() {
            return Err(format!("the SSRN result for ssrn:{reference} has no title"));
        }
        let authors = paper
            .authors
            .unwrap_or_default()
            .into_iter()
            .filter_map(|author| author.full_name)
            .collect::<Vec<_>>();
        let snippets = paper
            .snippets
            .unwrap_or_default()
            .iter()
            .map(|snippet| markup_text(snippet))
            .filter(|snippet| !snippet.is_empty())
            .collect::<Vec<_>>();
        let date = |value: Option<String>| {
            value
                .map(|value| {
                    utc_date(&value).ok_or_else(|| {
                        format!("the SSRN result for ssrn:{reference} has the date {value:?}")
                    })
                })
                .transpose()
        };
        Ok(Self {
            posted: date(paper.approved_date)?,
            last_revised: date(paper.revision_date)?,
            reference,
            title,
            authors: names(&authors),
            snippet: (!snippets.is_empty()).then(|| snippets.join(SNIPPET_SEPARATOR)),
        })
    }

    fn to_item(&self) -> PlatformItem {
        PlatformItem {
            url: self.reference.canonical_url(),
            reference: PlatformRef::Ssrn(self.reference.clone()),
            depth: if self.snippet.is_some() {
                ContentDepth::Snippet
            } else {
                ContentDepth::Metadata
            },
            title: Some(self.title.clone()),
            authors: self.authors.clone(),
            published: self.posted.map(|date| date.format("%Y-%m-%d").to_string()),
            data: PlatformItemData::Ssrn(SsrnItemData {
                snippet: self.snippet.clone(),
                posted: self.posted.map(site_date),
                last_revised: self.last_revised.map(site_date),
                ..SsrnItemData::default()
            }),
        }
    }
}

/// Returns the UTC calendar date of an RFC 3339 time, the date SSRN's paper pages show.
fn utc_date(value: &str) -> Option<NaiveDate> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|time| time.with_timezone(&Utc).date_naive())
}

/// Formats a date as SSRN pages show it, such as `07 Sep 2019`.
fn site_date(date: NaiveDate) -> String {
    date.format("%d %b %Y").to_string()
}

/// Returns how many results SSRN lists on native page `page` of a search with `total` results.
fn listed_count(total: u64, page: u64) -> u64 {
    total
        .min(RESULT_LIMIT)
        .saturating_sub((page - 1) * NATIVE_PAGE_SIZE)
        .min(NATIVE_PAGE_SIZE)
}

/// Returns whether SSRN lists results after native page `page` of a search with `total` results.
fn has_page_after(total: u64, page: u64) -> bool {
    page * NATIVE_PAGE_SIZE < total.min(RESULT_LIMIT)
}

/// Checks that the adapter sent the native request for the query, criteria, and native page,
/// and decodes the answer. An answer that is neither a recognizable results page nor the site's
/// own empty result fails.
fn read_results(
    status: EnvelopeStatus,
    answer: &SearchAnswer,
    query: &str,
    position: NativePosition,
    options: &SsrnSearchOptions,
) -> Result<VerifiedPage, AttemptFailure> {
    NativeCriteria::new(options)
        .check_request(&answer.request_url, query, position.page)
        .map_err(runtime)?;
    if let Some(error) = &answer.network_error {
        return Err(AttemptFailure {
            kind: AttemptErrorKind::Network,
            status: None,
            message: format!("the browser could not reach the SSRN search API: {error}"),
        });
    }
    match answer.http_status {
        Some(200) => {}
        Some(code) => {
            return Err(AttemptFailure {
                kind: AttemptErrorKind::Runtime,
                status: Some(code),
                message: format!("the SSRN search API answered HTTP {code}"),
            });
        }
        None => return Err(runtime("the adapter reported no HTTP status".into())),
    }
    let results = NativeResults::deserialize(&answer.body).map_err(|error| {
        runtime(format!(
            "the SSRN search API answered an unrecognized body ({}): {error}",
            answer.content_type.as_deref().unwrap_or("no content type")
        ))
    })?;
    read_native_page(status, results, position).map_err(runtime)
}

fn read_native_page(
    status: EnvelopeStatus,
    results: NativeResults,
    position: NativePosition,
) -> Result<VerifiedPage, String> {
    let empty = results.total == 0 && results.papers.is_empty();
    if empty != (status == EnvelopeStatus::NoResults) {
        return Err(format!(
            "the adapter reported {status:?} for {} results",
            results.total
        ));
    }
    if empty {
        return Ok(VerifiedPage {
            results: Vec::new(),
            has_next_page: false,
            start: 0,
        });
    }
    let listed = results.papers.len() as u64;
    let expected = listed_count(results.total, position.page);
    if listed != expected {
        return Err(format!(
            "SSRN page {} lists {listed} results; {} results leave {expected} for it",
            position.page, results.total
        ));
    }
    if position.start >= listed {
        return Err(format!(
            "SSRN page {} has {listed} results; the requested position is past them",
            position.page
        ));
    }
    Ok(VerifiedPage {
        has_next_page: has_page_after(results.total, position.page),
        results: results
            .papers
            .into_iter()
            .map(SearchResult::read)
            .collect::<Result<_, _>>()?,
        start: usize::try_from(position.start).unwrap_or(usize::MAX),
    })
}

/// The native search conditions of SSRN Search Criteria, as SSRN's search API names them.
struct NativeCriteria<'a> {
    scope: &'static str,
    mode: &'static str,
    author: &'a str,
    date: &'static str,
    sort: String,
}

impl<'a> NativeCriteria<'a> {
    fn new(options: &'a SsrnSearchOptions) -> Self {
        let ascending = options.order == SsrnSortOrder::Asc;
        let metric = match options.sort {
            SsrnSort::Relevance => None,
            SsrnSort::Downloads => Some("downloads"),
            SsrnSort::Posted => Some("approval_date"),
            SsrnSort::Title => Some("title"),
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
            date: match options.date.unwrap_or(SsrnDatePreset::AllTime) {
                SsrnDatePreset::AllTime => "all_time",
                SsrnDatePreset::LastWeek => "last_week",
                SsrnDatePreset::LastMonth => "last_month",
                SsrnDatePreset::Last3Months => "last_3_months",
                SsrnDatePreset::Last6Months => "last_6_months",
                SsrnDatePreset::LastYear => "last_year",
                SsrnDatePreset::Last2Years => "last_2_years",
                SsrnDatePreset::Last3Years => "last_3_years",
            },
            sort: metric.map_or_else(String::new, |metric| {
                format!("{metric}-{}", if ascending { "asc" } else { "desc" })
            }),
        }
    }

    /// Checks that `url` is the native search request for `query` on native page `page`, with
    /// exactly the conditions of these criteria and no other parameter.
    fn check_request(&self, url: &str, query: &str, page: u64) -> Result<(), String> {
        let request = reqwest::Url::parse(url)
            .ok()
            .filter(|url| {
                url.scheme() == "https"
                    && url.host_str() == Some(SEARCH_HOST)
                    && url.path() == SEARCH_PATH
            })
            .ok_or_else(|| format!("the adapter requested `{url}`, not the SSRN search API"))?;
        let page = page.to_string();
        let expected = [
            ("text", query),
            ("text_fields", self.scope),
            ("search_mode", self.mode),
            ("authors", self.author),
            ("date", self.date),
            ("sort_by", self.sort.as_str()),
            ("page", page.as_str()),
        ];
        let sent_values = |name: &str| {
            request
                .query_pairs()
                .filter(|(key, _)| key == name)
                .map(|(_, value)| value.into_owned())
                .collect::<Vec<_>>()
        };
        if let Some((name, _)) = expected
            .iter()
            .find(|(name, value)| sent_values(name) != [*value])
        {
            return Err(format!("the SSRN search request has different {name}"));
        }
        if request.query_pairs().count() != expected.len() {
            return Err("the SSRN search request has other parameters".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        NativeCriteria, NativePosition, has_page_after, listed_count, site_date, utc_date,
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

    #[test]
    fn native_times_become_their_utc_calendar_date_in_the_site_format() {
        let dates = [
            "2019-09-07T19:00:38.000Z",
            "2019-09-08T03:00:38+08:00",
            "2012-04-19T23:59:59Z",
            "2000-07-05T00:00:00Z",
            "2019-09-07",
            "07 Sep 2019",
        ]
        .map(|value| utc_date(value).map(site_date));

        assert_eq!(
            dates,
            [
                Some("07 Sep 2019".to_owned()),
                Some("07 Sep 2019".to_owned()),
                Some("19 Apr 2012".to_owned()),
                Some("05 Jul 2000".to_owned()),
                None,
                None
            ]
        );
    }

    #[test]
    fn native_pages_end_at_the_total_or_the_site_result_limit() {
        let pages = [
            (120, 1),
            (120, 3),
            (120, 4),
            (100, 2),
            (10_000, 200),
            (25_000, 200),
            (25_000, 201),
        ]
        .map(|(total, page)| (listed_count(total, page), has_page_after(total, page)));

        assert_eq!(
            pages,
            [
                (50, true),
                (20, false),
                (0, false),
                (50, false),
                (50, false),
                (50, false),
                (0, false)
            ]
        );
    }

    #[test]
    fn every_native_condition_must_match_the_sent_request() {
        let options = crate::types::SsrnSearchOptions::default();
        let criteria = NativeCriteria::new(&options);
        let sent = "https://api.ssrn.com/papers/v1/papers/search/advanced?text=dual+momentum&text_fields=title-abstract-keywords&search_mode=fuzzy&authors=&date=all_time&sort_by=&page=2";
        let mut checks = vec![criteria.check_request(sent, "dual momentum", 2)];
        for (from, to) in [
            ("text=dual+momentum", "text=other"),
            ("text_fields=title-abstract-keywords", "text_fields=title"),
            ("search_mode=fuzzy", "search_mode=boolean"),
            ("authors=", "authors=other"),
            ("date=all_time", "date=last_week"),
            ("sort_by=", "sort_by=downloads-desc"),
            ("page=2", "page=1"),
            ("&sort_by=", "&other="),
            ("https://api.ssrn.com", "https://papers.ssrn.com"),
            ("https://", "http://"),
            ("&page=2", "&page=2&_forager_search=1"),
            ("&page=2", "&page=2&page=2"),
            ("text=dual+momentum", "text=dual++momentum"),
        ] {
            checks.push(criteria.check_request(&sent.replacen(from, to, 1), "dual momentum", 2));
        }

        assert_eq!(
            checks.iter().map(Result::is_ok).collect::<Vec<_>>(),
            [[true].as_slice(), &[false; 13]].concat()
        );
    }
}
