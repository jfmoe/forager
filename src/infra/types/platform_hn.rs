//! Hacker News shapes: item refs, search options, item fields, and the `stories` operation's
//! request and listing.

use std::fmt;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use super::attempt::ProviderAttempt;
use super::platform::{Platform, PlatformItem, PlatformRef, PlatformRefError};

const HN_MAX_LIMIT: u16 = 100;
/// The attempt-target name of the Hacker News stories operation.
pub(crate) const STORIES: &str = "stories";
const REF_PREFIX: &str = "hn:";
const ITEM_HOST: &str = "news.ycombinator.com";
const MIN_USERNAME_LENGTH: usize = 2;
const MAX_USERNAME_LENGTH: usize = 15;

const REF_HINT: &str = "pass an `hn:<id>` ref or a Hacker News item URL such as https://news.ycombinator.com/item?id=<id>";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// A Hacker News item identity: its decimal item ID. Stories, comments, polls, poll options,
/// and jobs share one ID space, and items have no versions.
pub struct HnRef {
    id: u64,
}

impl HnRef {
    /// Parses `hn:<id>` or a news.ycombinator.com item URL.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformRefError`] for any other input, including user and site pages and
    /// IDs that overflow a `u64`.
    pub fn parse(input: &str) -> Result<Self, PlatformRefError> {
        let trimmed = input.trim();
        let parsed = match trimmed.get(..REF_PREFIX.len()) {
            Some(prefix) if prefix.eq_ignore_ascii_case(REF_PREFIX) => {
                Self::from_id(&trimmed[REF_PREFIX.len()..])
            }
            _ => parse_url(trimmed),
        };
        parsed.ok_or_else(|| PlatformRefError {
            platform: Platform::Hn,
            input: Some(input.to_owned()),
            hint: REF_HINT,
        })
    }

    /// Returns the ref of a decimal item ID as Hacker News prints it: no sign, no leading zero.
    #[must_use]
    pub fn from_id(value: &str) -> Option<Self> {
        let canonical = !value.starts_with('0') && value.bytes().all(|byte| byte.is_ascii_digit());
        canonical
            .then(|| value.parse().ok())
            .flatten()
            .and_then(Self::new)
    }

    /// Returns the ref of an item ID; `None` for 0, which no item has.
    #[must_use]
    pub const fn new(id: u64) -> Option<Self> {
        if id == 0 { None } else { Some(Self { id }) }
    }

    /// Returns the item ID.
    #[must_use]
    pub const fn id(self) -> u64 {
        self.id
    }

    /// Returns the item page.
    #[must_use]
    pub fn canonical_url(self) -> String {
        format!("https://{ITEM_HOST}/item?id={}", self.id)
    }
}

impl fmt::Display for HnRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.id)
    }
}

/// Parses `http(s)://news.ycombinator.com/item?id=<id>`, ignoring other query parameters, the
/// fragment, and a trailing `/`.
fn parse_url(input: &str) -> Option<HnRef> {
    let scheme_end = input.find("://")?;
    if !matches!(
        input[..scheme_end].to_ascii_lowercase().as_str(),
        "http" | "https"
    ) {
        return None;
    }
    let rest = &input[scheme_end + 3..];
    let rest = rest.split('#').next().unwrap_or_default();
    let (location, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (host, path) = location.split_once('/').unwrap_or((location, ""));
    if !host.eq_ignore_ascii_case(ITEM_HOST) || path.trim_end_matches('/') != "item" {
        return None;
    }
    let id = query.split('&').find_map(|pair| pair.strip_prefix("id="))?;
    HnRef::from_id(id)
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// The order of Hacker News search results.
pub enum HnSort {
    /// Best match first.
    #[default]
    Relevance,
    /// Newest first.
    Date,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// An item type a search can be limited to; several types match any of them.
pub enum HnSearchType {
    Story,
    Comment,
    AskHn,
    ShowHn,
    LaunchHn,
    Job,
    Poll,
}

impl HnSearchType {
    /// Returns the stable identifier, which is also the search tag.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Story => "story",
            Self::Comment => "comment",
            Self::AskHn => "ask_hn",
            Self::ShowHn => "show_hn",
            Self::LaunchHn => "launch_hn",
            Self::Job => "job",
            Self::Poll => "poll",
        }
    }
}

/// Optional Hacker News search filters and order; every filter is enforced by the search
/// service. A cursor encodes them completely.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct HnSearchOptions {
    pub sort: HnSort,
    /// Item types; any of them matches.
    pub types: Vec<HnSearchType>,
    /// Only items by this user.
    pub author: Option<String>,
    /// Only items in the discussion of this story.
    pub story: Option<u64>,
    /// The first included UTC publication day.
    pub from: Option<NaiveDate>,
    /// The last included UTC publication day.
    pub to: Option<NaiveDate>,
    pub min_points: Option<u64>,
    pub min_comments: Option<u64>,
}

impl HnSearchOptions {
    /// Returns the options with repeated types dropped, keeping the first of each.
    #[must_use]
    pub fn normalized(mut self) -> Self {
        let mut types = Vec::new();
        for item_type in self.types {
            if !types.contains(&item_type) {
                types.push(item_type);
            }
        }
        self.types = types;
        self
    }

    pub(super) fn validate(&self, query: &str, limit: u16) -> Result<(), String> {
        if !(1..=HN_MAX_LIMIT).contains(&limit) {
            return Err(format!("--limit must be between 1 and {HN_MAX_LIMIT}"));
        }
        if query.trim().is_empty() {
            return Err("hn search needs a query".into());
        }
        if let Some(author) = &self.author
            && !is_username(author)
        {
            return Err(format!(
                "invalid Hacker News username `{author}`; a username is {MIN_USERNAME_LENGTH} to {MAX_USERNAME_LENGTH} letters, digits, `_`, or `-`"
            ));
        }
        if self.story == Some(0) {
            return Err("--story must be a Hacker News item ID".into());
        }
        if let (Some(from), Some(to)) = (self.from, self.to)
            && from > to
        {
            return Err("--from must not be later than --to".into());
        }
        // Comments have no points, so a points floor would match none of them.
        if self.min_points.is_some() && self.types == [HnSearchType::Comment] {
            return Err("--min-points cannot be combined with --type comment alone; comments have no points".into());
        }
        Ok(())
    }
}

fn is_username(value: &str) -> bool {
    (MIN_USERNAME_LENGTH..=MAX_USERNAME_LENGTH).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// The type of a Hacker News item.
pub enum HnItemType {
    Story,
    Comment,
    Poll,
    Pollopt,
    Job,
}

impl HnItemType {
    /// Parses the type name both Hacker News APIs use.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "story" => Some(Self::Story),
            "comment" => Some(Self::Comment),
            "poll" => Some(Self::Poll),
            "pollopt" => Some(Self::Pollopt),
            "job" => Some(Self::Job),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// How the comments of a delivered discussion are ordered.
pub enum HnThreadOrder {
    /// Top-level comments in the rank Hacker News shows; replies by time.
    RankedTopLevel,
    /// Every comment by time, because the rank was unavailable.
    Chronological,
}

#[derive(Clone, Debug, Serialize)]
/// The Hacker News fields of an item. Search and stories counts may lag the site by minutes.
pub struct HnItemData {
    pub item_type: HnItemType,
    /// The URL the item links to, if any.
    pub link_url: Option<String>,
    pub points: Option<u64>,
    /// The number of comments in the item's discussion.
    pub comments: Option<u64>,
    /// The item's own body as Markdown, without its comments or linked page.
    pub text: Option<String>,
    /// The story a comment belongs to; `null` for other types.
    pub story_ref: Option<PlatformRef>,
    pub story_title: Option<String>,
    /// The item a comment replies to; `null` for other types.
    pub parent_ref: Option<PlatformRef>,
    /// How many comments the discussion file holds, at `thread` depth.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comments_delivered: Option<usize>,
    /// How the discussion file orders its comments, at `thread` depth.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<HnThreadOrder>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// One official Hacker News story list.
pub enum HnStoriesList {
    /// The front page.
    Top,
    New,
    Best,
    Ask,
    Show,
    Job,
}

impl HnStoriesList {
    /// Returns the stable list identifier used by commands and output.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::New => "new",
            Self::Best => "best",
            Self::Ask => "ask",
            Self::Show => "show",
            Self::Job => "job",
        }
    }
}

/// A request for the current items of one story list.
#[derive(Clone, Copy, Debug)]
pub(crate) struct HnStoriesRequest {
    pub(crate) list: HnStoriesList,
    /// The most items to list, 1 to 100.
    pub(crate) limit: u16,
}

#[derive(Clone, Debug, Serialize)]
/// One listed item with its 1-based position in the list.
pub struct HnRankedItem {
    #[serde(flatten)]
    pub item: PlatformItem,
    pub rank: usize,
}

#[derive(Clone, Debug, Serialize)]
/// The current items of one story list, in list order. No cursor exists: the list changes
/// constantly, so a later page could not continue this one.
pub struct HnStoriesResult {
    pub platform: Platform,
    /// The route that read the list.
    pub provider: &'static str,
    pub list: HnStoriesList,
    pub items: Vec<HnRankedItem>,
    #[serde(rename = "provider_attempts", skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<ProviderAttempt>,
    #[serde(skip)]
    pub diagnostic: Option<String>,
}

/// One route's listing before the chain attaches the route and the planning attempts.
pub(crate) struct HnStoriesOutcome {
    pub(crate) items: Vec<HnRankedItem>,
    pub(crate) attempts: Vec<ProviderAttempt>,
    pub(crate) diagnostic: Option<String>,
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use crate::types::{Platform, PlatformRef, PlatformSearchOptions, PlatformSearchRequest};

    use super::{HnSearchOptions, HnSearchType};

    fn hn(input: &str) -> Option<String> {
        PlatformRef::parse(Platform::Hn, input)
            .ok()
            .map(|reference| reference.to_string())
    }

    #[test]
    fn every_supported_form_parses_to_the_same_ref() {
        let parsed = [
            "hn:8863",
            "HN:8863",
            " hn:8863 ",
            "https://news.ycombinator.com/item?id=8863",
            "http://news.ycombinator.com/item?id=8863",
            "HTTPS://News.YCombinator.com/item?id=8863",
            "https://news.ycombinator.com/item/?id=8863",
            "https://news.ycombinator.com/item?id=8863&p=2",
            "https://news.ycombinator.com/item?p=2&id=8863",
            "https://news.ycombinator.com/item?id=8863#9224",
        ]
        .map(hn);

        assert_eq!(parsed, [(); 10].map(|()| Some("hn:8863".to_owned())));
    }

    #[test]
    fn user_and_site_pages_other_hosts_and_bad_ids_are_rejected() {
        let parsed = [
            "https://news.ycombinator.com/user?id=dhouston",
            "https://news.ycombinator.com/from?site=example.com",
            "https://news.ycombinator.com/news",
            "https://news.ycombinator.com/item",
            "https://news.ycombinator.com/item?id=",
            "https://news.ycombinator.com/item?id=08863",
            "https://news.ycombinator.com/item?id=abc",
            "https://ycombinator.com/item?id=8863",
            "https://news.ycombinator.com.evil.example/item?id=8863",
            "https://hn.algolia.com/api/v1/items/8863",
            "ftp://news.ycombinator.com/item?id=8863",
            "8863",
            "hn:",
            "hn:0",
            "hn:+1",
            "hn:18446744073709551616",
        ]
        .map(hn);

        assert_eq!(parsed, [const { None }; 16]);
    }

    #[test]
    fn unrecognized_input_names_the_accepted_forms() {
        let error = PlatformRef::parse(Platform::Hn, "https://news.ycombinator.com/user?id=pg")
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "unrecognized hn reference `https://news.ycombinator.com/user?id=pg`; pass an `hn:<id>` ref or a Hacker News item URL such as https://news.ycombinator.com/item?id=<id>"
        );
    }

    #[test]
    fn canonical_urls_round_trip_to_the_ref() {
        let reference = PlatformRef::parse(Platform::Hn, "hn:8863").expect("ref");

        let round_trip = PlatformRef::parse(Platform::Hn, &reference.canonical_url());

        assert_eq!(
            (reference.canonical_url(), reference.kind(), round_trip.ok()),
            (
                "https://news.ycombinator.com/item?id=8863".to_owned(),
                "item",
                Some(reference)
            )
        );
    }

    fn validate(options: HnSearchOptions) -> Result<(), String> {
        PlatformSearchRequest {
            query: "rust".into(),
            limit: 20,
            options: PlatformSearchOptions::Hn(options),
            page: None,
        }
        .validate()
    }

    #[test]
    fn cross_field_rules_reject_contradictory_options() {
        let day = |value: &str| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok();
        let results = [
            HnSearchOptions {
                from: day("2024-02-01"),
                to: day("2024-01-01"),
                ..HnSearchOptions::default()
            },
            HnSearchOptions {
                types: vec![HnSearchType::Comment],
                min_points: Some(10),
                ..HnSearchOptions::default()
            },
            HnSearchOptions {
                author: Some("a".into()),
                ..HnSearchOptions::default()
            },
            HnSearchOptions {
                author: Some("bad name".into()),
                ..HnSearchOptions::default()
            },
        ]
        .map(|options| validate(options).err());

        assert_eq!(
            results,
            [
                Some("--from must not be later than --to".into()),
                Some("--min-points cannot be combined with --type comment alone; comments have no points".into()),
                Some("invalid Hacker News username `a`; a username is 2 to 15 letters, digits, `_`, or `-`".into()),
                Some("invalid Hacker News username `bad name`; a username is 2 to 15 letters, digits, `_`, or `-`".into()),
            ]
        );
    }

    #[test]
    fn compatible_options_pass_validation() {
        let results = [
            HnSearchOptions {
                types: vec![HnSearchType::Comment, HnSearchType::Story],
                min_points: Some(10),
                author: Some("dang".into()),
                ..HnSearchOptions::default()
            },
            HnSearchOptions {
                types: vec![HnSearchType::Comment],
                min_comments: Some(1),
                author: Some("a-b_c".into()),
                ..HnSearchOptions::default()
            },
        ]
        .map(validate);

        assert_eq!(results, [Ok(()), Ok(())]);
    }

    #[test]
    fn an_empty_query_or_an_out_of_range_limit_is_rejected() {
        let request = |query: &str, limit| PlatformSearchRequest {
            query: query.into(),
            limit,
            options: PlatformSearchOptions::Hn(HnSearchOptions::default()),
            page: None,
        };

        let results = [request(" ", 20), request("rust", 0), request("rust", 101)]
            .map(|request| request.validate().err());

        assert_eq!(
            results,
            [
                Some("hn search needs a query".into()),
                Some("--limit must be between 1 and 100".into()),
                Some("--limit must be between 1 and 100".into()),
            ]
        );
    }

    #[test]
    fn repeated_types_keep_their_first_position() {
        let options = HnSearchOptions {
            types: vec![
                HnSearchType::ShowHn,
                HnSearchType::Comment,
                HnSearchType::ShowHn,
            ],
            ..HnSearchOptions::default()
        }
        .normalized();

        assert_eq!(options.types, [HnSearchType::ShowHn, HnSearchType::Comment]);
    }
}
