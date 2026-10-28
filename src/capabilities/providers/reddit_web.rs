//! The `reddit_web` route: Reddit's own site pages, read anonymously (ADR 0033).
//!
//! Reddit's public Data API takes no new apps and is being retired, so the route reads what the
//! site itself renders: the search results fragment, the comment-tree fragment of a post, and
//! the post's embed page, the only anonymous page that carries the post's title, author, score,
//! and text. These pages are undocumented, so users enable the route themselves. Every request
//! to either origin follows the route's one access policy and none is retried; a verification
//! page stops the route instead of being answered.

use std::time::Duration;

use reqwest::Client;
use scraper::{Html, Selector};

use crate::catalog::ProviderId;
use crate::config::RedditRouteRuntimeConfig;
use crate::credentials::CredentialPool;
use crate::net::{RetryPolicy, read_complete_protocol, send_provider_request};
use crate::providers::execution::{ExecutionOutcome, ExecutionSettings, execute_anonymous};
use crate::providers::shared::{
    Refusal, Trail, acquire_window, other_platform_message, parameter_error, utc_second,
};
use crate::rate_limit::RateLimiter;
use crate::types::{
    AttemptErrorKind, AttemptTarget, ContentDepth, Deadline, FullTextSource, Platform,
    PlatformFetchOutcome, PlatformFetchRequest, PlatformItem, PlatformItemData, PlatformRef,
    ProviderError, RedditItemData, RedditRef, RedditThreadOrder,
};

#[path = "reddit_web_embed.rs"]
mod embed;
#[path = "reddit_markup.rs"]
mod markup;
#[path = "reddit_web_search.rs"]
mod search;
#[path = "reddit_web_thread.rs"]
mod thread;

pub(crate) use search::search_support;

use embed::EmbedPost;
use thread::CommentsPage;

const ROUTE: ProviderId = ProviderId::RedditWeb;
const PLATFORM: Platform = Platform::Reddit;
const SITE_ORIGIN: &str = "https://www.reddit.com";
// The comment-tree fragment answers for a post under any subreddit path; `all` names none.
const ANY_SUBREDDIT: &str = "all";
const IP_RESTRICTED: &str =
    "Reddit is refusing this egress IP; wait, or switch networks, and try again";

/// Returns whether the route can fetch at the requested depth; it never sends a request. The
/// post type is unknown until the post is read, so an image post asked for its full text fails
/// later.
pub(crate) fn fetch_support(request: &PlatformFetchRequest) -> Result<(), String> {
    if !matches!(request.reference, PlatformRef::Reddit(_)) {
        return Err(other_platform_message(ROUTE, request.reference.platform()));
    }
    match request.depth {
        ContentDepth::Thread | ContentDepth::Metadata | ContentDepth::FullText => Ok(()),
        depth => Err(format!(
            "{} cannot fetch at depth `{}`",
            ROUTE.name(),
            depth.as_str()
        )),
    }
}

/// The pages the route reads.
#[derive(Clone, Copy)]
enum Step {
    Search,
    Comments,
    Embed,
}

impl Step {
    const fn timeout_message(self) -> &'static str {
        match self {
            Self::Search => "Reddit search page request timed out",
            Self::Comments => "Reddit comment page request timed out",
            Self::Embed => "Reddit embed page request timed out",
        }
    }

    const fn operation(self) -> &'static str {
        match self {
            Self::Search => "search",
            Self::Comments | Self::Embed => "fetch",
        }
    }
}

pub(crate) struct RedditWeb {
    config: RedditRouteRuntimeConfig,
    client: Client,
    // The route needs no credentials; the empty pool only drives shared redaction.
    credentials: CredentialPool,
    limiter: RateLimiter,
    deadline: Deadline,
}

impl RedditWeb {
    pub(crate) fn new(
        config: RedditRouteRuntimeConfig,
        client: Client,
        limiter: RateLimiter,
        deadline: Deadline,
    ) -> Self {
        Self {
            config,
            client,
            credentials: CredentialPool::new(ROUTE.name(), Vec::new()),
            limiter,
            deadline,
        }
    }

    /// Reads one post: the comment-tree fragment, which names the post's subreddit and counts
    /// its comments, and then the embed page under that subreddit, which carries the post.
    pub(crate) async fn fetch(
        &self,
        request: &PlatformFetchRequest,
    ) -> Result<PlatformFetchOutcome, ProviderError> {
        fetch_support(request).map_err(parameter_error)?;
        let PlatformRef::Reddit(reference) = &request.reference else {
            unreachable!("the support check accepts only Reddit refs")
        };
        let mut trail = Trail::default();
        let comments_url = format!(
            "{}/svc/shreddit/comments/r/{ANY_SUBREDDIT}/t3_{}",
            self.config.url.trim_end_matches('/'),
            reference.id()
        );
        let comments = trail.required(
            self.send(
                Step::Comments,
                &comments_url,
                &[("sort", "top".to_owned())],
                |html| thread::read_comments_page(html, reference),
            )
            .await,
        )?;
        let embed_url = format!(
            "{}{}",
            self.config.embed_url.trim_end_matches('/'),
            comments.permalink_path
        );
        let post = trail.required(
            self.send(Step::Embed, &embed_url, &[], |html| {
                embed::read_embed_page(html, reference)
            })
            .await,
        )?;
        let (item, content_source) = match request.depth {
            ContentDepth::Thread => {
                let item = post_item(reference, post, &comments, ContentDepth::Thread);
                let markdown = thread::render(&item, &comments.comments);
                (item, FullTextSource::Native(markdown))
            }
            ContentDepth::FullText => {
                let item = post_item(reference, post, &comments, ContentDepth::Metadata);
                let source = full_text_source(reference, &item)
                    .map_err(|(kind, message)| trail.error(kind, message))?;
                (item, source)
            }
            _ => (
                post_item(reference, post, &comments, ContentDepth::Metadata),
                FullTextSource::Urls(Vec::new()),
            ),
        };
        Ok(PlatformFetchOutcome {
            item,
            content_source,
            diagnostic: trail.diagnostic(),
            attempts: trail.attempts,
        })
    }

    /// Sends one GET under the route's access policy, once; `decode` turns the page into the
    /// step's value inside the attempt, so its refusal is that attempt's failure.
    async fn send<T>(
        &self,
        step: Step,
        url: &str,
        query: &[(&str, String)],
        decode: impl Fn(&str) -> Result<T, Refusal>,
    ) -> Result<ExecutionOutcome<T>, ProviderError> {
        let decode = &decode;
        execute_anonymous(self.settings(step), |deadline| async move {
            let _permit = acquire_window(&self.limiter, deadline).await?;
            let request = self.client.get(url).query(query);
            let response = send_provider_request(request, &self.credentials).await?;
            let body = read_complete_protocol(response, &self.credentials, failure_message).await?;
            let value = decode(&body.text).map_err(|refusal| refusal.at(body.status))?;
            Ok((Some(body.status), value))
        })
        .await
    }

    fn settings(&self, step: Step) -> ExecutionSettings {
        ExecutionSettings {
            provider: ROUTE.name(),
            target: AttemptTarget::platform(PLATFORM.as_str(), step.operation()),
            // A retry would hit a site that may be restricting this IP again.
            retry_policy: RetryPolicy::new(1, 1.0, Duration::ZERO),
            deadline: self.deadline,
            attempt_timeout: Duration::from_secs(self.config.timeout_seconds),
            verbose: false,
            timeout_message: step.timeout_message(),
            model: None,
            transport: Some("http"),
            endpoint_host: None,
        }
    }
}

/// Builds the fetched item from the embed page and the comment tree; the tree's count is the
/// post's comment count.
fn post_item(
    reference: &RedditRef,
    post: EmbedPost,
    comments: &CommentsPage,
    depth: ContentDepth,
) -> PlatformItem {
    let thread = depth == ContentDepth::Thread;
    PlatformItem {
        url: reference.canonical_url(),
        reference: PlatformRef::Reddit(reference.clone()),
        depth,
        title: Some(post.title),
        authors: post.author.into_iter().collect(),
        published: post.published,
        data: PlatformItemData::Reddit(RedditItemData {
            subreddit: Some(comments.subreddit.clone()),
            permalink: Some(format!("{SITE_ORIGIN}{}", comments.permalink_path)),
            post_type: post.post_type,
            link_url: post.link_url,
            score: post.score,
            comments: Some(comments.total),
            nsfw: post.nsfw,
            spoiler: None,
            snippet: None,
            snippet_source: None,
            text: post.text,
            comments_delivered: thread.then_some(comments.comments.len()),
            has_more_comments: thread.then_some(comments.has_more),
            order: thread.then_some(RedditThreadOrder::Top),
        }),
    }
}

/// Says where the full text of a post lives: the page a link post points to, or the post's own
/// title and text. An image or video post without text has none to read.
fn full_text_source(
    reference: &RedditRef,
    item: &PlatformItem,
) -> Result<FullTextSource, (AttemptErrorKind, String)> {
    let PlatformItemData::Reddit(data) = &item.data else {
        unreachable!("the reddit route builds reddit items")
    };
    if data.post_type.as_deref() == Some("link")
        && let Some(url) = &data.link_url
    {
        return Ok(FullTextSource::Urls(vec![url.clone()]));
    }
    if let Some(text) = &data.text {
        let title = item.title.as_deref().unwrap_or_default();
        return Ok(FullTextSource::Native(format!("# {title}\n\n{text}")));
    }
    let kind = data.post_type.as_deref().unwrap_or("untyped");
    let media = data
        .link_url
        .as_ref()
        .map(|url| format!("; its media is at {url}"))
        .unwrap_or_default();
    Err((
        AttemptErrorKind::Quality,
        format!(
            "reddit:{reference} is a post of type `{kind}` without text of its own{media}; use --depth thread to read its discussion"
        ),
    ))
}

fn failure_message(_body: &str, status: u16) -> String {
    match status {
        403 | 429 => format!("Reddit returned HTTP {status}: {IP_RESTRICTED}"),
        _ => format!("Reddit returned HTTP {status}"),
    }
}

/// Refuses a page that is not the one the step reads: a verification page means the site wants
/// a browser here (Auth); anything else means its format changed (Runtime).
fn unexpected_page(document: &Html, expected: &str) -> Refusal {
    if document
        .select(&selector("input[name=\"js_challenge\"]"))
        .next()
        .is_some()
    {
        Refusal::new(
            AttemptErrorKind::Auth,
            format!(
                "Reddit answered with a verification page that only a browser can pass; {IP_RESTRICTED}"
            ),
        )
    } else {
        Refusal::runtime(format!(
            "the response is not {expected}; the page format may have changed"
        ))
    }
}

/// Parses a selector the route spells as a literal.
fn selector(css: &str) -> Selector {
    Selector::parse(css).unwrap_or_else(|error| panic!("invalid selector `{css}`: {error}"))
}

/// Returns an element's text with HTML whitespace folded.
fn folded_text(element: scraper::ElementRef<'_>) -> String {
    element
        .text()
        .flat_map(str::split_ascii_whitespace)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Formats a site timestamp such as `2026-09-30T17:37:11.509000+0000` as a platform item
/// timestamp.
fn site_timestamp(value: &str) -> Option<String> {
    chrono::DateTime::parse_from_str(value.trim(), "%Y-%m-%dT%H:%M:%S%.f%z")
        .ok()
        .map(|time| utc_second(time.to_utc()))
}

#[cfg(test)]
mod tests {
    use super::site_timestamp;

    #[test]
    fn site_timestamps_become_utc_seconds() {
        assert_eq!(
            [
                site_timestamp("2026-09-30T17:37:11.509000+0000"),
                site_timestamp("2023-11-11T21:39:10+0100"),
                site_timestamp("yesterday"),
            ],
            [
                Some("2026-09-30T17:37:11Z".to_owned()),
                Some("2023-11-11T20:39:10Z".to_owned()),
                None
            ]
        );
    }
}
