//! X shapes: post refs and their snowflake time, search options, post fields, and the `ask`
//! operation's request and answer.

use std::fmt;

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use super::attempt::ProviderAttempt;
use super::platform::{ASK, Platform, PlatformRef, PlatformRefError};

const X_MAX_LIMIT: u16 = 20;
const MAX_HANDLES: usize = 20;
const MAX_HANDLE_LENGTH: usize = 15;
const REF_PREFIX: &str = "x:";
const POST_HOSTS: [&str; 6] = [
    "x.com",
    "www.x.com",
    "mobile.x.com",
    "twitter.com",
    "www.twitter.com",
    "mobile.twitter.com",
];
// X's snowflake epoch, 2010-11-04T01:42:54.657Z, in Unix milliseconds.
const SNOWFLAKE_EPOCH_MS: i64 = 1_288_834_974_657;
// The sequential post IDs issued before snowflakes stayed below 3·10^10; a snowflake that low
// would date to the first seconds after its epoch, before X issued any.
const FIRST_SNOWFLAKE_ID: u64 = 30_000_000_000;

const REF_HINT: &str = "pass an `x:<post_id>` ref or an x.com or twitter.com post URL such as https://x.com/<handle>/status/<id>";
const SHORT_LINK_HINT: &str = "t.co short links need a network request to expand: open the link in a browser and pass the x.com post URL it leads to";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// An X post identity: its decimal post ID. Posts have no versions.
pub struct XRef {
    post_id: u64,
}

impl XRef {
    /// Parses `x:<post_id>` or an x.com or twitter.com post URL.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformRefError`] for any other input, including t.co short links, profile
    /// pages, and post IDs that overflow a `u64`.
    pub fn parse(input: &str) -> Result<Self, PlatformRefError> {
        let trimmed = input.trim();
        let parsed = match trimmed.get(..REF_PREFIX.len()) {
            Some(prefix) if prefix.eq_ignore_ascii_case(REF_PREFIX) => {
                Self::from_post_id(&trimmed[REF_PREFIX.len()..]).ok_or(REF_HINT)
            }
            _ => parse_url(trimmed),
        };
        parsed.map_err(|hint| PlatformRefError {
            platform: Platform::X,
            input: Some(input.to_owned()),
            hint,
        })
    }

    /// Returns the post a status URL names, or `None` for any other URL.
    pub(crate) fn from_status_url(url: &str) -> Option<Self> {
        parse_url(url.trim()).ok()
    }

    /// Returns the ref of a decimal post ID as X prints it.
    #[must_use]
    pub fn from_post_id(value: &str) -> Option<Self> {
        let canonical = !value.starts_with('0') && value.bytes().all(|byte| byte.is_ascii_digit());
        canonical
            .then(|| value.parse().ok())
            .flatten()
            .map(|post_id| Self { post_id })
    }

    /// Returns the post ID.
    #[must_use]
    pub const fn post_id(self) -> u64 {
        self.post_id
    }

    /// Returns the post page, which opens whatever the author's handle is now.
    #[must_use]
    pub fn canonical_url(self) -> String {
        format!("https://x.com/i/status/{}", self.post_id)
    }

    /// Returns when the post was published, as a UTC ISO 8601 timestamp read from its snowflake
    /// ID, or `None` for a post older than snowflake IDs.
    #[must_use]
    pub fn published(self) -> Option<String> {
        self.published_at()
            .map(|time| time.format("%Y-%m-%dT%H:%M:%SZ").to_string())
    }

    /// Returns the UTC calendar day the post was published, when its ID dates it.
    pub(crate) fn published_on(self) -> Option<NaiveDate> {
        self.published_at().map(|time| time.date_naive())
    }

    fn published_at(self) -> Option<DateTime<Utc>> {
        if self.post_id < FIRST_SNOWFLAKE_ID {
            return None;
        }
        let milliseconds = i64::try_from(self.post_id >> 22).ok()? + SNOWFLAKE_EPOCH_MS;
        DateTime::from_timestamp_millis(milliseconds)
    }
}

impl fmt::Display for XRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.post_id)
    }
}

/// Parses an x.com or twitter.com post URL: `/<handle>/status/<id>` (optionally followed by
/// `/photo/<n>`), `/i/status/<id>`, or `/i/web/status/<id>`, ignoring the query, the fragment,
/// and a trailing `/`.
fn parse_url(input: &str) -> Result<XRef, &'static str> {
    let rest = input
        .strip_prefix("https://")
        .or_else(|| input.strip_prefix("http://"))
        .ok_or(REF_HINT)?;
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    if host.eq_ignore_ascii_case("t.co") {
        return Err(SHORT_LINK_HINT);
    }
    if !POST_HOSTS
        .iter()
        .any(|known| host.eq_ignore_ascii_case(known))
    {
        return Err(REF_HINT);
    }
    let path = path.split(['?', '#']).next().unwrap_or_default();
    let segments = path.trim_end_matches('/').split('/').collect::<Vec<_>>();
    let id = match segments.as_slice() {
        ["i", "status", id] | ["i", "web", "status", id] => id,
        [handle, "status", id] | [handle, "status", id, "photo", _] if is_handle(handle) => id,
        _ => return Err(REF_HINT),
    };
    if let ["photo", index] = segments.get(3..).unwrap_or_default()
        && XRef::from_post_id(index).is_none()
    {
        return Err(REF_HINT);
    }
    XRef::from_post_id(id).ok_or(REF_HINT)
}

fn is_handle(value: &str) -> bool {
    !value.is_empty()
        && value != "i"
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// The filters X search and ask send with the `x_search` tool; xAI enforces each of them on the
/// server. At most one of the handle lists is non-empty.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct XSearchOptions {
    /// Only posts by these handles, without `@`.
    pub handles: Vec<String>,
    /// No posts by these handles, without `@`.
    pub excluded_handles: Vec<String>,
    /// The first included UTC publication day.
    pub from: Option<NaiveDate>,
    /// The last included UTC publication day.
    pub to: Option<NaiveDate>,
}

impl XSearchOptions {
    /// Returns options whose handles drop a leading `@` and repeats, case-insensitively, in the
    /// order given.
    #[must_use]
    pub fn new(
        handles: Vec<String>,
        excluded_handles: Vec<String>,
        from: Option<NaiveDate>,
        to: Option<NaiveDate>,
    ) -> Self {
        Self {
            handles: normalize_handles(handles),
            excluded_handles: normalize_handles(excluded_handles),
            from,
            to,
        }
    }

    pub(super) fn validate(&self, query: &str, limit: u16) -> Result<(), String> {
        if !(1..=X_MAX_LIMIT).contains(&limit) {
            return Err(format!("--limit must be between 1 and {X_MAX_LIMIT}"));
        }
        validate_query(query, "search")?;
        self.validate_filters()
    }

    fn validate_filters(&self) -> Result<(), String> {
        if !self.handles.is_empty() && !self.excluded_handles.is_empty() {
            return Err("--handle cannot be combined with --exclude-handle".into());
        }
        for (flag, handles) in [
            ("--handle", &self.handles),
            ("--exclude-handle", &self.excluded_handles),
        ] {
            if handles.len() > MAX_HANDLES {
                return Err(format!("{flag} accepts at most {MAX_HANDLES} handles"));
            }
            if let Some(handle) = handles
                .iter()
                .find(|handle| handle.len() > MAX_HANDLE_LENGTH || !is_handle(handle))
            {
                return Err(format!(
                    "invalid X handle `{handle}`; a handle is 1 to {MAX_HANDLE_LENGTH} letters, digits, or underscores"
                ));
            }
        }
        if let (Some(from), Some(to)) = (self.from, self.to)
            && from > to
        {
            return Err("--from must not be later than --to".into());
        }
        Ok(())
    }
}

fn normalize_handles(handles: Vec<String>) -> Vec<String> {
    let mut normalized: Vec<String> = Vec::new();
    for handle in handles {
        let handle = handle.trim();
        let handle = handle.strip_prefix('@').unwrap_or(handle);
        if !normalized
            .iter()
            .any(|known| known.eq_ignore_ascii_case(handle))
        {
            normalized.push(handle.to_owned());
        }
    }
    normalized
}

fn validate_query(query: &str, operation: &str) -> Result<(), String> {
    if query.trim().is_empty() {
        return Err(format!("x {operation} needs a query"));
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize)]
/// The X fields of a post. The text and handle are relayed by the model that read the post, so
/// they may differ from the original.
pub struct XPostData {
    pub author_handle: String,
    pub text: String,
    /// The post's conversation in the order the model gave it, at `thread` depth.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread: Option<Vec<XThreadPost>>,
}

#[derive(Clone, Debug, Serialize)]
/// One post of a fetched conversation.
pub struct XThreadPost {
    #[serde(rename = "ref")]
    pub reference: PlatformRef,
    pub url: String,
    pub author_handle: String,
    pub published: Option<String>,
    pub text: String,
    pub role: XPostRole,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// Where a post sits in the conversation of the fetched post.
pub enum XPostRole {
    /// A post the fetched post replies to.
    Parent,
    /// The fetched post itself.
    Root,
    /// A reply to the fetched post.
    Reply,
    /// A post the fetched post quotes.
    Quoted,
}

impl XPostRole {
    /// Returns the stable role identifier used in output.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Parent => "parent",
            Self::Root => "root",
            Self::Reply => "reply",
            Self::Quoted => "quoted",
        }
    }
}

/// A request to answer a question from X posts alone.
#[derive(Clone, Debug)]
pub(crate) struct XAskRequest {
    pub(crate) query: String,
    pub(crate) options: XSearchOptions,
}

impl XAskRequest {
    /// Checks the rules that argument parsing cannot express.
    pub(crate) fn validate(&self) -> Result<(), String> {
        validate_query(&self.query, ASK)?;
        self.options.validate_filters()
    }
}

#[derive(Clone, Debug, Serialize)]
/// An answer written from X posts, with the sources it cites. The whole answer is relayed
/// content.
pub struct XAskResult {
    pub platform: Platform,
    /// The route that answered.
    pub provider: &'static str,
    pub query: String,
    pub answer: String,
    pub sources: Vec<XSource>,
    #[serde(rename = "provider_attempts", skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<ProviderAttempt>,
    #[serde(skip)]
    pub diagnostic: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
/// One source an answer cites: a post with its ref, or another page such as a profile.
pub struct XSource {
    #[serde(rename = "ref")]
    pub reference: Option<PlatformRef>,
    pub url: String,
    pub title: String,
}

/// One route's answer before the chain attaches the route and the planning attempts.
pub(crate) struct XAskOutcome {
    pub(crate) answer: String,
    pub(crate) sources: Vec<XSource>,
    pub(crate) attempts: Vec<ProviderAttempt>,
    pub(crate) diagnostic: Option<String>,
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use crate::types::{Platform, PlatformRef, PlatformSearchOptions, PlatformSearchRequest};

    use super::{XAskRequest, XRef, XSearchOptions};

    fn x(input: &str) -> Option<String> {
        PlatformRef::parse(Platform::X, input)
            .ok()
            .map(|reference| reference.to_string())
    }

    #[test]
    fn every_supported_form_parses_to_the_same_ref() {
        let parsed = [
            "x:2110126570233094527",
            "X:2110126570233094527",
            " x:2110126570233094527 ",
            "https://x.com/SpaceX/status/2110126570233094527",
            "http://x.com/SpaceX/status/2110126570233094527",
            "https://X.com/SpaceX/status/2110126570233094527/",
            "https://www.x.com/SpaceX/status/2110126570233094527",
            "https://mobile.x.com/SpaceX/status/2110126570233094527",
            "https://twitter.com/SpaceX/status/2110126570233094527",
            "https://www.twitter.com/SpaceX/status/2110126570233094527",
            "https://mobile.twitter.com/SpaceX/status/2110126570233094527",
            "https://x.com/i/status/2110126570233094527",
            "https://x.com/i/web/status/2110126570233094527",
            "https://x.com/SpaceX/status/2110126570233094527/photo/1",
            "https://x.com/SpaceX/status/2110126570233094527?s=20&t=abc",
            "https://x.com/SpaceX/status/2110126570233094527#reply",
        ]
        .map(x);

        assert_eq!(
            parsed,
            [(); 16].map(|()| Some("x:2110126570233094527".to_owned()))
        );
    }

    #[test]
    fn profiles_other_paths_lookalike_hosts_and_bad_ids_are_rejected() {
        let parsed = [
            "https://x.com/SpaceX",
            "https://x.com/SpaceX/status/",
            "https://x.com/SpaceX/status/2110126570233094527/likes",
            "https://x.com/SpaceX/status/2110126570233094527/photo/x",
            "https://x.com/i/user/1112661938608078848",
            "https://x.com/i/status/0123",
            "https://x.com/Space-X/status/2110126570233094527",
            "https://x.com.evil.example/SpaceX/status/2110126570233094527",
            "https://nitter.net/SpaceX/status/2110126570233094527",
            "ftp://x.com/SpaceX/status/2110126570233094527",
            "2110126570233094527",
            "x:",
            "x:18446744073709551616",
            "x:+1",
        ]
        .map(x);

        assert_eq!(parsed, [const { None }; 14]);
    }

    #[test]
    fn short_links_explain_how_to_expand_them() {
        let error = PlatformRef::parse(Platform::X, "https://t.co/AbCdEf").unwrap_err();

        assert!(
            error
                .to_string()
                .contains("t.co short links need a network request"),
            "{error}"
        );
    }

    #[test]
    fn canonical_urls_round_trip_to_the_ref() {
        let reference = PlatformRef::parse(Platform::X, "x:2110126570233094527").expect("ref");

        let round_trip = PlatformRef::parse(Platform::X, &reference.canonical_url());

        assert_eq!(
            (reference.canonical_url(), reference.kind(), round_trip.ok()),
            (
                "https://x.com/i/status/2110126570233094527".to_owned(),
                "post",
                Some(reference)
            )
        );
    }

    #[test]
    fn a_snowflake_id_dates_its_post_to_the_second() {
        // X showed these posts as published at 21:52:33 and 18:27:34 GMT.
        let published = ["2110126570233094527", "2106088717547077910"]
            .map(|id| XRef::from_post_id(id).and_then(XRef::published));

        assert_eq!(
            published,
            [
                Some("2026-10-13T21:52:33Z".to_owned()),
                Some("2026-10-02T18:27:34Z".to_owned())
            ]
        );
    }

    #[test]
    fn a_sequential_id_from_before_snowflakes_has_no_date() {
        let published =
            ["20", "29700859247"].map(|id| XRef::from_post_id(id).and_then(XRef::published));

        assert_eq!(published, [None, None]);
    }

    fn request(options: XSearchOptions) -> PlatformSearchRequest {
        PlatformSearchRequest {
            query: "starship".into(),
            limit: 10,
            options: PlatformSearchOptions::X(options),
            page: None,
        }
    }

    fn handles(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn handles_drop_the_at_sign_and_repeats_in_order() {
        let options = XSearchOptions::new(
            handles(&["@SpaceX", "elonmusk", "spacex", " @NASA "]),
            Vec::new(),
            None,
            None,
        );

        assert_eq!(options.handles, ["SpaceX", "elonmusk", "NASA"]);
    }

    #[test]
    fn handle_lists_are_exclusive_bounded_and_well_formed() {
        let too_many = (0..21).map(|index| format!("user{index}")).collect();
        let results = [
            XSearchOptions::new(handles(&["a"]), handles(&["b"]), None, None),
            XSearchOptions::new(too_many, Vec::new(), None, None),
            XSearchOptions::new(Vec::new(), handles(&["space-x"]), None, None),
            XSearchOptions::new(handles(&["sixteen_chars_xx"]), Vec::new(), None, None),
            XSearchOptions::new(handles(&["@"]), Vec::new(), None, None),
            XSearchOptions::new(handles(&["fifteen_chars_x"]), Vec::new(), None, None),
        ]
        .map(|options| request(options).validate());

        assert_eq!(
            results,
            [
                Err("--handle cannot be combined with --exclude-handle".to_owned()),
                Err("--handle accepts at most 20 handles".to_owned()),
                Err("invalid X handle `space-x`; a handle is 1 to 15 letters, digits, or underscores".to_owned()),
                Err("invalid X handle `sixteen_chars_xx`; a handle is 1 to 15 letters, digits, or underscores".to_owned()),
                Err("invalid X handle ``; a handle is 1 to 15 letters, digits, or underscores".to_owned()),
                Ok(()),
            ]
        );
    }

    #[test]
    fn the_first_day_may_equal_but_not_follow_the_last() {
        let day = |value: &str| value.parse::<NaiveDate>().ok();
        let results =
            [("2026-10-01", "2026-10-01"), ("2026-10-02", "2026-10-01")].map(|(from, to)| {
                request(XSearchOptions::new(
                    Vec::new(),
                    Vec::new(),
                    day(from),
                    day(to),
                ))
                .validate()
            });

        assert_eq!(
            results,
            [Ok(()), Err("--from must not be later than --to".to_owned())]
        );
    }

    #[test]
    fn a_search_needs_a_query_and_one_to_twenty_posts() {
        let blank = PlatformSearchRequest {
            query: " ".into(),
            ..request(XSearchOptions::default())
        };
        let limits = [0, 1, 20, 21].map(|limit| {
            PlatformSearchRequest {
                limit,
                ..request(XSearchOptions::default())
            }
            .validate()
            .is_ok()
        });

        assert_eq!(
            (blank.validate(), limits),
            (
                Err("x search needs a query".to_owned()),
                [false, true, true, false]
            )
        );
    }

    #[test]
    fn an_ask_checks_its_query_and_filters() {
        let ask = |query: &str, options| XAskRequest {
            query: query.into(),
            options,
        };

        assert_eq!(
            [
                ask("", XSearchOptions::default()).validate(),
                ask(
                    "what do people say",
                    XSearchOptions::new(handles(&["a"]), handles(&["b"]), None, None)
                )
                .validate(),
                ask("what do people say", XSearchOptions::default()).validate(),
            ],
            [
                Err("x ask needs a query".to_owned()),
                Err("--handle cannot be combined with --exclude-handle".to_owned()),
                Ok(()),
            ]
        );
    }
}
