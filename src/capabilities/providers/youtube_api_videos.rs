//! YouTube videos: one `playlistItems.list` page of a playlist, or of a channel's uploads
//! playlist, which `channels.list` names first.

use serde::Deserialize;

use super::{Refusal, YoutubeApi, channel_ref, decode, utc_timestamp};
use crate::net::{AttemptFailure, combine_diagnostics};
use crate::providers::execution::ExecutionOutcome;
use crate::types::{
    AttemptErrorKind, ContentDepth, PlatformItem, PlatformItemData, PlatformRef, ProviderAttempt,
    ProviderError, VIDEOS, YoutubeCollection, YoutubeItemData, YoutubeKind, YoutubeRef,
    YoutubeVideosOutcome, YoutubeVideosRequest,
};

/// The titles YouTube gives the entries of videos nobody may read any longer.
const UNREADABLE_TITLES: [&str; 2] = ["Private video", "Deleted video"];
/// The longest description excerpt a listed video carries, in characters.
const EXCERPT_CHARS: usize = 160;

#[derive(Deserialize)]
struct ChannelListResponse {
    /// The API leaves the list out when nothing matches.
    #[serde(default)]
    items: Vec<Channel>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Channel {
    id: String,
    content_details: Option<ChannelDetails>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChannelDetails {
    related_playlists: Option<RelatedPlaylists>,
}

#[derive(Deserialize)]
struct RelatedPlaylists {
    uploads: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlaylistItemListResponse {
    #[serde(default)]
    items: Vec<PlaylistItem>,
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlaylistItem {
    snippet: Option<PlaylistItemSnippet>,
    content_details: Option<PlaylistItemDetails>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlaylistItemSnippet {
    title: Option<String>,
    description: Option<String>,
    position: Option<u64>,
    video_owner_channel_title: Option<String>,
    video_owner_channel_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlaylistItemDetails {
    video_id: Option<String>,
    video_published_at: Option<String>,
}

/// The videos of one page and the positions of the entries skipped as unreadable.
struct ListedPage {
    items: Vec<PlatformItem>,
    unreadable: Vec<u64>,
    next_page: Option<String>,
}

impl YoutubeApi {
    /// Lists one page of a collection's videos in the collection's order. A channel lists its
    /// uploads, newest first; a handle resolves to its channel first. Entries of private or
    /// deleted videos are skipped and summarized in the diagnostic.
    pub(crate) async fn videos(
        &self,
        request: &YoutubeVideosRequest,
    ) -> Result<YoutubeVideosOutcome, ProviderError> {
        let mut attempts = Vec::new();
        let mut notes = Vec::new();
        let (collection, playlist) = match &request.collection {
            YoutubeCollection::Ref(playlist) if playlist.kind() == YoutubeKind::Playlist => {
                (playlist.clone(), playlist.id().to_owned())
            }
            YoutubeCollection::Ref(channel) => {
                let lookup = self.channel_uploads(ChannelLookup::Id(channel)).await;
                carry(&mut attempts, &mut notes, lookup)?
            }
            YoutubeCollection::Handle(handle) => {
                let lookup = self.channel_uploads(ChannelLookup::Handle(handle)).await;
                carry(&mut attempts, &mut notes, lookup)?
            }
        };
        let mut query = vec![
            ("part", "snippet,contentDetails".to_owned()),
            ("playlistId", playlist),
            ("maxResults", request.limit.to_string()),
        ];
        if let Some(page) = &request.page {
            query.push(("pageToken", page.clone()));
        }
        let page = self
            .get_with("playlistItems", &query, VIDEOS, listed_page, |failure| {
                playlist_not_found(&collection, failure)
            })
            .await;
        let page = carry(&mut attempts, &mut notes, page)?;
        if !page.unreadable.is_empty() {
            notes.push(unreadable_note(&page.unreadable));
        }
        Ok(YoutubeVideosOutcome {
            collection,
            items: page.items,
            next_page: page.next_page,
            attempts,
            diagnostic: combine_diagnostics(notes),
        })
    }

    /// Reads a channel's ID and uploads playlist with `channels.list`.
    async fn channel_uploads(
        &self,
        lookup: ChannelLookup<'_>,
    ) -> Result<ExecutionOutcome<(YoutubeRef, String)>, ProviderError> {
        let query = [
            ("part", "contentDetails".to_owned()),
            match lookup {
                ChannelLookup::Id(channel) => ("id", channel.id().to_owned()),
                ChannelLookup::Handle(handle) => ("forHandle", handle.to_owned()),
            },
        ];
        self.get("channels", &query, VIDEOS, |body| {
            let response = decode::<ChannelListResponse>(body)?;
            resolved_channel(&lookup, response.items)
        })
        .await
    }
}

/// How `channels.list` names the channel to read.
enum ChannelLookup<'a> {
    Id(&'a YoutubeRef),
    /// A handle with its leading `@`.
    Handle(&'a str),
}

impl ChannelLookup<'_> {
    fn describe(&self) -> String {
        match self {
            Self::Id(channel) => format!("youtube:{channel}"),
            Self::Handle(handle) => format!("handle {handle}"),
        }
    }
}

/// Keeps a step's attempts and diagnostic; a failed step carries every attempt so far.
fn carry<T>(
    attempts: &mut Vec<ProviderAttempt>,
    notes: &mut Vec<String>,
    result: Result<ExecutionOutcome<T>, ProviderError>,
) -> Result<T, ProviderError> {
    match result {
        Ok(outcome) => {
            attempts.extend(outcome.attempts);
            notes.extend(outcome.diagnostic);
            Ok(outcome.value)
        }
        Err(mut error) => {
            let mut all = std::mem::take(attempts);
            all.append(&mut error.attempts);
            error.attempts = all;
            Err(error)
        }
    }
}

/// Returns the channel and its uploads playlist. A channel or handle the API does not know
/// lists nothing; a lookup by ID that answers another channel is a broken response.
fn resolved_channel(
    lookup: &ChannelLookup<'_>,
    channels: Vec<Channel>,
) -> Result<(YoutubeRef, String), Refusal> {
    let mut channels = channels.into_iter();
    let Some(channel) = channels.next() else {
        return Err(Refusal {
            kind: AttemptErrorKind::Parameter,
            message: format!("YouTube channel not found: {}", lookup.describe()),
        });
    };
    let answered_other = match lookup {
        ChannelLookup::Id(requested) => channel.id != requested.id(),
        ChannelLookup::Handle(_) => false,
    };
    let reference = YoutubeRef::channel(&channel.id)
        .filter(|_| !answered_other && channels.next().is_none())
        .ok_or_else(|| {
            Refusal::runtime(format!(
                "YouTube Data API answered {} with channel `{}`",
                lookup.describe(),
                channel.id
            ))
        })?;
    let uploads = channel
        .content_details
        .and_then(|details| details.related_playlists)
        .and_then(|playlists| playlists.uploads)
        .filter(|uploads| YoutubeRef::playlist(uploads).is_some())
        .ok_or_else(|| {
            Refusal::runtime(format!(
                "YouTube Data API returned youtube:{reference} without its uploads playlist"
            ))
        })?;
    Ok((reference, uploads))
}

/// Names the collection in the 404 the API answers for a playlist it does not know.
fn playlist_not_found(collection: &YoutubeRef, failure: AttemptFailure) -> AttemptFailure {
    if failure.status == Some(404) {
        AttemptFailure {
            message: format!("YouTube playlist not found or not public: youtube:{collection}"),
            ..failure
        }
    } else {
        failure
    }
}

fn listed_page(body: &str) -> Result<ListedPage, Refusal> {
    let response = decode::<PlaylistItemListResponse>(body)?;
    let mut page = ListedPage {
        items: Vec::new(),
        unreadable: Vec::new(),
        next_page: response.next_page_token,
    };
    for entry in response.items {
        match listed_video(entry)? {
            Listed::Video(item) => page.items.push(*item),
            Listed::Unreadable(position) => page.unreadable.push(position),
        }
    }
    Ok(page)
}

enum Listed {
    Video(Box<PlatformItem>),
    Unreadable(u64),
}

/// Builds a listed video: its owner, not the playlist's, is its channel, and its publication,
/// not when it joined the playlist, is its time. Titles in playlists come unescaped.
fn listed_video(entry: PlaylistItem) -> Result<Listed, Refusal> {
    let snippet = entry.snippet.ok_or_else(|| {
        Refusal::runtime("YouTube Data API returned a playlist entry without a snippet".into())
    })?;
    let position = snippet.position.ok_or_else(|| {
        Refusal::runtime("YouTube Data API returned a playlist entry without a position".into())
    })?;
    if snippet
        .title
        .as_deref()
        .is_some_and(|title| UNREADABLE_TITLES.contains(&title))
    {
        return Ok(Listed::Unreadable(position));
    }
    let details = entry.content_details;
    let id = details
        .as_ref()
        .and_then(|details| details.video_id.as_deref())
        .unwrap_or_default();
    let reference = YoutubeRef::video(id).ok_or_else(|| {
        Refusal::runtime(format!("YouTube Data API returned invalid video ID `{id}`"))
    })?;
    let excerpt = snippet.description.as_deref().and_then(excerpt);
    Ok(Listed::Video(Box::new(PlatformItem {
        url: reference.canonical_url(),
        depth: if excerpt.is_some() {
            ContentDepth::Snippet
        } else {
            ContentDepth::Metadata
        },
        reference: PlatformRef::Youtube(reference),
        title: snippet.title,
        authors: snippet.video_owner_channel_title.into_iter().collect(),
        published: details
            .and_then(|details| details.video_published_at)
            .as_deref()
            .and_then(utc_timestamp),
        data: PlatformItemData::Youtube(YoutubeItemData {
            channel_ref: channel_ref(snippet.video_owner_channel_id.as_deref()),
            snippet: excerpt,
            duration_seconds: None,
            live_status: None,
            views: None,
            likes: None,
            comments: None,
            language: None,
            has_captions: None,
            caption_tracks: None,
            description: None,
            chapters: None,
            transcript: None,
            position: Some(position),
        }),
    })))
}

/// Cuts a playlist entry's full description to a search-sized excerpt on one line, at a word
/// boundary where one exists.
fn excerpt(description: &str) -> Option<String> {
    let line = description.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.is_empty() {
        return None;
    }
    if line.chars().count() <= EXCERPT_CHARS {
        return Some(line);
    }
    let cut = line
        .char_indices()
        .nth(EXCERPT_CHARS)
        .map_or(line.len(), |(index, _)| index);
    let head = &line[..cut];
    let head = head.rfind(' ').map_or(head, |space| &head[..space]);
    Some(format!("{head}…"))
}

fn unreadable_note(positions: &[u64]) -> String {
    let positions = positions
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    format!("skipped private or deleted videos at playlist positions {positions}")
}

#[cfg(test)]
mod tests {
    use super::{EXCERPT_CHARS, excerpt};

    #[test]
    fn excerpts_keep_short_descriptions_and_cut_long_ones_at_a_word() {
        let long = "word ".repeat(EXCERPT_CHARS);
        let cut = excerpt(&long).expect("excerpt");

        assert_eq!(
            (
                excerpt("  Line one.\n\nLine   two. "),
                excerpt(" \n "),
                cut.chars().count() <= EXCERPT_CHARS + 1,
                cut.ends_with("word…"),
            ),
            (Some("Line one. Line two.".to_owned()), None, true, true)
        );
    }
}
