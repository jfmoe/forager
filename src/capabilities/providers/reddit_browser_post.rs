//! Reddit fetch: opening one post page, projecting the post and the comments the page loaded
//! with it, and judging the projection: page facts, the post's identity, its fields, and the
//! comment order.
//!
//! The page is projected once, as soon as it shows the post: the route never scrolls for more
//! top-level comments or expands replies, so the comments delivered are the first batch the
//! site rendered.

use reqwest::Url;
use scraper::Html;
use serde::Deserialize;

use super::markup::reddit_markdown;
use super::thread::{Comment, render};
use super::{
    POLL, PageFacts, Projection, ROUTE, RedditBrowser, SITE_ORIGIN, blocked, capture,
    document_status, project, projection_script, runtime, site_path, site_timestamp, unfinished,
    unread,
};
use crate::catalog::PlatformOperation;
use crate::net::AttemptFailure;
use crate::providers::browser::Page;
use crate::providers::browser_route::PageReading;
use crate::providers::execution::execute_anonymous;
use crate::providers::shared::{other_platform_message, parameter_error};
use crate::types::{
    AttemptErrorKind, ContentDepth, FullTextSource, PlatformFetchOutcome, PlatformFetchRequest,
    PlatformItem, PlatformItemData, PlatformRef, ProviderError, RedditItemData, RedditRef,
    RedditThreadOrder,
};

/// The post types the platform's fields describe; any other is passed through as the site
/// names it, and has no full text.
const KNOWN_POST_TYPES: [&str; 6] = ["text", "link", "image", "video", "gallery", "multi_media"];

/// Returns whether the route can fetch at the requested depth; it never starts a process. The
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

impl RedditBrowser {
    /// Opens the post page in one browser command and reads the post and the comments it
    /// loaded with it. The same attempt judges the page and builds the item, the discussion
    /// file at `thread` depth, and the native full text of a post with text of its own.
    pub(crate) async fn fetch(
        &self,
        request: &PlatformFetchRequest,
    ) -> Result<PlatformFetchOutcome, ProviderError> {
        fetch_support(request).map_err(parameter_error)?;
        let PlatformRef::Reddit(reference) = &request.reference else {
            unreachable!("support checked")
        };
        let target = PostTarget::new(reference);
        let depth = request.depth;
        let execution = execute_anonymous(
            self.settings(PlatformOperation::Fetch.as_str()),
            |deadline| {
                let target = &target;
                async move {
                    let observed = self
                        .browser
                        .read(capture, deadline, async |page| {
                            observe_post(page, target).await
                        })
                        .await?;
                    Ok((None, post_outcome(&observed, reference, depth)?))
                }
            },
        )
        .await?;
        let (item, content_source) = execution.value;
        Ok(PlatformFetchOutcome {
            item,
            content_source,
            attempts: execution.attempts,
            diagnostic: execution.diagnostic,
        })
    }
}

/// The post page a fetch opens: the site completes the subreddit and slug of `/comments/<id>/`
/// itself, and `sort=top` asks for its comments by score.
#[derive(Debug)]
struct PostTarget {
    url: String,
    path: String,
}

impl PostTarget {
    fn new(reference: &RedditRef) -> Self {
        let path = format!("/comments/{}/", reference.id());
        Self {
            url: format!("{SITE_ORIGIN}{path}?sort=top"),
            path,
        }
    }
}

/// What one projection of a post page shows.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct PostProjection {
    page: PageFacts,
    /// Whether the page has a fragment of more comments to load, top-level or replies.
    more_comments: bool,
    /// The post's ID and adult flag from the page's screen-view record.
    #[serde(deserialize_with = "Option::deserialize")]
    screenview: Option<ScreenView>,
    /// The page's post element; `None` when the page shows no post.
    #[serde(deserialize_with = "Option::deserialize")]
    post: Option<PostRecord>,
    /// The `sort` the comment tree reports, such as `CONFIDENCE`.
    #[serde(deserialize_with = "Option::deserialize")]
    sort: Option<String>,
    /// The comment elements in reading order.
    comments: Vec<CommentRecord>,
}

impl Projection for PostProjection {
    fn page_mut(&mut self) -> &mut PageFacts {
        &mut self.page
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct ScreenView {
    #[serde(deserialize_with = "Option::deserialize")]
    id: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    nsfw: Option<bool>,
}

/// The attributes of the post element, and the HTML of its body.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct PostRecord {
    #[serde(deserialize_with = "Option::deserialize")]
    id: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    title: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    author: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    score: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    comment_count: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    created: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    post_type: Option<String>,
    /// The post page path with its subreddit and slug.
    #[serde(deserialize_with = "Option::deserialize")]
    permalink: Option<String>,
    /// The URL the post points to: its own page, a linked page, or its media.
    #[serde(deserialize_with = "Option::deserialize")]
    content_href: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    body: Option<String>,
}

/// The attributes of one comment element, and the HTML of its body.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct CommentRecord {
    #[serde(deserialize_with = "Option::deserialize")]
    id: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    depth: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    author: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    created: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    score: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    permalink: Option<String>,
    deleted: bool,
    author_deleted: bool,
    #[serde(deserialize_with = "Option::deserialize")]
    body: Option<String>,
}

// The post, its comments, and the comment order. The screen-view record also says things about
// the viewer, so only the post's ID and adult flag leave the page; the post element carries the
// viewer's account ID, so only the attributes forager reads do.
const POST_PROJECTION: &str = r#"
  const attribute = (element, name) => (element ? element.getAttribute(name) : null);
  const html = (id) => {
    const element = document.getElementById(id);
    return element ? element.innerHTML : null;
  };
  let screenview = null;
  const record = document.querySelector('shreddit-screenview-data[data]');
  if (record) {
    try {
      const viewed = JSON.parse(record.getAttribute('data')).post || {};
      screenview = { id: viewed.id === undefined ? null : viewed.id, nsfw: viewed.nsfw === undefined ? null : viewed.nsfw };
    } catch { screenview = null; }
  }
  const element = document.querySelector('shreddit-post');
  const id = attribute(element, 'id');
  const post = element ? {
    id,
    title: attribute(element, 'post-title'),
    author: attribute(element, 'author'),
    score: attribute(element, 'score'),
    comment_count: attribute(element, 'comment-count'),
    created: attribute(element, 'created-timestamp'),
    post_type: attribute(element, 'post-type'),
    permalink: attribute(element, 'permalink'),
    content_href: attribute(element, 'content-href'),
    body: id ? html(id + '-post-rtjson-content') : null,
  } : null;
  const comments = [...document.querySelectorAll('shreddit-comment[thingid]')].map((comment) => {
    const thing = comment.getAttribute('thingid');
    return {
      id: thing,
      depth: comment.getAttribute('depth'),
      author: comment.getAttribute('author'),
      created: comment.getAttribute('created'),
      score: comment.getAttribute('score'),
      permalink: comment.getAttribute('permalink'),
      deleted: comment.hasAttribute('is-comment-deleted'),
      author_deleted: comment.hasAttribute('is-author-deleted'),
      body: html(thing + '-comment-rtjson-content'),
    };
  });
  return {
    page,
    more_comments: document.querySelector('faceplate-partial[src*="/svc/shreddit/more-comments/"]') !== null,
    screenview,
    post,
    sort: attribute(document.querySelector('shreddit-comment-tree-stats'), 'sort'),
    comments,
  };"#;

/// What one fetch read observed.
#[derive(Debug, Default)]
struct PostObservation {
    projection: PostProjection,
    document_status: Option<u16>,
    /// The read deadline passed before the page reached a final state.
    timed_out: bool,
}

/// Whether `projection` ends the read: the page shows the post, a block, or a loaded page
/// of a logged-in session without a post, which renders none later.
fn post_ended(projection: &PostProjection, document_status: Option<u16>) -> bool {
    let page = &projection.page;
    projection.post.is_some()
        || blocked(page, document_status).is_some()
        || (page.logged_in() && page.loaded())
}

/// Drives one fetch: opens the post page and projects it every half second until it reaches a
/// final state or the read deadline passes.
async fn observe_post(
    page: &mut Page,
    target: &PostTarget,
) -> Result<PostObservation, AttemptFailure> {
    let script = projection_script(POST_PROJECTION);
    let mut reading = PageReading::new(page);
    reading.navigate(&target.url).await?;
    let mut last: Option<PostProjection> = None;
    let mut status = None;
    let mut ended = false;
    while reading.page().reading() {
        if let Some(projection) = project::<PostProjection>(&mut reading, &script).await? {
            status = document_status(reading.exchanges(), &target.path, &projection.page.url);
            ended = post_ended(&projection, status);
            last = Some(projection);
            if ended {
                break;
            }
        }
        reading.page().rest(POLL).await;
    }
    Ok(PostObservation {
        projection: last.unwrap_or_else(|| PostProjection {
            page: unread(&reading),
            ..PostProjection::default()
        }),
        document_status: status,
        timed_out: !ended,
    })
}

/// Judges one fetch read: classifies the page facts, checks the post's identity, and builds the
/// item and its content source for `depth`.
fn post_outcome(
    observed: &PostObservation,
    reference: &RedditRef,
    depth: ContentDepth,
) -> Result<(PlatformItem, FullTextSource), AttemptFailure> {
    let projection = &observed.projection;
    let post = shown_post(observed, reference)?;
    let thread = depth == ContentDepth::Thread;
    let (comments, order) = if thread {
        (
            decode_comments(&projection.comments),
            Some(order(projection)?),
        )
    } else {
        (Vec::new(), None)
    };
    let mut item = post_item(projection, post, reference)?;
    if let (PlatformItemData::Reddit(data), Some(order)) = (&mut item.data, order) {
        item.depth = ContentDepth::Thread;
        data.comments_delivered = Some(comments.len());
        data.has_more_comments = Some(projection.more_comments);
        data.order = Some(order);
    }
    let source = match (depth, order) {
        (ContentDepth::Thread, Some(order)) => {
            FullTextSource::Native(render(&item, &comments, order))
        }
        (ContentDepth::FullText, _) => full_text_source(reference, &item)?,
        _ => FullTextSource::Urls(Vec::new()),
    };
    Ok((item, source))
}

/// The post the page shows, once the page facts and the post's identity allow delivering it.
fn shown_post<'o>(
    observed: &'o PostObservation,
    reference: &RedditRef,
) -> Result<&'o PostRecord, AttemptFailure> {
    let projection = &observed.projection;
    let facts = &projection.page;
    if let Some(failure) = blocked(facts, observed.document_status) {
        return Err(failure);
    }
    let Some(post) = &projection.post else {
        if observed.timed_out {
            return Err(unfinished(
                facts,
                is_post_page(&facts.url, reference),
                || "the Reddit post page showed no post before the read deadline".to_owned(),
            ));
        }
        // A site failure is no statement about the post.
        if let Some(status) = observed.document_status.filter(|status| *status >= 500) {
            return Err(AttemptFailure {
                kind: AttemptErrorKind::Runtime,
                status: Some(status),
                message: format!(
                    "Reddit answered the post page with HTTP {status} and showed no post; the site may be failing, so retry later"
                ),
            });
        }
        return Err(AttemptFailure {
            kind: AttemptErrorKind::Parameter,
            status: None,
            message: format!(
                "Reddit post not found: reddit:{reference}; it may be deleted, private, or restricted, or hidden by the account's adult-content setting"
            ),
        });
    };
    let fullname = format!("t3_{}", reference.id());
    let shown = [
        post.id.as_deref(),
        projection
            .screenview
            .as_ref()
            .and_then(|view| view.id.as_deref()),
    ];
    if let Some(other) = shown.into_iter().flatten().find(|id| *id != fullname) {
        return Err(runtime(format!(
            "the Reddit post page shows {other} instead of {fullname}"
        )));
    }
    if post.id.is_none() {
        return Err(format_change(&fullname, "ID"));
    }
    Ok(post)
}

/// Builds the metadata item of a post whose identity is checked.
fn post_item(
    projection: &PostProjection,
    post: &PostRecord,
    reference: &RedditRef,
) -> Result<PlatformItem, AttemptFailure> {
    let fullname = format!("t3_{}", reference.id());
    let title = post
        .title
        .clone()
        .filter(|title| !title.trim().is_empty())
        .ok_or_else(|| format_change(&fullname, "title"))?;
    let permalink_path = post
        .permalink
        .as_deref()
        .filter(|path| path.starts_with('/'))
        .ok_or_else(|| format_change(&fullname, "permalink"))?;
    let permalink = format!("{SITE_ORIGIN}{permalink_path}");
    Ok(PlatformItem {
        url: reference.canonical_url(),
        reference: PlatformRef::Reddit(reference.clone()),
        depth: ContentDepth::Metadata,
        title: Some(title),
        authors: post
            .author
            .clone()
            .filter(|author| named(author))
            .into_iter()
            .collect(),
        published: post.created.as_deref().and_then(site_timestamp),
        data: PlatformItemData::Reddit(RedditItemData {
            subreddit: subreddit(permalink_path, reference),
            post_type: post.post_type.clone(),
            link_url: post
                .content_href
                .clone()
                .filter(|href| href.trim_end_matches('/') != permalink.trim_end_matches('/')),
            permalink: Some(permalink),
            score: post.score.as_deref().and_then(|score| score.parse().ok()),
            comments: post
                .comment_count
                .as_deref()
                .and_then(|count| count.parse().ok()),
            nsfw: projection.screenview.as_ref().and_then(|view| view.nsfw),
            spoiler: None,
            snippet: None,
            snippet_source: None,
            text: post.body.as_deref().and_then(markdown),
            comments_delivered: None,
            has_more_comments: None,
            order: None,
        }),
    })
}

fn format_change(fullname: &str, field: &str) -> AttemptFailure {
    runtime(format!(
        "the Reddit post page gives no {field} for {fullname}; the page format may have changed"
    ))
}

/// The comment order the page's comment tree reports.
fn order(projection: &PostProjection) -> Result<RedditThreadOrder, AttemptFailure> {
    match projection.sort.as_deref() {
        Some("TOP") => Ok(RedditThreadOrder::Top),
        Some("CONFIDENCE") => Ok(RedditThreadOrder::Best),
        Some("NEW") => Ok(RedditThreadOrder::New),
        Some("CONTROVERSIAL") => Ok(RedditThreadOrder::Controversial),
        Some("OLD") => Ok(RedditThreadOrder::Old),
        Some("QA") => Ok(RedditThreadOrder::Qa),
        Some(other) => Err(runtime(format!(
            "the Reddit post page orders its comments by `{other}`, which forager does not know; the page format may have changed"
        ))),
        None => Err(runtime(
            "the Reddit post page does not say how its comments are ordered; the page format may have changed".into(),
        )),
    }
}

/// Reads the comments in reading order; an element that names no comment is left out. A
/// deleted or removed comment keeps its place without text, so its replies keep their parent.
fn decode_comments(records: &[CommentRecord]) -> Vec<Comment> {
    records
        .iter()
        .filter_map(|record| {
            let id = record.id.as_deref()?.strip_prefix("t1_")?;
            Some(Comment {
                id: id.to_owned(),
                depth: record
                    .depth
                    .as_deref()
                    .and_then(|depth| depth.parse().ok())
                    .unwrap_or_default(),
                author: record
                    .author
                    .clone()
                    .filter(|author| !record.author_deleted && named(author)),
                published: record.created.as_deref().and_then(site_timestamp),
                score: record.score.as_deref().and_then(|score| score.parse().ok()),
                permalink: record
                    .permalink
                    .as_deref()
                    .filter(|path| path.starts_with('/'))
                    .map(|path| format!("{SITE_ORIGIN}{path}")),
                text: (!record.deleted)
                    .then(|| record.body.as_deref().and_then(markdown))
                    .flatten(),
            })
        })
        .collect()
}

/// Whether `author` names an account: the site shows `[deleted]` for a deleted one.
fn named(author: &str) -> bool {
    !author.is_empty() && !author.starts_with('[')
}

/// Converts the inner HTML of a body element to Markdown.
fn markdown(body: &str) -> Option<String> {
    reddit_markdown(Html::parse_fragment(body).root_element())
}

/// Returns the subreddit of a post page path such as `/r/<sub>/comments/<id>/<slug>/`; a post
/// on a user profile belongs to the profile's `u_<name>` subreddit.
fn subreddit(path: &str, reference: &RedditRef) -> Option<String> {
    let segments = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    match segments.as_slice() {
        ["r", subreddit, "comments", id, ..] if *id == reference.id() => {
            Some((*subreddit).to_owned())
        }
        ["user", name, "comments", id, ..] if *id == reference.id() => Some(format!("u_{name}")),
        _ => None,
    }
}

/// Whether `url` is the page of the requested post, under any subreddit and slug the site
/// redirected it to.
fn is_post_page(url: &str, reference: &RedditRef) -> bool {
    let Some(path) = Url::parse(url).ok().and_then(|url| site_path(&url)) else {
        return false;
    };
    let segments = path.split('/').collect::<Vec<_>>();
    segments
        .windows(2)
        .any(|pair| pair[0] == "comments" && pair[1] == reference.id())
}

/// Says where the full text of a post lives: the page a link post points to, or the post's own
/// title and text. A post of a type forager does not know, and an image or video post without
/// text, has none to read.
fn full_text_source(
    reference: &RedditRef,
    item: &PlatformItem,
) -> Result<FullTextSource, AttemptFailure> {
    let PlatformItemData::Reddit(data) = &item.data else {
        unreachable!("the reddit route builds reddit items")
    };
    let kind = data.post_type.as_deref().unwrap_or("untyped");
    let known = KNOWN_POST_TYPES.contains(&kind);
    if kind == "link"
        && let Some(url) = &data.link_url
    {
        return Ok(FullTextSource::Urls(vec![url.clone()]));
    }
    if known && let Some(text) = &data.text {
        let title = item.title.as_deref().unwrap_or_default();
        return Ok(FullTextSource::Native(format!("# {title}\n\n{text}")));
    }
    let media = data
        .link_url
        .as_ref()
        .map(|url| format!("; its media is at {url}"))
        .unwrap_or_default();
    let reason = if known {
        "without text of its own"
    } else {
        "that forager has no full text for"
    };
    Err(AttemptFailure {
        kind: AttemptErrorKind::Quality,
        status: None,
        message: format!(
            "reddit:{reference} is a post of type `{kind}` {reason}{media}; use --depth thread to read its discussion"
        ),
    })
}

#[cfg(test)]
#[path = "reddit_browser_post_tests.rs"]
mod tests;
