//! Xiaohongshu search: driving the search from the site's home page with native input, then
//! judging what it observed: page-fact classification, condition checks, and note-card
//! decoding.

use std::collections::HashSet;
use std::time::Duration;

use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta};
use reqwest::Url;
use serde::Deserialize;

use super::page::{Observed, Reading, Tracked, input_failure, page_script, site_capture};
use super::{
    HOME_URL, PageFacts, ROUTE, SITE_HOST, XiaohongshuBrowser, classify_blocks, describe, runtime,
    unfinished_read,
};
use crate::catalog::PlatformOperation;
use crate::net::{AttemptFailure, combine_diagnostics};
use crate::providers::browser::{
    CaptureLevel, Exchange, InputError, InputFailure, Key, Missed, Page, RequestMethod,
    ResponseBody,
};
use crate::providers::execution::execute_anonymous;
use crate::providers::shared::{other_platform_message, parameter_error};
use crate::types::{
    AccessToken, ContentDepth, PlatformItem, PlatformItemData, PlatformRef, PlatformSearchOptions,
    PlatformSearchOutcome, PlatformSearchRequest, ProviderError, XiaohongshuItemData,
    XiaohongshuNoteType, XiaohongshuPublishTime, XiaohongshuRef, XiaohongshuSearchOptions,
    XiaohongshuSort,
};

const PAGE_SIZE: u16 = 20;
const MAX_PAGES: usize = 5;
// The results page of the older layout, and of the `/search_result_ai` layout.
const RESULTS_PATHS: [&str; 2] = ["/search_result", "/search_result_ai"];
const SEARCH: Tracked = Tracked {
    path: "/api/sns/web/v2/search/notes",
    method: RequestMethod::Post,
};
const SORT_FILTER: &str = "sort_type";
const NOTE_TYPE_FILTER: &str = "filter_note_type";
const TIME_FILTER: &str = "filter_note_time";
// The filter groups forager never sets; they depend on browsing history and location.
const UNSET_FILTERS: [&str; 2] = ["filter_note_range", "filter_pos_distance"];
const ANY: &str = "不限";

// The home page's search box in the `/search_result_ai` layout, then in the older layout.
const SEARCH_BOXES: [&str; 2] = [
    "document.querySelectorAll('textarea#search-input-in-feeds')",
    "document.querySelectorAll('input#search-input')",
];
// The filter button in either layout. The panel opens on hover, and a click also opens it when
// the hover does not.
const FILTER_BUTTON: &str =
    "document.querySelectorAll('.filter.ai-chat-filter, .search-layout__top > .filter')";
const PANEL_OPEN: &str = "[...document.querySelectorAll('.filter-panel')].some((element) => { const rect = element.getBoundingClientRect(); const style = getComputedStyle(element); return rect.width > 0 && rect.height > 0 && style.display !== 'none' && style.visibility !== 'hidden'; })";
const PANEL_AFTER_HOVER: Duration = Duration::from_secs(1);
const PANEL_AFTER_CLICK: Duration = Duration::from_millis(1500);
// How long a filter click has to show its effect: the option selected or a new results page.
const CLICK_CHECK: Duration = Duration::from_secs(5);

/// Returns whether the route can run the request; it never starts a process. Xiaohongshu
/// search issues no cursor, so only a first page can run.
pub(crate) fn search_support(request: &PlatformSearchRequest) -> Result<(), String> {
    match &request.options {
        PlatformSearchOptions::Xiaohongshu(_) => {
            if request.page.is_some() {
                return Err(format!(
                    "{} cannot continue a search from a page position; Xiaohongshu search has no cursor",
                    ROUTE.name()
                ));
            }
            Ok(())
        }
        PlatformSearchOptions::Arxiv(_)
        | PlatformSearchOptions::Ssrn(_)
        | PlatformSearchOptions::Scholar(_)
        | PlatformSearchOptions::X(_)
        | PlatformSearchOptions::Hn(_)
        | PlatformSearchOptions::Polymarket(_)
        | PlatformSearchOptions::Reddit(_)
        | PlatformSearchOptions::Youtube(_) => {
            Err(other_platform_message(ROUTE, request.options.platform()))
        }
    }
}

impl XiaohongshuBrowser {
    /// Searches from the site's home page and reads the results pages that `limit` needs in
    /// one browser command. Xiaohongshu ranks differently on every visit, so the route issues
    /// no cursor and reports in a diagnostic when results remain.
    pub(crate) async fn search(
        &self,
        request: &PlatformSearchRequest,
    ) -> Result<PlatformSearchOutcome, ProviderError> {
        search_support(request).map_err(parameter_error)?;
        let PlatformSearchOptions::Xiaohongshu(options) = &request.options else {
            unreachable!("support checked")
        };
        let query = request.query.trim();
        let pages = usize::from(request.limit.div_ceil(PAGE_SIZE)).clamp(1, MAX_PAGES);
        let execution = execute_anonymous(
            self.settings(PlatformOperation::Search.as_str()),
            move |deadline| async move {
                let data = self
                    .browser
                    .read(capture, deadline, async |page| {
                        observe_search(page, query, options, pages).await
                    })
                    .await?;
                let decoded = search_outcome(&data, query, options, request.limit, local_now())?;
                Ok((None, decoded))
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

fn capture(url: &Url) -> CaptureLevel {
    site_capture(url, &[SEARCH.path])
}

/// The filter panel's group and option text of each non-default condition, in the order they
/// are set.
fn filter_choices(options: &XiaohongshuSearchOptions) -> Vec<(&'static str, &'static str)> {
    let sort = match options.sort {
        XiaohongshuSort::Comprehensive => None,
        XiaohongshuSort::Latest => Some("最新"),
        XiaohongshuSort::MostLiked => Some("最多点赞"),
        XiaohongshuSort::MostCommented => Some("最多评论"),
        XiaohongshuSort::MostCollected => Some("最多收藏"),
    };
    let note_type = match options.note_type {
        XiaohongshuNoteType::All => None,
        XiaohongshuNoteType::Image => Some("图文"),
        XiaohongshuNoteType::Video => Some("视频"),
    };
    let time = match options.publish_time {
        XiaohongshuPublishTime::Any => None,
        XiaohongshuPublishTime::Day => Some("一天内"),
        XiaohongshuPublishTime::Week => Some("一周内"),
        XiaohongshuPublishTime::HalfYear => Some("半年内"),
    };
    [
        ("排序依据", sort),
        ("笔记类型", note_type),
        ("发布时间", time),
    ]
    .into_iter()
    .filter_map(|(group, option)| option.map(|option| (group, option)))
    .collect()
}

/// What the search drive did besides reading the page.
#[derive(Default)]
struct SearchProgress {
    filter_clicks: u32,
    filter_failure: Option<String>,
    search_failure: Option<String>,
    /// Whether the query was submitted from the home page.
    submitted: bool,
    /// For each filter click, how many exchanges the read had kept before it.
    marks: Vec<usize>,
}

/// Drives one search: opens the home page, types the query into its search box, presses
/// Enter, sets each requested filter in the filter panel, and scrolls the results for the
/// `pages` the request needs. Returns what the page showed and answered.
async fn observe_search(
    page: &mut Page,
    query: &str,
    options: &XiaohongshuSearchOptions,
    pages: usize,
) -> Result<SearchData, AttemptFailure> {
    let mut reading = Reading::new(page);
    let mut progress = SearchProgress::default();
    drive_search(&mut reading, query, options, pages, &mut progress).await?;
    search_data(reading.finish()?, progress)
}

async fn drive_search(
    reading: &mut Reading<'_>,
    query: &str,
    options: &XiaohongshuSearchOptions,
    pages: usize,
    progress: &mut SearchProgress,
) -> Result<(), AttemptFailure> {
    reading.navigate(HOME_URL).await?;
    if !reading.loaded().await? {
        return Ok(());
    }
    match enter_query(reading.page(), query).await? {
        None => progress.submitted = true,
        Some(missed) if missed.failure == InputFailure::DeadlinePassed => {
            reading.mark_timed_out();
            return Ok(());
        }
        Some(missed) => {
            progress.search_failure = Some(format!(
                "{} for the search box",
                missed.describe(missed.failure.reason())
            ));
            return Ok(());
        }
    }
    if !reading.next(SEARCH, 0, false).await? {
        return Ok(());
    }
    // The responses that answer the last filter click come after the ones completed before it.
    let mut answered_from = 0;
    for (group, option) in filter_choices(options) {
        // The first screen requests its second page by itself; a click before that page
        // arrives would count it as an answer to the click.
        if !reading.settle(SEARCH).await? {
            return Ok(());
        }
        let seen = reading.completions(SEARCH);
        if let Some(reason) = choose_option(reading, group, option, seen, progress).await? {
            progress.filter_failure = Some(format!("{reason} for {group} {option}"));
            return Ok(());
        }
        progress.filter_clicks += 1;
        answered_from = seen;
        if !reading.next(SEARCH, seen, false).await? {
            return Ok(());
        }
    }
    while wants_another_page(reading.exchanges(), answered_from, pages) {
        if !reading.more(SEARCH).await? {
            break;
        }
    }
    Ok(())
}

/// Whether the search must scroll for another results page: fewer than `pages` responses have
/// completed after the first `answered_from`, which came before the last filter click, and the
/// last one says more results remain. The page the first screen requests by itself counts.
fn wants_another_page(exchanges: &[Exchange], answered_from: usize, pages: usize) -> bool {
    let answered = SEARCH
        .completed(exchanges)
        .count()
        .saturating_sub(answered_from);
    answered < pages && last_page_has_more(exchanges)
}

/// Types the query into the home page's search box and submits it; returns why it could not.
async fn enter_query(page: &mut Page, query: &str) -> Result<Option<Missed>, AttemptFailure> {
    let mut missed = None;
    for search_box in SEARCH_BOXES {
        missed = input_failure(page.click(search_box).await)?;
        if missed
            .as_ref()
            .is_none_or(|missed| missed.failure != InputFailure::NoTarget)
        {
            break;
        }
    }
    if missed.is_some() {
        return Ok(missed);
    }
    if let Some(failure) = input_failure(page.type_text(query).await)? {
        return Ok(Some(failure));
    }
    input_failure(page.press(Key::Enter).await)
}

/// Opens the filter panel with the pointer and clicks one option, then confirms the click took
/// effect. Returns why it could not.
async fn choose_option(
    reading: &mut Reading<'_>,
    group: &str,
    option: &str,
    seen: usize,
    progress: &mut SearchProgress,
) -> Result<Option<String>, AttemptFailure> {
    let button = |missed: Missed| {
        missed.describe(match missed.failure {
            InputFailure::NoTarget => "no_filter_button",
            InputFailure::AmbiguousTarget => "ambiguous_filter_button",
            other => other.reason(),
        })
    };
    if !reading.page().evaluate::<bool>(PANEL_OPEN).await? {
        if let Some(missed) = input_failure(reading.page().hover(FILTER_BUTTON).await)? {
            return Ok(Some(button(missed)));
        }
        if !panel_opens(reading.page(), PANEL_AFTER_HOVER).await? {
            if let Some(missed) = input_failure(reading.page().click(FILTER_BUTTON).await)? {
                return Ok(Some(button(missed)));
            }
            if !panel_opens(reading.page(), PANEL_AFTER_CLICK).await? {
                return Ok(Some("no_filter_panel".into()));
            }
        }
    }
    let group_shown = format!("{}.length === 1", groups_expression(group, true));
    if !reading.page().evaluate::<bool>(&group_shown).await? {
        return Ok(Some("no_filter_group".into()));
    }
    progress.marks.push(reading.exchanges().len());
    let clicked = reading
        .page()
        .click(&options_expression(group, option, true))
        .await;
    let missed = input_failure(clicked)?.map(|missed| {
        missed.describe(match missed.failure {
            InputFailure::NoTarget => "no_filter_option",
            InputFailure::AmbiguousTarget => "ambiguous_filter_option",
            other => other.reason(),
        })
    });
    if missed.is_none()
        && reading
            .confirm(SEARCH, seen, &selected_script(group, option), CLICK_CHECK)
            .await?
    {
        return Ok(None);
    }
    progress.marks.pop();
    Ok(Some(missed.unwrap_or_else(|| "click_missed".into())))
}

async fn panel_opens(page: &Page, within: Duration) -> Result<bool, AttemptFailure> {
    match page.confirm(PANEL_OPEN, within).await {
        Ok(()) => Ok(true),
        Err(InputError::Failed(_)) => Ok(false),
        Err(InputError::Transport(failure)) => Err(failure),
    }
}

/// A page expression for the filter groups labelled `group`, in an open panel when `shown`.
fn groups_expression(group: &str, shown: bool) -> String {
    format!(
        r"(() => {{
    const text = (element) => (element ? element.textContent : '').replace(/\s+/g, '');
    return [...document.querySelectorAll('.filter-panel')]
      .filter((panel) => !{shown} || panel.getBoundingClientRect().height > 0)
      .flatMap((panel) => [...panel.querySelectorAll('.filters')])
      .filter((element) => text([...element.children].find((child) => child.tagName === 'SPAN')) === {group});
  }})()",
        group = serde_json::Value::from(group),
    )
}

/// A page expression for the options with the text `option` in the one group `group`. The
/// panel renders every option twice; the native click picks the one a pointer hits.
fn options_expression(group: &str, option: &str, shown: bool) -> String {
    format!(
        r"(() => {{
    const groups = {groups};
    if (groups.length !== 1) return [];
    return [...groups[0].querySelectorAll('.tags')]
      .filter((element) => (element.textContent || '').replace(/\s+/g, '') === {option});
  }})()",
        groups = groups_expression(group, shown),
        option = serde_json::Value::from(option),
    )
}

fn selected_script(group: &str, option: &str) -> String {
    page_script(&format!(
        r#"return [...{options}].some((element) => !element.closest('[aria-hidden="true"]')
      && (/\b(active|selected|checked)\b/.test(element.className)
        || ['aria-selected', 'aria-checked', 'aria-pressed'].some((name) => element.getAttribute(name) === 'true')));"#,
        options = options_expression(group, option, false),
    ))
}

/// Whether the last search response with a body says more results remain.
fn last_page_has_more(exchanges: &[Exchange]) -> bool {
    exchanges
        .iter()
        .rev()
        .filter(|exchange| SEARCH.matches(exchange))
        .find_map(|exchange| match &exchange.body {
            ResponseBody::Received(body) => Some(serde_json::from_str::<SearchBody>(body).ok()),
            _ => None,
        })
        .flatten()
        .and_then(|body| body.data)
        .is_some_and(|page| page.has_more)
}

/// Builds the search observation from what the read kept: every `search/notes` exchange with
/// a body, tagged with the filter clicks made before it.
fn search_data(observed: Observed, progress: SearchProgress) -> Result<SearchData, AttemptFailure> {
    let mut responses = Vec::new();
    for (index, exchange) in observed.exchanges.iter().enumerate() {
        let ResponseBody::Received(body) = &exchange.body else {
            continue;
        };
        if !SEARCH.matches(exchange) {
            continue;
        }
        let request = exchange
            .request_body
            .as_deref()
            .and_then(|request| serde_json::from_str(request).ok())
            .ok_or_else(|| {
                runtime("Xiaohongshu sent a search request without a readable body".into())
            })?;
        let body = serde_json::from_str(body).unwrap_or_else(|_| SearchBody {
            msg: Some("not a search results body".into()),
            data: None,
        });
        let clicks = progress.marks.iter().filter(|mark| **mark <= index).count();
        responses.push(CapturedSearch {
            click: u32::try_from(clicks).unwrap_or(u32::MAX),
            request,
            body,
        });
    }
    Ok(SearchData {
        page: observed.page,
        filter_clicks: progress.filter_clicks,
        filter_failure: progress.filter_failure,
        search_failure: progress.search_failure,
        before_search: observed.timed_out && !progress.submitted,
        timed_out: observed.timed_out,
        body_missing: observed.body_missing,
        responses,
    })
}

/// The filter tags the search requests must carry, and how many filter clicks set them.
struct Conditions {
    tags: [(&'static str, &'static str); 3],
    clicks: u32,
}

impl Conditions {
    fn of(options: &XiaohongshuSearchOptions) -> Self {
        let sort = match options.sort {
            XiaohongshuSort::Comprehensive => "general",
            XiaohongshuSort::Latest => "time_descending",
            XiaohongshuSort::MostLiked => "popularity_descending",
            XiaohongshuSort::MostCommented => "comment_descending",
            XiaohongshuSort::MostCollected => "collect_descending",
        };
        let note_type = match options.note_type {
            XiaohongshuNoteType::All => ANY,
            XiaohongshuNoteType::Image => "普通笔记",
            XiaohongshuNoteType::Video => "视频笔记",
        };
        let time = match options.publish_time {
            XiaohongshuPublishTime::Any => ANY,
            XiaohongshuPublishTime::Day => "一天内",
            XiaohongshuPublishTime::Week => "一周内",
            XiaohongshuPublishTime::HalfYear => "半年内",
        };
        let defaults = XiaohongshuSearchOptions::default();
        let clicks = [
            options.sort != defaults.sort,
            options.note_type != defaults.note_type,
            options.publish_time != defaults.publish_time,
        ]
        .into_iter()
        .filter(|changed| *changed)
        .count();
        Self {
            tags: [
                (SORT_FILTER, sort),
                (NOTE_TYPE_FILTER, note_type),
                (TIME_FILTER, time),
            ],
            clicks: u32::try_from(clicks).unwrap_or(u32::MAX),
        }
    }

    /// Returns the first filter group whose effective tag differs, with that tag. A request
    /// without `filters` carries every default.
    fn mismatch(&self, filters: &[Filter]) -> Option<(&'static str, String)> {
        let tag = |group: &str| {
            filters
                .iter()
                .find(|filter| filter.kind == group)
                .and_then(|filter| filter.tags.first())
                .cloned()
        };
        self.tags
            .iter()
            .find_map(|(group, expected)| {
                let default = if *group == SORT_FILTER {
                    "general"
                } else {
                    ANY
                };
                let effective = tag(group).unwrap_or_else(|| default.to_owned());
                (effective != *expected).then_some((*group, effective))
            })
            .or_else(|| {
                UNSET_FILTERS.iter().find_map(|group| {
                    tag(group)
                        .filter(|effective| effective != ANY)
                        .map(|effective| (*group, effective))
                })
            })
    }
}

/// What one search command observed.
#[derive(Debug, Default)]
#[cfg_attr(test, derive(Deserialize))]
#[cfg_attr(test, serde(default))]
struct SearchData {
    page: PageFacts,
    /// How many filter clicks the page logic made.
    filter_clicks: u32,
    /// Why the page logic could not make the next filter click.
    filter_failure: Option<String>,
    /// Why the page logic could not enter the search on the home page.
    search_failure: Option<String>,
    /// The read deadline passed on the home page, before the query was submitted.
    before_search: bool,
    /// The read deadline passed before the expected responses arrived.
    timed_out: bool,
    /// An awaited search response finished loading, but the browser returned no body for it.
    body_missing: bool,
    /// Every captured `search/notes` exchange with a body, in the order the responses ended.
    responses: Vec<CapturedSearch>,
}

#[derive(Debug, Deserialize)]
struct CapturedSearch {
    /// The number of filter clicks made before the request.
    click: u32,
    request: SearchRequestFacts,
    body: SearchBody,
}

#[derive(Debug, Deserialize)]
struct SearchRequestFacts {
    keyword: String,
    page: u32,
    search_id: String,
    #[serde(default)]
    filters: Option<Vec<Filter>>,
}

#[derive(Debug, Deserialize)]
struct Filter {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct SearchBody {
    #[serde(default)]
    msg: Option<String>,
    #[serde(default)]
    data: Option<SearchPage>,
}

#[derive(Debug, Deserialize)]
struct SearchPage {
    has_more: bool,
    items: Vec<serde_json::Value>,
}

/// Judges one search observation: classifies the page facts, checks that the responses answer
/// `query` with the conditions of `options`, and decodes the notes that `limit` keeps. `now` is
/// the request time in the system local timezone.
fn search_outcome(
    data: &SearchData,
    query: &str,
    options: &XiaohongshuSearchOptions,
    limit: u16,
    now: NaiveDateTime,
) -> Result<Decoded, AttemptFailure> {
    let expected = Conditions::of(options);
    let pages = usize::from(limit.div_ceil(PAGE_SIZE));
    let verified = read_search(data, query, &expected, pages)?;
    decode(&verified, limit, now).map_err(runtime)
}

/// The search pages that answer the request, in page order.
#[derive(Debug)]
struct VerifiedPages {
    pages: Vec<SearchPage>,
}

/// Classifies the page facts and returns the responses that answer the request: those after
/// the last filter click, for the requested query and conditions, pages 1, 2, … of one search.
fn read_search(
    data: &SearchData,
    query: &str,
    expected: &Conditions,
    pages: usize,
) -> Result<VerifiedPages, AttemptFailure> {
    let facts = &data.page;
    classify_blocks(facts, |code, notice| {
        format!(
            "Xiaohongshu blocked the page ({code}: {notice}); the account may be rate-limited, so stop and retry later"
        )
    })?;
    if data.body_missing {
        return Err(runtime(format!(
            "a Xiaohongshu search response arrived without its body on {}",
            describe(facts)
        )));
    }
    let clicked = data.filter_clicks == expected.clicks;
    let answering = data
        .responses
        .iter()
        .filter(|response| response.click == data.filter_clicks)
        .collect::<Vec<_>>();
    let complete = answering.len() >= pages
        || answering
            .last()
            .and_then(|response| response.body.data.as_ref())
            .is_some_and(|page| !page.has_more);
    if !clicked || answering.is_empty() || !complete {
        return Err(incomplete(data, expected, answering.len(), pages));
    }
    let first_search_id = &answering[0].request.search_id;
    let mut verified = Vec::new();
    for (index, response) in answering.iter().take(pages).enumerate() {
        let request = &response.request;
        if request.keyword != query {
            return Err(runtime(format!(
                "Xiaohongshu searched for {:?}, not {query:?}",
                request.keyword
            )));
        }
        if let Some((group, tag)) = expected.mismatch(request.filters.as_deref().unwrap_or(&[])) {
            return Err(runtime(format!(
                "Xiaohongshu applied `{tag}` for {group}, not the requested condition"
            )));
        }
        let expected_page = u32::try_from(index + 1).unwrap_or(u32::MAX);
        if request.page != expected_page {
            return Err(runtime(format!(
                "Xiaohongshu answered page {} where page {expected_page} was due",
                request.page
            )));
        }
        if &request.search_id != first_search_id {
            return Err(runtime(
                "the Xiaohongshu search changed its search_id between pages".into(),
            ));
        }
        let Some(page) = &response.body.data else {
            return Err(runtime(format!(
                "Xiaohongshu search page {expected_page} has no results data ({})",
                response.body.msg.as_deref().unwrap_or("no message")
            )));
        };
        verified.push(SearchPage {
            has_more: page.has_more,
            items: page.items.clone(),
        });
        if !page.has_more {
            break;
        }
    }
    Ok(VerifiedPages { pages: verified })
}

/// Explains a read that ended before it set every filter or read every page it needed.
fn incomplete(
    data: &SearchData,
    expected: &Conditions,
    answered: usize,
    pages: usize,
) -> AttemptFailure {
    if let Some(reason) = &data.search_failure {
        return runtime(format!(
            "the Xiaohongshu page logic could not search from {} ({reason})",
            describe(&data.page)
        ));
    }
    let clicked = data.filter_clicks == expected.clicks;
    let on_expected_page = data.before_search || is_results_page(&data.page.url);
    let unfinished = unfinished_read(&data.page, data.timed_out, on_expected_page, || {
        if data.before_search {
            "the read deadline passed before the search was submitted on the Xiaohongshu home page"
                .to_owned()
        } else if clicked {
            format!(
                "Xiaohongshu returned {answered} of {pages} search pages before the read deadline"
            )
        } else {
            format!(
                "the read deadline passed after {} of {} Xiaohongshu filter clicks",
                data.filter_clicks, expected.clicks
            )
        }
    });
    if let Some(failure) = unfinished {
        return failure;
    }
    if !clicked {
        let reason = data
            .filter_failure
            .as_deref()
            .map(|failure| format!(" ({failure})"))
            .unwrap_or_default();
        return runtime(format!(
            "the Xiaohongshu page logic made {} filter clicks, not {}{reason}",
            data.filter_clicks, expected.clicks
        ));
    }
    runtime(format!(
        "the Xiaohongshu page logic read {answered} of {pages} search pages without timing out"
    ))
}

fn is_results_page(url: &str) -> bool {
    Url::parse(url).is_ok_and(|url| {
        url.host_str() == Some(SITE_HOST)
            && RESULTS_PATHS.contains(&url.path().trim_end_matches('/'))
    })
}

/// The decoded items and the diagnostics that say what they leave out.
struct Decoded {
    items: Vec<PlatformItem>,
    skipped: Option<String>,
    incomplete: Option<String>,
}

/// Decodes the note cards across pages, skips other cards and unusable notes, removes repeated
/// notes, and keeps at most `limit`. Only Xiaohongshu itself can say a search has no results.
fn decode(verified: &VerifiedPages, limit: u16, now: NaiveDateTime) -> Result<Decoded, String> {
    let first = &verified.pages[0];
    let notes = verified
        .pages
        .iter()
        .flat_map(|page| &page.items)
        .filter(|item| item["model_type"] == "note")
        .collect::<Vec<_>>();
    let first_has_notes = first.items.iter().any(|item| item["model_type"] == "note");
    if !first_has_notes {
        if first.has_more {
            return Err(
                "Xiaohongshu listed no notes on the first page but says more remain".into(),
            );
        }
        return Ok(Decoded {
            items: Vec::new(),
            skipped: None,
            incomplete: None,
        });
    }
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    let mut skipped = 0_usize;
    for note in &notes {
        let Some(item) = decode_note(note, now) else {
            skipped += 1;
            continue;
        };
        if seen.insert(item.url.clone()) {
            items.push(item);
        }
    }
    if items.is_empty() {
        return Err(format!(
            "Xiaohongshu listed {} notes, but none has a valid note ID and access token",
            notes.len()
        ));
    }
    let limit = usize::from(limit);
    let truncated = items.len() > limit;
    items.truncate(limit);
    let upstream_has_more = verified.pages.last().is_some_and(|page| page.has_more);
    Ok(Decoded {
        items,
        skipped: (skipped > 0).then(|| {
            format!("skipped {skipped} Xiaohongshu notes without a valid note ID or access token")
        }),
        incomplete: (upstream_has_more || truncated).then(|| {
            "Xiaohongshu has more results than this command returned; Xiaohongshu search cannot continue from a cursor, so raise --limit (up to 100) to read more in one command".to_owned()
        }),
    })
}

fn decode_note(note: &serde_json::Value, now: NaiveDateTime) -> Option<PlatformItem> {
    let reference = note["id"].as_str().and_then(XiaohongshuRef::from_note_id)?;
    let token = note["xsec_token"].as_str().and_then(AccessToken::new)?;
    let card = &note["note_card"];
    let text = |value: &serde_json::Value| value.as_str().map(str::to_owned);
    let published_text = card["corner_tag_info"].as_array().and_then(|tags| {
        tags.iter()
            .find(|tag| tag["type"] == "publish_time")
            .and_then(|tag| text(&tag["text"]))
    });
    let counts = &card["interact_info"];
    Some(PlatformItem {
        url: reference.canonical_url(),
        depth: ContentDepth::Metadata,
        title: Some(
            card["display_title"]
                .as_str()
                .map(str::trim)
                .unwrap_or_default()
                .to_owned(),
        ),
        authors: card["user"]["nickname"]
            .as_str()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(|name| vec![name.to_owned()])
            .unwrap_or_default(),
        published: published_text
            .as_deref()
            .and_then(|shown| published_date(shown, now)),
        data: PlatformItemData::Xiaohongshu(XiaohongshuItemData {
            note_type: card["type"].as_str().map(|kind| match kind {
                "normal" => "image".to_owned(),
                other => other.to_owned(),
            }),
            author_id: text(&card["user"]["user_id"]),
            likes: text(&counts["liked_count"]),
            collects: text(&counts["collected_count"]),
            comments: text(&counts["comment_count"]),
            shares: text(&counts["shared_count"]),
            published_text,
            access_url: reference.access_url(&token),
        }),
        reference: PlatformRef::Xiaohongshu(reference),
    })
}

/// The system clock as a time in the system local timezone, the reference for the dates a card
/// shows without a year or as a relative time.
fn local_now() -> NaiveDateTime {
    Local::now().naive_local()
}

/// Normalizes the publication date a note card shows to a calendar date, counted from `now` (the
/// request time in the system local timezone): `YYYY-MM-DD` as is; `MM-DD` in the year of `now`,
/// or the year before when that date lies in the future; and the relative forms `刚刚`,
/// `N分钟前`, `N小时前`, `N天前`, `昨天`, and `昨天 HH:MM`. Any other text has no date.
fn published_date(shown: &str, now: NaiveDateTime) -> Option<String> {
    let shown = shown.trim();
    let date = relative_date(shown, now).or_else(|| calendar_date(shown, now.date()))?;
    Some(date.format("%Y-%m-%d").to_string())
}

fn relative_date(shown: &str, now: NaiveDateTime) -> Option<NaiveDate> {
    let today = now.date();
    if shown == "刚刚" {
        return Some(today);
    }
    if let Some(rest) = shown.strip_prefix("昨天") {
        let valid = rest.is_empty()
            || rest.strip_prefix(' ').is_some_and(|time| {
                time.len() == "HH:MM".len() && NaiveTime::parse_from_str(time, "%H:%M").is_ok()
            });
        return valid.then(|| today.pred_opt()).flatten();
    }
    let count = |unit: &str| {
        shown
            .strip_suffix(unit)
            .filter(|count| !count.is_empty() && count.bytes().all(|byte| byte.is_ascii_digit()))
            .and_then(|count| count.parse::<i64>().ok())
    };
    if let Some(days) = count("天前") {
        return today.checked_sub_signed(TimeDelta::try_days(days)?);
    }
    let ago = match count("分钟前") {
        Some(minutes) => TimeDelta::try_minutes(minutes)?,
        None => TimeDelta::try_hours(count("小时前")?)?,
    };
    now.checked_sub_signed(ago).map(|then| then.date())
}

fn calendar_date(shown: &str, today: NaiveDate) -> Option<NaiveDate> {
    let digits = |part: &str, length: usize| {
        (part.len() == length && part.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| part.parse::<u32>().ok())
            .flatten()
    };
    match shown.split('-').collect::<Vec<_>>().as_slice() {
        [year, month, day] => NaiveDate::from_ymd_opt(
            i32::try_from(digits(year, 4)?).ok()?,
            digits(month, 2)?,
            digits(day, 2)?,
        ),
        [month, day] => {
            let (month, day) = (digits(month, 2)?, digits(day, 2)?);
            match NaiveDate::from_ymd_opt(today.year(), month, day) {
                Some(date) if date <= today => Some(date),
                _ => NaiveDate::from_ymd_opt(today.year() - 1, month, day),
            }
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "xiaohongshu_browser_search_tests.rs"]
mod tests;
