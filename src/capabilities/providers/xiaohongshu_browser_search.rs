//! Xiaohongshu search: page-fact classification, condition checks, and note-card decoding.

use std::collections::HashSet;

use chrono::{Datelike, FixedOffset, NaiveDate, Utc};
use serde::Deserialize;

use super::{PageFacts, ROUTE, XiaohongshuBrowser, classify_blocks, describe, runtime};
use crate::catalog::PlatformOperation;
use crate::net::{AttemptFailure, combine_diagnostics};
use crate::providers::execution::execute_anonymous;
use crate::providers::opencli::{self, EnvelopeStatus};
use crate::providers::shared::{other_platform_message, parameter_error};
use crate::types::{
    AccessToken, AttemptErrorKind, ContentDepth, PlatformItem, PlatformItemData, PlatformRef,
    PlatformSearchOptions, PlatformSearchOutcome, PlatformSearchRequest, ProviderError,
    XiaohongshuItemData, XiaohongshuNoteType, XiaohongshuPublishTime, XiaohongshuRef,
    XiaohongshuSearchOptions, XiaohongshuSort,
};

const PAGE_SIZE: u16 = 20;
const SITE_HOST: &str = "www.xiaohongshu.com";
const RESULTS_PATH: &str = "/search_result";
const SORT_FILTER: &str = "sort_type";
const NOTE_TYPE_FILTER: &str = "filter_note_type";
const TIME_FILTER: &str = "filter_note_time";
// The filter groups forager never sets; they depend on browsing history and location.
const UNSET_FILTERS: [&str; 2] = ["filter_note_range", "filter_pos_distance"];
const ANY: &str = "不限";

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
        | PlatformSearchOptions::Scholar(_) => {
            Err(other_platform_message(ROUTE, request.options.platform()))
        }
    }
}

impl XiaohongshuBrowser {
    /// Reads the results pages that `limit` needs in one OpenCLI command. Xiaohongshu ranks
    /// differently on every visit, so the route issues no cursor and reports in a diagnostic
    /// when results remain.
    pub(crate) async fn search(
        &self,
        request: &PlatformSearchRequest,
    ) -> Result<PlatformSearchOutcome, ProviderError> {
        search_support(request).map_err(parameter_error)?;
        let PlatformSearchOptions::Xiaohongshu(options) = &request.options else {
            unreachable!("support checked")
        };
        let query = request.query.trim();
        let expected = Conditions::of(options);
        let command = self.command(
            "search",
            vec![
                ("query", query.to_owned()),
                ("sort", sort_name(options.sort).to_owned()),
                ("note-type", note_type_name(options.note_type).to_owned()),
                (
                    "publish-time",
                    publish_time_name(options.publish_time).to_owned(),
                ),
                ("pages", request.limit.div_ceil(PAGE_SIZE).to_string()),
            ],
        );
        let command = &command;
        let expected = &expected;
        let pages = usize::from(request.limit.div_ceil(PAGE_SIZE));
        let execution = execute_anonymous(
            self.settings(PlatformOperation::Search),
            move |deadline| async move {
                let envelope = opencli::run::<SearchData>(command, &self.limiter, deadline).await?;
                if envelope.status != EnvelopeStatus::Ok {
                    return Err(runtime(
                        "the forager-xhs adapter reported no results instead of page facts".into(),
                    ));
                }
                let verified = read_search(&envelope.data, query, expected, pages)?;
                let decoded = decode(&verified, request.limit, beijing_today()).map_err(runtime)?;
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

fn sort_name(sort: XiaohongshuSort) -> &'static str {
    match sort {
        XiaohongshuSort::Comprehensive => "comprehensive",
        XiaohongshuSort::Latest => "latest",
        XiaohongshuSort::MostLiked => "most-liked",
        XiaohongshuSort::MostCommented => "most-commented",
        XiaohongshuSort::MostCollected => "most-collected",
    }
}

fn note_type_name(note_type: XiaohongshuNoteType) -> &'static str {
    match note_type {
        XiaohongshuNoteType::All => "all",
        XiaohongshuNoteType::Image => "image",
        XiaohongshuNoteType::Video => "video",
    }
}

fn publish_time_name(time: XiaohongshuPublishTime) -> &'static str {
    match time {
        XiaohongshuPublishTime::Any => "any",
        XiaohongshuPublishTime::Day => "day",
        XiaohongshuPublishTime::Week => "week",
        XiaohongshuPublishTime::HalfYear => "half-year",
    }
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

/// The facts the adapter reports for one search command.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SearchData {
    page: PageFacts,
    /// How many filter clicks the adapter made.
    filter_clicks: u32,
    /// Why the adapter could not make the next filter click.
    filter_failure: Option<String>,
    /// The read deadline passed before the expected responses arrived.
    timed_out: bool,
    /// A matching response never got its body before the read deadline.
    body_missing: bool,
    /// Every captured `search/notes` exchange, in order.
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
    let facts = &data.page;
    let clicked = data.filter_clicks == expected.clicks;
    let on_results = is_results_page(&facts.url);
    if data.timed_out && on_results {
        let message = if clicked {
            format!(
                "Xiaohongshu returned {answered} of {pages} search pages before the read deadline"
            )
        } else {
            format!(
                "the read deadline passed after {} of {} Xiaohongshu filter clicks",
                data.filter_clicks, expected.clicks
            )
        };
        return AttemptFailure {
            kind: AttemptErrorKind::Timeout,
            status: None,
            message,
        };
    }
    if !on_results {
        return runtime(format!(
            "the forager-xhs adapter stopped at an unexpected page: {}",
            describe(facts)
        ));
    }
    if !clicked {
        let reason = data
            .filter_failure
            .as_deref()
            .map(|failure| format!(" ({failure})"))
            .unwrap_or_default();
        return runtime(format!(
            "the forager-xhs adapter made {} filter clicks, not {}{reason}",
            data.filter_clicks, expected.clicks
        ));
    }
    runtime(format!(
        "the forager-xhs adapter returned {answered} of {pages} search pages without timing out"
    ))
}

fn is_results_page(url: &str) -> bool {
    reqwest::Url::parse(url).is_ok_and(|url| {
        url.host_str() == Some(SITE_HOST) && url.path().trim_end_matches('/') == RESULTS_PATH
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
fn decode(verified: &VerifiedPages, limit: u16, today: NaiveDate) -> Result<Decoded, String> {
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
        let Some(item) = decode_note(note, today) else {
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

fn decode_note(note: &serde_json::Value, today: NaiveDate) -> Option<PlatformItem> {
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
        title: card["display_title"]
            .as_str()
            .map(str::trim)
            .unwrap_or_default()
            .to_owned(),
        authors: card["user"]["nickname"]
            .as_str()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(|name| vec![name.to_owned()])
            .unwrap_or_default(),
        published: published_text
            .as_deref()
            .and_then(|shown| published_date(shown, today)),
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

fn beijing_today() -> NaiveDate {
    let beijing = FixedOffset::east_opt(8 * 3600).expect("UTC+8 is a valid offset");
    Utc::now().with_timezone(&beijing).date_naive()
}

/// Normalizes the publication date a note card shows: `YYYY-MM-DD` as is, and `MM-DD` in the
/// year of `today` (Beijing time), or the year before when that date lies in the future. Any
/// other text, such as a relative time, has no date.
fn published_date(shown: &str, today: NaiveDate) -> Option<String> {
    let shown = shown.trim();
    let digits = |part: &str, length: usize| {
        (part.len() == length && part.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| part.parse::<u32>().ok())
            .flatten()
    };
    let parts = shown.split('-').collect::<Vec<_>>();
    let date = match parts.as_slice() {
        [year, month, day] => NaiveDate::from_ymd_opt(
            i32::try_from(digits(year, 4)?).ok()?,
            digits(month, 2)?,
            digits(day, 2)?,
        )?,
        [month, day] => {
            let (month, day) = (digits(month, 2)?, digits(day, 2)?);
            let this_year = NaiveDate::from_ymd_opt(today.year(), month, day);
            match this_year {
                Some(date) if date <= today => date,
                _ => NaiveDate::from_ymd_opt(today.year() - 1, month, day)?,
            }
        }
        _ => return None,
    };
    Some(date.format("%Y-%m-%d").to_string())
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::published_date;

    #[test]
    fn a_month_and_day_take_the_latest_year_that_is_not_in_the_future() {
        let today = NaiveDate::from_ymd_opt(2026, 3, 1).expect("date");

        let dates = [
            "07-09",
            "03-01",
            "02-28",
            "2025-06-13",
            "02-29",
            "3天前",
            "2025-6-13",
        ]
        .map(|shown| published_date(shown, today));

        assert_eq!(
            dates,
            [
                Some("2025-07-09".to_owned()),
                Some("2026-03-01".to_owned()),
                Some("2026-02-28".to_owned()),
                Some("2025-06-13".to_owned()),
                None,
                None,
                None,
            ]
        );
    }
}
