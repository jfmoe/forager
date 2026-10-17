//! YouTube videos: the request of the `videos` operation and one route's page of a collection.

use serde::{Deserialize, Serialize};

use super::attempt::ProviderAttempt;
use super::platform::PlatformItem;
use super::platform_youtube::{YoutubeCollection, YoutubeRef};

/// The attempt-target name of the videos operation.
pub(crate) const VIDEOS: &str = "videos";

const YOUTUBE_VIDEOS_MAX_LIMIT: u16 = 50;

/// A request for one page of the videos a playlist or channel lists; a page cursor encodes it
/// completely.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct YoutubeVideosRequest {
    pub(crate) collection: YoutubeCollection,
    pub(crate) limit: u16,
    /// The route-owned position of the requested page; absent for the first page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) page: Option<String>,
}

impl YoutubeVideosRequest {
    /// Checks the rules that argument parsing cannot express for a cursor.
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !(1..=YOUTUBE_VIDEOS_MAX_LIMIT).contains(&self.limit) {
            return Err(format!(
                "--limit must be between 1 and {YOUTUBE_VIDEOS_MAX_LIMIT}"
            ));
        }
        Ok(())
    }
}

/// One route's page of a collection before cursor encoding.
pub(crate) struct YoutubeVideosOutcome {
    /// The playlist or channel the page lists; a handle is resolved to its channel.
    pub(crate) collection: YoutubeRef,
    pub(crate) items: Vec<PlatformItem>,
    pub(crate) next_page: Option<String>,
    pub(crate) attempts: Vec<ProviderAttempt>,
    pub(crate) diagnostic: Option<String>,
}
