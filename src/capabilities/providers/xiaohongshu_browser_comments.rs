//! Xiaohongshu comments: the comment responses of one note page, their order and ownership
//! checks, and the comments and replies they decode into.

use std::collections::{HashMap, HashSet};

use serde::Deserialize;

use super::{
    PageFacts, XiaohongshuBrowser, beijing_time, classify_blocks, count_text, describe,
    is_note_page, note_unavailable, runtime, unfinished_read, without_token,
};
use crate::net::AttemptFailure;
use crate::providers::execution::execute_anonymous;
use crate::types::{
    COMMENTS, ProviderError, XiaohongshuComment, XiaohongshuCommentsOutcome,
    XiaohongshuCommentsRequest, XiaohongshuRef, XiaohongshuReply,
};

/// Top-level comments in one `comment/page` response.
const PAGE_SIZE: u16 = 10;

impl XiaohongshuBrowser {
    /// Reads the top-level comments `limit` needs and expands the first `replies` delivered
    /// comments that have more replies, in one OpenCLI command on the note page.
    pub(crate) async fn comments(
        &self,
        request: &XiaohongshuCommentsRequest,
    ) -> Result<XiaohongshuCommentsOutcome, ProviderError> {
        let token = &request.access;
        let command = self.command(
            "comments",
            vec![
                ("id", request.note.note_id().to_owned()),
                ("xsec-token", token.as_str().to_owned()),
                ("limit", request.limit.to_string()),
                ("expand", request.replies.to_string()),
            ],
        );
        let command = &command;
        let execution = execute_anonymous(self.settings(COMMENTS), move |deadline| async move {
            let read = async {
                let data: CommentsData = self.read_page(command, deadline).await?;
                read_comments(&data, request)
            };
            read.await
                .map(|value| (None, value))
                .map_err(|failure| without_token(failure, token))
        })
        .await?;
        let (comments, has_more) = execution.value;
        Ok(XiaohongshuCommentsOutcome {
            comments,
            has_more,
            attempts: execution.attempts,
            diagnostic: execution.diagnostic,
        })
    }
}

/// The facts the adapter reports for one comments command.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct CommentsData {
    page: PageFacts,
    /// The read deadline passed before the expected responses arrived.
    timed_out: bool,
    /// An awaited comment response completed, but the capture held no body for it.
    body_missing: bool,
    /// Why the adapter could not expand the replies of a selected comment.
    expand_failure: Option<String>,
    /// Every captured `comment/page` and `comment/sub/page` exchange, in order.
    responses: Vec<CapturedComments>,
}

#[derive(Debug, Deserialize)]
struct CapturedComments {
    kind: ResponseKind,
    params: CommentParams,
    body: CommentsBody,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum ResponseKind {
    /// `comment/page`: top-level comments.
    Page,
    /// `comment/sub/page`: the replies of one top-level comment.
    Sub,
}

/// The query parameters of the request that the response answers.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct CommentParams {
    note_id: String,
    cursor: String,
    root_comment_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CommentsBody {
    #[serde(default)]
    msg: Option<String>,
    #[serde(default)]
    data: Option<CommentsPage>,
}

#[derive(Debug, Deserialize)]
struct CommentsPage {
    #[serde(default)]
    comments: Vec<RawComment>,
    #[serde(default)]
    cursor: String,
    has_more: bool,
}

/// A top-level comment or a reply, as Xiaohongshu answers it.
#[derive(Debug, Deserialize)]
struct RawComment {
    id: String,
    #[serde(default)]
    content: Option<String>,
    /// Milliseconds since the Unix epoch.
    #[serde(default)]
    create_time: Option<i64>,
    #[serde(default)]
    ip_location: Option<String>,
    #[serde(default)]
    like_count: Option<serde_json::Value>,
    #[serde(default)]
    sub_comment_count: Option<serde_json::Value>,
    /// Where the first reply page after the carried replies starts.
    #[serde(default)]
    sub_comment_cursor: Option<String>,
    #[serde(default)]
    sub_comment_has_more: bool,
    /// The replies a top-level comment carries, usually one.
    #[serde(default)]
    sub_comments: Vec<RawComment>,
    #[serde(default)]
    user_info: RawUser,
    /// The comment or reply a reply answers.
    #[serde(default)]
    target_comment: Option<RawTarget>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawUser {
    user_id: Option<String>,
    nickname: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawTarget {
    id: String,
}

/// Classifies the page facts, checks that the comment responses answer the request, and
/// decodes the delivered comments with whether more top-level comments remain.
fn read_comments(
    data: &CommentsData,
    request: &XiaohongshuCommentsRequest,
) -> Result<(Vec<XiaohongshuComment>, bool), AttemptFailure> {
    let requested = &request.note;
    classify_blocks(&data.page, |code, notice| {
        note_unavailable(requested, code, notice)
    })?;
    if data.body_missing {
        return Err(runtime(format!(
            "a Xiaohongshu comment response arrived without its body on {}",
            describe(&data.page)
        )));
    }
    if let Some(failure) = &data.expand_failure {
        return Err(runtime(format!(
            "the forager-xhs adapter could not expand the replies of a comment ({failure})"
        )));
    }
    let needed = usize::from(request.limit.div_ceil(PAGE_SIZE));
    let pages = top_level_pages(data, requested, needed)?;
    let mut seen = HashSet::new();
    let mut comments = pages
        .iter()
        .flat_map(|page| &page.comments)
        .filter(|comment| seen.insert(comment.id.as_str()))
        .collect::<Vec<_>>();
    let upstream_has_more = pages.last().is_some_and(|page| page.has_more);
    if comments.is_empty() {
        if pages[0].has_more {
            return Err(runtime(
                "Xiaohongshu listed no comments on the first page but says more remain".into(),
            ));
        }
        return Ok((Vec::new(), false));
    }
    let limit = usize::from(request.limit);
    let has_more = upstream_has_more || comments.len() > limit;
    comments.truncate(limit);
    let expanded = reply_pages(data, requested, &comments, usize::from(request.replies))?;
    let decoded = comments
        .into_iter()
        .map(|comment| decode_comment(comment, expanded.get(comment.id.as_str()).copied()))
        .collect();
    Ok((decoded, has_more))
}

/// Returns the top-level comment pages that answer the request: each for the requested note,
/// each continuing from the cursor the previous one returned, up to `needed` pages or the
/// last one.
fn top_level_pages<'a>(
    data: &'a CommentsData,
    requested: &XiaohongshuRef,
    needed: usize,
) -> Result<Vec<&'a CommentsPage>, AttemptFailure> {
    let answering = data
        .responses
        .iter()
        .filter(|response| response.kind == ResponseKind::Page)
        .collect::<Vec<_>>();
    let mut pages = Vec::new();
    let mut cursor = "";
    for (index, response) in answering.iter().take(needed).enumerate() {
        check_note(&response.params, requested)?;
        if response.params.cursor != cursor {
            return Err(runtime(format!(
                "Xiaohongshu comment page {} continued from cursor `{}`, not `{cursor}`",
                index + 1,
                response.params.cursor
            )));
        }
        let Some(page) = &response.body.data else {
            return Err(runtime(format!(
                "Xiaohongshu comment page {} has no comments data ({})",
                index + 1,
                response.body.msg.as_deref().unwrap_or("no message")
            )));
        };
        pages.push(page);
        if !page.has_more {
            return Ok(pages);
        }
        cursor = &page.cursor;
    }
    if pages.len() < needed {
        return Err(incomplete(
            data,
            requested,
            &format!("{} of {needed} comment pages", pages.len()),
        ));
    }
    Ok(pages)
}

/// Returns the first reply page of each comment the command expands: the first `replies`
/// delivered comments with more replies than they carry. Each reply page must belong to the
/// requested note and one of those comments, and start where the comment's carried replies
/// end.
fn reply_pages<'a>(
    data: &'a CommentsData,
    requested: &XiaohongshuRef,
    delivered: &[&'a RawComment],
    replies: usize,
) -> Result<HashMap<&'a str, &'a CommentsPage>, AttemptFailure> {
    let selected = delivered
        .iter()
        .filter(|comment| comment.sub_comment_has_more)
        .take(replies)
        .map(|comment| (comment.id.as_str(), *comment))
        .collect::<HashMap<_, _>>();
    let mut expanded = HashMap::new();
    for response in data
        .responses
        .iter()
        .filter(|response| response.kind == ResponseKind::Sub)
    {
        check_note(&response.params, requested)?;
        let root = response.params.root_comment_id.as_deref().unwrap_or("");
        let Some(comment) = selected.get(root) else {
            return Err(runtime(format!(
                "Xiaohongshu answered replies of comment `{root}`, which this command did not expand"
            )));
        };
        let start = comment.sub_comment_cursor.as_deref().unwrap_or("");
        if response.params.cursor != start {
            return Err(runtime(format!(
                "the first Xiaohongshu reply page of comment `{root}` started from cursor `{}`, not `{start}`",
                response.params.cursor
            )));
        }
        let Some(page) = &response.body.data else {
            return Err(runtime(format!(
                "the Xiaohongshu reply page of comment `{root}` has no replies data ({})",
                response.body.msg.as_deref().unwrap_or("no message")
            )));
        };
        if expanded.insert(comment.id.as_str(), page).is_some() {
            return Err(runtime(format!(
                "Xiaohongshu answered more than one reply page of comment `{root}`"
            )));
        }
    }
    if expanded.len() < selected.len() {
        return Err(incomplete(
            data,
            requested,
            &format!("{} of {} reply expansions", expanded.len(), selected.len()),
        ));
    }
    Ok(expanded)
}

fn check_note(params: &CommentParams, requested: &XiaohongshuRef) -> Result<(), AttemptFailure> {
    if params.note_id == requested.note_id() {
        return Ok(());
    }
    Err(runtime(format!(
        "Xiaohongshu answered comments of note `{}` for xiaohongshu:{requested}",
        params.note_id
    )))
}

/// Explains a read that ended before it had every comment page or reply page it needed.
fn incomplete(data: &CommentsData, requested: &XiaohongshuRef, missing: &str) -> AttemptFailure {
    let on_note = is_note_page(&data.page.url, requested);
    unfinished_read(&data.page, data.timed_out, on_note, || {
        format!("Xiaohongshu returned {missing} before the read deadline")
    })
    .unwrap_or_else(|| {
        runtime(format!(
            "the forager-xhs adapter returned {missing} without timing out"
        ))
    })
}

fn decode_comment(comment: &RawComment, expanded: Option<&CommentsPage>) -> XiaohongshuComment {
    let mut seen = HashSet::new();
    let replies = comment
        .sub_comments
        .iter()
        .chain(expanded.into_iter().flat_map(|page| &page.comments))
        .filter(|reply| seen.insert(reply.id.as_str()))
        .map(|reply| decode_reply(reply, &comment.id))
        .collect();
    XiaohongshuComment {
        id: comment.id.clone(),
        author: text(comment.user_info.nickname.as_ref()),
        author_id: text(comment.user_info.user_id.as_ref()),
        text: comment.content.clone().unwrap_or_default(),
        likes: count_text(comment.like_count.as_ref()),
        published: comment.create_time.and_then(beijing_time),
        ip_location: text(comment.ip_location.as_ref()),
        reply_count: count_text(comment.sub_comment_count.as_ref()),
        replies,
        replies_has_more: expanded.map_or(comment.sub_comment_has_more, |page| page.has_more),
    }
}

fn decode_reply(reply: &RawComment, root: &str) -> XiaohongshuReply {
    XiaohongshuReply {
        id: reply.id.clone(),
        author: text(reply.user_info.nickname.as_ref()),
        author_id: text(reply.user_info.user_id.as_ref()),
        text: reply.content.clone().unwrap_or_default(),
        likes: count_text(reply.like_count.as_ref()),
        published: reply.create_time.and_then(beijing_time),
        ip_location: text(reply.ip_location.as_ref()),
        reply_to: reply
            .target_comment
            .as_ref()
            .map(|target| target.id.trim())
            .filter(|id| !id.is_empty())
            .unwrap_or(root)
            .to_owned(),
    }
}

fn text(value: Option<&String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}
