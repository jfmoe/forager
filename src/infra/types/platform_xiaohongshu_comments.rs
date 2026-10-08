//! Xiaohongshu comments: the request of the `comments` operation and the comments it lists.

use serde::Serialize;

use super::attempt::ProviderAttempt;
use super::platform::{Platform, PlatformRef};
use super::platform_xiaohongshu::{AccessToken, XiaohongshuRef};

/// The attempt-target name of the Xiaohongshu comments operation.
pub(crate) const COMMENTS: &str = "comments";

/// A request for the top-level comments of one note, and the replies of the first few.
#[derive(Clone, Debug)]
pub(crate) struct XiaohongshuCommentsRequest {
    pub(crate) note: XiaohongshuRef,
    /// The access token that opens the note.
    pub(crate) access: AccessToken,
    /// The most top-level comments to deliver, 1 to 50.
    pub(crate) limit: u16,
    /// How many delivered comments with more replies to expand, 0 to 10.
    pub(crate) replies: u16,
}

#[derive(Clone, Debug, Serialize)]
/// The comments of one Xiaohongshu note. No cursor exists: the page loads comments only in
/// order from the first page.
pub struct XiaohongshuCommentsPage {
    pub platform: Platform,
    /// The route that read the comments.
    pub provider: &'static str,
    /// The note the comments belong to.
    pub note: PlatformRef,
    pub comments: Vec<XiaohongshuComment>,
    /// Whether the note has top-level comments this result does not deliver, including those
    /// the limit cut off.
    pub has_more: bool,
    #[serde(rename = "provider_attempts", skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<ProviderAttempt>,
    #[serde(skip)]
    pub diagnostic: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
/// One top-level comment and the replies read with it.
pub struct XiaohongshuComment {
    pub id: String,
    pub author: Option<String>,
    pub author_id: Option<String>,
    /// The text exactly as Xiaohongshu stores it.
    pub text: String,
    /// The like count exactly as Xiaohongshu shows it.
    pub likes: Option<String>,
    /// When the comment was posted, in Beijing time.
    pub published: Option<String>,
    pub ip_location: Option<String>,
    /// The reply count exactly as Xiaohongshu shows it.
    pub reply_count: Option<String>,
    /// The reply the comment carries, then the first page of an expanded comment's replies.
    pub replies: Vec<XiaohongshuReply>,
    /// Whether the comment has replies this result does not deliver.
    pub replies_has_more: bool,
}

#[derive(Clone, Debug, Serialize)]
/// One reply under a top-level comment.
pub struct XiaohongshuReply {
    pub id: String,
    pub author: Option<String>,
    pub author_id: Option<String>,
    pub text: String,
    pub likes: Option<String>,
    pub published: Option<String>,
    pub ip_location: Option<String>,
    /// The comment or reply this reply answers.
    pub reply_to: String,
}

/// One route's comments before the chain attaches the route and the planning attempts.
pub(crate) struct XiaohongshuCommentsOutcome {
    pub(crate) comments: Vec<XiaohongshuComment>,
    pub(crate) has_more: bool,
    pub(crate) attempts: Vec<ProviderAttempt>,
    pub(crate) diagnostic: Option<String>,
}
