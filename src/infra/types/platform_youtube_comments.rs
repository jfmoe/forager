//! YouTube comments: the request of the `comments` operation and the comments it lists.

use serde::Serialize;

use super::attempt::ProviderAttempt;
use super::platform::{Platform, PlatformRef};
use super::platform_youtube::YoutubeRef;

/// A request for one page of a video's top-level comments and the replies each thread carries.
#[derive(Clone, Debug)]
pub(crate) struct YoutubeCommentsRequest {
    /// The video; parsing guarantees a video ref.
    pub(crate) video: YoutubeRef,
    /// The most top-level comments to deliver, 1 to 100.
    pub(crate) limit: u16,
    pub(crate) sort: YoutubeCommentSort,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/// The order of a video's top-level comments.
pub(crate) enum YoutubeCommentSort {
    /// The order YouTube ranks as most relevant.
    #[default]
    Relevance,
    /// Newest first; a pinned comment may still come first.
    Time,
}

#[derive(Clone, Debug, Serialize)]
/// The first page of a YouTube video's top-level comments. No cursor exists: reading further
/// pages is out of scope.
pub struct YoutubeCommentsPage {
    pub platform: Platform,
    /// The route that read the comments.
    pub provider: &'static str,
    /// The video the comments belong to.
    pub video: PlatformRef,
    pub comments: Vec<YoutubeComment>,
    /// Whether the video has top-level comments this page does not deliver.
    pub has_more: bool,
    #[serde(rename = "provider_attempts", skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<ProviderAttempt>,
    #[serde(skip)]
    pub diagnostic: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
/// One top-level comment and the replies its thread carries.
pub struct YoutubeComment {
    pub id: String,
    /// The display name, such as `@handle`.
    pub author: Option<String>,
    /// The author's channel.
    pub author_channel_ref: Option<PlatformRef>,
    /// The comment as plain text.
    pub text: String,
    pub likes: Option<u64>,
    pub published: Option<String>,
    /// How many replies the comment has in all.
    pub reply_count: u64,
    /// The replies the thread carries, at most a handful.
    pub replies: Vec<YoutubeReply>,
    /// Whether `replies` holds every reply the comment has.
    pub replies_complete: bool,
}

#[derive(Clone, Debug, Serialize)]
/// One reply under a top-level comment.
pub struct YoutubeReply {
    pub id: String,
    pub author: Option<String>,
    pub author_channel_ref: Option<PlatformRef>,
    pub text: String,
    pub likes: Option<u64>,
    pub published: Option<String>,
}

/// One route's comments before the chain attaches the route and the planning attempts.
pub(crate) struct YoutubeCommentsOutcome {
    pub(crate) comments: Vec<YoutubeComment>,
    pub(crate) has_more: bool,
    pub(crate) attempts: Vec<ProviderAttempt>,
    pub(crate) diagnostic: Option<String>,
}
