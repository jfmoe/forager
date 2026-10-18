//! YouTube comments: one `commentThreads.list` page of a video's top-level comments and the
//! replies each thread carries.

use serde::Deserialize;

use super::{YoutubeApi, channel_ref, decode, failed_with_reason, video_not_found};
use crate::net::AttemptFailure;
use crate::providers::shared::{Refusal, utc_timestamp};
use crate::types::{
    AttemptErrorKind, COMMENTS, ProviderError, YoutubeComment, YoutubeCommentSort,
    YoutubeCommentsOutcome, YoutubeCommentsRequest, YoutubeRef, YoutubeReply,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadListResponse {
    #[serde(default)]
    items: Vec<Thread>,
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
struct Thread {
    snippet: ThreadSnippet,
    replies: Option<Replies>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadSnippet {
    video_id: Option<String>,
    top_level_comment: Comment,
    #[serde(default)]
    total_reply_count: u64,
}

#[derive(Deserialize)]
struct Replies {
    #[serde(default)]
    comments: Vec<Comment>,
}

#[derive(Deserialize)]
struct Comment {
    id: String,
    snippet: CommentSnippet,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommentSnippet {
    text_display: Option<String>,
    author_display_name: Option<String>,
    author_channel_id: Option<AuthorChannel>,
    like_count: Option<u64>,
    published_at: Option<String>,
}

#[derive(Deserialize)]
struct AuthorChannel {
    value: Option<String>,
}

impl YoutubeApi {
    /// Reads one page of a video's top-level comments as plain text. A missing video, or one
    /// whose comments are disabled, fails as Parameter, never as an empty list.
    pub(crate) async fn comments(
        &self,
        request: &YoutubeCommentsRequest,
    ) -> Result<YoutubeCommentsOutcome, ProviderError> {
        let video = &request.video;
        let order = match request.sort {
            YoutubeCommentSort::Relevance => "relevance",
            YoutubeCommentSort::Time => "time",
        };
        let query = [
            ("part", "snippet,replies".to_owned()),
            ("videoId", video.id().to_owned()),
            ("textFormat", "plainText".to_owned()),
            ("maxResults", request.limit.to_string()),
            ("order", order.to_owned()),
        ];
        let outcome = self
            .get_with(
                "commentThreads",
                &query,
                COMMENTS,
                |body| {
                    let response = decode::<ThreadListResponse>(body)?;
                    let comments = response
                        .items
                        .into_iter()
                        .map(|thread| top_level_comment(video, thread))
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok((comments, response.next_page_token.is_some()))
                },
                |failure| name_the_video(video, failure),
            )
            .await?;
        let (comments, has_more) = outcome.value;
        Ok(YoutubeCommentsOutcome {
            comments,
            has_more,
            attempts: outcome.attempts,
            diagnostic: outcome.diagnostic,
        })
    }
}

/// Names the video in the refusals that are about it: a missing video (`videoNotFound`, a 404)
/// and disabled comments (the one reason that marks a 403 as Parameter, `commentsDisabled`).
fn name_the_video(video: &YoutubeRef, failure: AttemptFailure) -> AttemptFailure {
    if failure.kind != AttemptErrorKind::Parameter {
        return failure;
    }
    let message = if failed_with_reason(&failure, "videoNotFound") {
        video_not_found(video)
    } else if failure.status == Some(403) {
        format!("comments are disabled for youtube:{video}")
    } else {
        return failure;
    };
    AttemptFailure { message, ..failure }
}

fn top_level_comment(video: &YoutubeRef, thread: Thread) -> Result<YoutubeComment, Refusal> {
    let snippet = thread.snippet;
    if let Some(answered) = snippet.video_id.as_deref()
        && answered != video.id()
    {
        return Err(Refusal::runtime(format!(
            "YouTube Data API answered the comments of youtube:{video} with video `{answered}`"
        )));
    }
    let replies = thread
        .replies
        .map(|replies| replies.comments.into_iter().map(reply).collect::<Vec<_>>())
        .unwrap_or_default();
    let top = reply(snippet.top_level_comment);
    let delivered = u64::try_from(replies.len()).unwrap_or(u64::MAX);
    Ok(YoutubeComment {
        id: top.id,
        author: top.author,
        author_channel_ref: top.author_channel_ref,
        text: top.text,
        likes: top.likes,
        published: top.published,
        reply_count: snippet.total_reply_count,
        replies_complete: delivered >= snippet.total_reply_count,
        replies,
    })
}

fn reply(comment: Comment) -> YoutubeReply {
    let snippet = comment.snippet;
    YoutubeReply {
        id: comment.id,
        author: snippet.author_display_name,
        author_channel_ref: channel_ref(
            snippet
                .author_channel_id
                .and_then(|channel| channel.value)
                .as_deref(),
        ),
        text: snippet.text_display.unwrap_or_default(),
        likes: snippet.like_count,
        published: snippet.published_at.as_deref().and_then(utc_timestamp),
    }
}
