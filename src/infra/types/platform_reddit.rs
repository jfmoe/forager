//! Reddit shapes: post refs, search options, and post fields.

use std::borrow::Cow;
use std::fmt;

use serde::{Deserialize, Serialize};

use super::platform::{Platform, PlatformRefError};

const REDDIT_MAX_LIMIT: u16 = 25;
const REF_PREFIX: &str = "reddit:";
const POST_PREFIX: &str = "t3_";
const COMMENT_PREFIX: &str = "t1_";
const CANONICAL_HOST: &str = "www.reddit.com";
const SITE_HOSTS: [&str; 6] = [
    "reddit.com",
    "www.reddit.com",
    "old.reddit.com",
    "new.reddit.com",
    "np.reddit.com",
    "m.reddit.com",
];
const SHORT_HOST: &str = "redd.it";
// Reddit IDs are base36; comment IDs reach 13 characters, posts are shorter.
const MAX_ID_LENGTH: usize = 13;
const MAX_SUBREDDIT_LENGTH: usize = 21;

const REF_HINT: &str = "pass a `reddit:<id>` ref or a Reddit post URL such as https://www.reddit.com/r/<subreddit>/comments/<id>/";
const SHARE_HINT: &str = "a share link needs the network to resolve; open it in a browser and pass the post URL it leads to";

#[derive(Clone, Debug, Eq, PartialEq)]
/// A Reddit post identity: its base36 ID, without the `t3_` type prefix. Posts have no
/// versions, and the subreddit is not part of the identity.
pub struct RedditRef {
    id: String,
}

impl RedditRef {
    /// Parses `reddit:<id>`, `reddit:t3_<id>`, a reddit.com post URL, or a `redd.it/<id>` short
    /// link.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformRefError`] for any other input. A comment permalink names the post it
    /// belongs to in the hint; a share link (`/s/<code>`) needs the network and is refused.
    pub fn parse(input: &str) -> Result<Self, PlatformRefError> {
        let trimmed = input.trim();
        let parsed = match trimmed.get(..REF_PREFIX.len()) {
            Some(prefix) if prefix.eq_ignore_ascii_case(REF_PREFIX) => {
                Self::from_fullname(&trimmed[REF_PREFIX.len()..]).ok_or(Cow::Borrowed(REF_HINT))
            }
            _ => parse_url(trimmed),
        };
        parsed.map_err(|hint| PlatformRefError {
            platform: Platform::Reddit,
            input: Some(input.to_owned()),
            hint,
        })
    }

    /// Returns the ref of a post ID, with or without its `t3_` prefix.
    #[must_use]
    pub fn from_fullname(value: &str) -> Option<Self> {
        let id = match value.get(..POST_PREFIX.len()) {
            Some(prefix) if prefix.eq_ignore_ascii_case(POST_PREFIX) => &value[POST_PREFIX.len()..],
            _ => value,
        };
        Self::from_id(id)
    }

    /// Returns the ref of a bare base36 post ID; case does not matter, and the ref keeps it in
    /// lowercase as Reddit prints it.
    #[must_use]
    pub fn from_id(value: &str) -> Option<Self> {
        is_id(value).then(|| Self {
            id: value.to_ascii_lowercase(),
        })
    }

    /// Returns the base36 post ID.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the post page without a subreddit or slug; Reddit redirects it to the post.
    #[must_use]
    pub fn canonical_url(&self) -> String {
        format!("https://{CANONICAL_HOST}/comments/{}/", self.id)
    }
}

impl fmt::Display for RedditRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.id)
    }
}

fn is_id(value: &str) -> bool {
    (1..=MAX_ID_LENGTH).contains(&value.len())
        && !value.starts_with('0')
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

/// Returns whether `value` is a subreddit name as Reddit allows it: letters, digits, and `_`.
fn is_subreddit(value: &str) -> bool {
    (1..=MAX_SUBREDDIT_LENGTH).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// Parses `http(s)://<reddit host>/[r/<sub>/|user/<name>/]comments/<id>[/<slug>][/]` and
/// `http(s)://redd.it/<id>`, ignoring the query, the fragment, and a trailing `/`.
fn parse_url(input: &str) -> Result<RedditRef, Cow<'static, str>> {
    let unrecognized = || Cow::Borrowed(REF_HINT);
    let scheme_end = input.find("://").ok_or_else(unrecognized)?;
    if !matches!(
        input[..scheme_end].to_ascii_lowercase().as_str(),
        "http" | "https"
    ) {
        return Err(unrecognized());
    }
    let rest = &input[scheme_end + 3..];
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.to_ascii_lowercase();
    let segments = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if host == SHORT_HOST {
        return match segments.as_slice() {
            [id] => RedditRef::from_id(id).ok_or_else(unrecognized),
            _ => Err(unrecognized()),
        };
    }
    if !SITE_HOSTS.contains(&host.as_str()) {
        return Err(unrecognized());
    }
    let after_owner = match segments.as_slice() {
        ["r", subreddit, rest @ ..] if is_subreddit(subreddit) => rest,
        ["user" | "u", _, rest @ ..] | rest => rest,
    };
    match after_owner {
        ["s", _] => Err(Cow::Borrowed(SHARE_HINT)),
        ["comments", id, rest @ ..] => {
            let post = RedditRef::from_id(id).ok_or_else(unrecognized)?;
            match rest {
                [] | [_] => Ok(post),
                [_, comment] if is_comment_id(comment) => Err(Cow::Owned(format!(
                    "this URL names comment {COMMENT_PREFIX}{} of post reddit:{post}; Reddit refs name posts, so fetch reddit:{post} to read the post and its discussion",
                    comment.to_ascii_lowercase()
                ))),
                _ => Err(unrecognized()),
            }
        }
        _ => Err(unrecognized()),
    }
}

fn is_comment_id(value: &str) -> bool {
    is_id(value)
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// The order of Reddit search results, as the site's search offers it.
pub enum RedditSort {
    /// Best match first.
    #[default]
    Relevance,
    /// Currently popular first.
    Hot,
    /// Highest score first.
    Top,
    /// Newest first.
    New,
    /// Most comments first.
    Comments,
}

impl RedditSort {
    /// Returns the stable identifier, which is also the site's `sort` parameter.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Relevance => "relevance",
            Self::Hot => "hot",
            Self::Top => "top",
            Self::New => "new",
            Self::Comments => "comments",
        }
    }

    /// Returns whether the site applies a time window to this order; it ignores one for `hot`
    /// and `new`.
    #[must_use]
    pub const fn takes_time(self) -> bool {
        matches!(self, Self::Relevance | Self::Top | Self::Comments)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// How far back a Reddit search reaches.
pub enum RedditTime {
    Hour,
    Day,
    Week,
    Month,
    Year,
    All,
}

impl RedditTime {
    /// Returns the stable identifier, which is also the site's `t` parameter.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hour => "hour",
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
            Self::Year => "year",
            Self::All => "all",
        }
    }
}

/// Optional Reddit search scope, order, and window; the site enforces each one. A cursor
/// encodes them completely.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct RedditSearchOptions {
    /// Only posts in this subreddit, without the `r/` prefix.
    pub subreddit: Option<String>,
    pub sort: RedditSort,
    /// The window; absent leaves the site's default.
    pub time: Option<RedditTime>,
}

impl RedditSearchOptions {
    pub(super) fn validate(&self, query: &str, limit: u16) -> Result<(), String> {
        if !(1..=REDDIT_MAX_LIMIT).contains(&limit) {
            return Err(format!("--limit must be between 1 and {REDDIT_MAX_LIMIT}"));
        }
        if query.trim().is_empty() {
            return Err("reddit search needs a query".into());
        }
        if let Some(subreddit) = &self.subreddit
            && !is_subreddit(subreddit)
        {
            return Err(format!(
                "invalid subreddit `{subreddit}`; a subreddit name is up to {MAX_SUBREDDIT_LENGTH} letters, digits, or `_`, without the `r/` prefix"
            ));
        }
        if self.time.is_some() && !self.sort.takes_time() {
            return Err(format!(
                "--time cannot be combined with --sort {}; Reddit applies a time window only to relevance, top, and comments",
                self.sort.as_str()
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// What a Reddit search excerpt quotes.
pub enum RedditSnippetSource {
    /// The post's own text.
    Post,
    /// A comment of the post that matched the query.
    Comment,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// How the comments of a delivered discussion are ordered.
pub enum RedditThreadOrder {
    /// Reddit's `top` order: top-level comments by score, replies nested under them.
    Top,
}

#[derive(Clone, Debug, Default, Serialize)]
/// The Reddit fields of a post. Scores and comment counts are the site's current numbers,
/// which Reddit may fuzz.
pub struct RedditItemData {
    /// The subreddit, without the `r/` prefix.
    pub subreddit: Option<String>,
    /// The post page with its subreddit and slug, as Reddit links it.
    pub permalink: Option<String>,
    /// Reddit's own post type, such as `text`, `link`, `image`, `video`, `gallery`, or
    /// `multi_media`; `null` in search results, which do not carry it.
    pub post_type: Option<String>,
    /// The page a link post points to, or the media an image or video post shows.
    pub link_url: Option<String>,
    pub score: Option<i64>,
    /// The number of comments in the post's discussion.
    pub comments: Option<u64>,
    /// Whether Reddit marks the post as adult content.
    pub nsfw: Option<bool>,
    pub spoiler: Option<bool>,
    /// The excerpt Reddit's search shows for the post.
    pub snippet: Option<String>,
    pub snippet_source: Option<RedditSnippetSource>,
    /// The post's own text as Markdown, without its comments or linked page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// How many comments the discussion file holds, at `thread` depth.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comments_delivered: Option<usize>,
    /// Whether Reddit has more comments than the discussion file holds, at `thread` depth.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_more_comments: Option<bool>,
    /// How the discussion file orders its comments, at `thread` depth.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<RedditThreadOrder>,
}

#[cfg(test)]
mod tests {
    use crate::types::{Platform, PlatformRef, PlatformSearchOptions, PlatformSearchRequest};

    use super::{RedditSearchOptions, RedditSort, RedditTime};

    fn reddit(input: &str) -> Option<String> {
        PlatformRef::parse(Platform::Reddit, input)
            .ok()
            .map(|reference| reference.to_string())
    }

    fn hint(input: &str) -> String {
        PlatformRef::parse(Platform::Reddit, input)
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn every_supported_form_parses_to_the_same_ref() {
        let parsed = [
            "reddit:17snc7d",
            "REDDIT:17SNC7D",
            " reddit:t3_17snc7d ",
            "reddit:T3_17snc7d",
            "https://www.reddit.com/r/running/comments/17snc7d/achievements_for_saturday_november_11_2023/",
            "https://www.reddit.com/r/running/comments/17snc7d",
            "http://reddit.com/r/running/comments/17snc7d/x/?utm_source=share#top",
            "https://old.reddit.com/r/running/comments/17snc7d/",
            "https://new.reddit.com/r/running/comments/17snc7d/",
            "https://np.reddit.com/r/running/comments/17snc7d/",
            "https://m.reddit.com/r/running/comments/17snc7d/",
            "HTTPS://WWW.Reddit.com/r/running/comments/17snc7d/",
            "https://www.reddit.com/comments/17snc7d/",
            "https://www.reddit.com/user/someone/comments/17snc7d/slug/",
            "https://www.reddit.com/u/someone/comments/17snc7d/",
            "https://redd.it/17snc7d",
        ]
        .map(reddit);

        assert_eq!(parsed, [(); 16].map(|()| Some("reddit:17snc7d".to_owned())));
    }

    #[test]
    fn other_pages_hosts_and_bad_ids_are_rejected() {
        let parsed = [
            "https://www.reddit.com/r/running/",
            "https://www.reddit.com/r/running/comments/",
            "https://www.reddit.com/user/someone/",
            "https://www.reddit.com/r/running/comments/17snc7d/slug/k8so54m/extra/",
            "https://www.reddit.com/r/bad-name/comments/17snc7d/",
            "https://www.reddit.com/r/running/comments/017snc7d/",
            "https://www.reddit.com/r/running/comments/17snc_7d/",
            "https://reddit.com.evil.example/r/running/comments/17snc7d/",
            "https://i.redd.it/rvyyy1ket1xh1.jpeg",
            "https://redd.it/17snc7d/extra",
            "https://embed.reddit.com/r/running/comments/17snc7d/",
            "ftp://www.reddit.com/r/running/comments/17snc7d/",
            "17snc7d",
            "reddit:",
            "reddit:t1_k8so54m",
            "reddit:abcdefghijklmn",
        ]
        .map(reddit);

        assert_eq!(parsed, [const { None }; 16]);
    }

    #[test]
    fn a_comment_permalink_names_its_post_instead_of_dropping_the_comment() {
        let hints = [
            "https://www.reddit.com/r/running/comments/17snc7d/comment/k8so54m/",
            "https://www.reddit.com/r/running/comments/17snc7d/achievements/K8SO54M/",
            "https://www.reddit.com/comments/17snc7d/_/k8so54m/",
        ]
        .map(|input| hint(input).rsplit("; ").next().map(str::to_owned));

        assert_eq!(
            hints,
            [(); 3].map(|()| Some("Reddit refs name posts, so fetch reddit:17snc7d to read the post and its discussion".to_owned()))
        );
        assert!(
            hint("https://www.reddit.com/comments/17snc7d/_/k8so54m/")
                .contains("this URL names comment t1_k8so54m of post reddit:17snc7d")
        );
    }

    #[test]
    fn a_share_link_is_refused_because_resolving_it_needs_the_network() {
        let hints = [
            "https://www.reddit.com/r/running/s/3TzXiyxaMD",
            "https://www.reddit.com/s/3TzXiyxaMD",
        ]
        .map(|input| {
            hint(input).ends_with("open it in a browser and pass the post URL it leads to")
        });

        assert_eq!(hints, [true, true]);
    }

    #[test]
    fn unrecognized_input_names_the_accepted_forms() {
        assert_eq!(
            hint("https://www.reddit.com/r/running/"),
            "unrecognized reddit reference `https://www.reddit.com/r/running/`; pass a `reddit:<id>` ref or a Reddit post URL such as https://www.reddit.com/r/<subreddit>/comments/<id>/"
        );
    }

    #[test]
    fn canonical_urls_round_trip_to_the_ref() {
        let reference = PlatformRef::parse(Platform::Reddit, "reddit:17snc7d").expect("ref");

        let round_trip = PlatformRef::parse(Platform::Reddit, &reference.canonical_url());

        assert_eq!(
            (reference.canonical_url(), reference.kind(), round_trip.ok()),
            (
                "https://www.reddit.com/comments/17snc7d/".to_owned(),
                "post",
                Some(reference)
            )
        );
    }

    fn validate(query: &str, limit: u16, options: RedditSearchOptions) -> Result<(), String> {
        PlatformSearchRequest {
            query: query.into(),
            limit,
            options: PlatformSearchOptions::Reddit(options),
            page: None,
        }
        .validate()
    }

    #[test]
    fn invalid_requests_are_rejected_before_any_request() {
        let results = [
            validate(" ", 10, RedditSearchOptions::default()),
            validate("rust", 0, RedditSearchOptions::default()),
            validate("rust", 26, RedditSearchOptions::default()),
            validate(
                "rust",
                10,
                RedditSearchOptions {
                    subreddit: Some("r/rust".into()),
                    ..RedditSearchOptions::default()
                },
            ),
            validate(
                "rust",
                10,
                RedditSearchOptions {
                    sort: RedditSort::New,
                    time: Some(RedditTime::Week),
                    ..RedditSearchOptions::default()
                },
            ),
        ]
        .map(Result::err);

        assert_eq!(
            results,
            [
                Some("reddit search needs a query".into()),
                Some("--limit must be between 1 and 25".into()),
                Some("--limit must be between 1 and 25".into()),
                Some("invalid subreddit `r/rust`; a subreddit name is up to 21 letters, digits, or `_`, without the `r/` prefix".into()),
                Some("--time cannot be combined with --sort new; Reddit applies a time window only to relevance, top, and comments".into()),
            ]
        );
    }

    #[test]
    fn a_window_applies_to_the_orders_that_take_one() {
        let results = [RedditSort::Relevance, RedditSort::Top, RedditSort::Comments].map(|sort| {
            validate(
                "rust",
                25,
                RedditSearchOptions {
                    subreddit: Some("rust_gamedev".into()),
                    sort,
                    time: Some(RedditTime::Hour),
                },
            )
        });

        assert_eq!(results, [Ok(()), Ok(()), Ok(())]);
    }
}
