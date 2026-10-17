//! YouTube search: one `search.list` page, enriched by one batched `videos.list`.

use std::collections::HashMap;

use serde::Deserialize;

use super::{
    ROUTE, Refusal, Video, VideoListResponse, YoutubeApi, caption_flag, channel_ref, decode,
    duration_seconds,
};
use crate::catalog::PlatformOperation;
use crate::net::combine_diagnostics;
use crate::providers::markup::decode_entities;
use crate::providers::shared::{count, other_platform_message, parameter_error, utc_timestamp};
use crate::types::{
    ContentDepth, PlatformItem, PlatformItemData, PlatformRef, PlatformSearchOptions,
    PlatformSearchOutcome, PlatformSearchRequest, ProviderError, YoutubeDuration, YoutubeItemData,
    YoutubeLiveStatus, YoutubeRef, YoutubeSearchOptions, YoutubeSort,
};

const ENRICHMENT_PARTS: &str = "contentDetails,statistics";

/// Returns whether the route can run the search; it never sends a request.
pub(crate) fn search_support(request: &PlatformSearchRequest) -> Result<(), String> {
    if !matches!(request.options, PlatformSearchOptions::Youtube(_)) {
        return Err(other_platform_message(ROUTE, request.options.platform()));
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchResponse {
    items: Vec<SearchResult>,
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
struct SearchResult {
    id: SearchId,
    snippet: Option<SearchSnippet>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchId {
    video_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchSnippet {
    published_at: Option<String>,
    channel_id: Option<String>,
    title: Option<String>,
    description: Option<String>,
    channel_title: Option<String>,
    live_broadcast_content: Option<String>,
}

impl YoutubeApi {
    /// Searches one page of videos, then reads their durations, counts, and caption flags in
    /// one batch. A failed batch keeps the results and says what is missing.
    pub(crate) async fn search(
        &self,
        request: &PlatformSearchRequest,
    ) -> Result<PlatformSearchOutcome, ProviderError> {
        let PlatformSearchOptions::Youtube(options) = &request.options else {
            return Err(parameter_error(other_platform_message(
                ROUTE,
                request.options.platform(),
            )));
        };
        let operation = PlatformOperation::Search.as_str();
        let found = self
            .get(
                "search",
                &search_parameters(request, options),
                operation,
                |body| {
                    let response = decode::<SearchResponse>(body)?;
                    let items = response
                        .items
                        .into_iter()
                        .map(listed_item)
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok((items, response.next_page_token))
                },
            )
            .await?;
        let (mut items, next_page) = found.value;
        let mut attempts = found.attempts;
        let mut notes = found.diagnostic.into_iter().collect::<Vec<_>>();
        if !items.is_empty() {
            let ids = items
                .iter()
                .map(|item| video_id(item).to_owned())
                .collect::<Vec<_>>()
                .join(",");
            let query = [("part", ENRICHMENT_PARTS.to_owned()), ("id", ids)];
            match self
                .get("videos", &query, operation, |body| {
                    decode::<VideoListResponse>(body).map(|response| response.items)
                })
                .await
            {
                Ok(details) => {
                    attempts.extend(details.attempts);
                    notes.extend(details.diagnostic);
                    enrich(&mut items, details.value);
                }
                Err(mut error) => {
                    attempts.append(&mut error.attempts);
                    notes.extend(error.diagnostic.take());
                    notes.push(format!(
                        "YouTube video details unavailable ({}); duration, counts, and captions are missing",
                        error.message
                    ));
                }
            }
        }
        Ok(PlatformSearchOutcome {
            items,
            next_page,
            attempts,
            diagnostic: combine_diagnostics(notes),
        })
    }
}

/// Maps the request to `search.list` parameters; an option not given sends nothing.
fn search_parameters(
    request: &PlatformSearchRequest,
    options: &YoutubeSearchOptions,
) -> Vec<(&'static str, String)> {
    let mut query = vec![
        ("part", "snippet".to_owned()),
        ("type", "video".to_owned()),
        ("q", request.query.clone()),
        ("maxResults", request.limit.to_string()),
    ];
    let order = match options.sort {
        YoutubeSort::Relevance => None,
        YoutubeSort::Date => Some("date"),
        YoutubeSort::Views => Some("viewCount"),
        YoutubeSort::Rating => Some("rating"),
    };
    if let Some(order) = order {
        query.push(("order", order.to_owned()));
    }
    if let Some(from) = options.from {
        query.push(("publishedAfter", format!("{from}T00:00:00Z")));
    }
    if let Some(to) = options.to {
        query.push(("publishedBefore", format!("{to}T23:59:59Z")));
    }
    if let Some(duration) = options.duration {
        let duration = match duration {
            YoutubeDuration::Short => "short",
            YoutubeDuration::Medium => "medium",
            YoutubeDuration::Long => "long",
        };
        query.push(("videoDuration", duration.to_owned()));
    }
    if let Some(channel) = &options.channel {
        query.push(("channelId", channel.clone()));
    }
    if options.has_captions {
        query.push(("videoCaption", "closedCaption".to_owned()));
    }
    if let Some(page) = &request.page {
        query.push(("pageToken", page.clone()));
    }
    query
}

/// Builds a listed video: titles and excerpts come HTML-escaped and are decoded here. A video
/// with an excerpt is a snippet, one without is metadata.
fn listed_item(result: SearchResult) -> Result<PlatformItem, Refusal> {
    let id = result.id.video_id.unwrap_or_default();
    let reference = YoutubeRef::video(&id).ok_or_else(|| {
        Refusal::runtime(format!("YouTube Data API returned invalid video ID `{id}`"))
    })?;
    let snippet = result.snippet.ok_or_else(|| {
        Refusal::runtime(format!(
            "YouTube Data API returned youtube:{reference} without a snippet"
        ))
    })?;
    let excerpt = snippet
        .description
        .map(|description| decode_entities(description.trim()))
        .filter(|description| !description.is_empty());
    let live_status = match snippet.live_broadcast_content.as_deref() {
        Some("none") => Some(YoutubeLiveStatus::None),
        Some("upcoming") => Some(YoutubeLiveStatus::Upcoming),
        Some("live") => Some(YoutubeLiveStatus::Live),
        _ => None,
    };
    Ok(PlatformItem {
        url: reference.canonical_url(),
        depth: if excerpt.is_some() {
            ContentDepth::Snippet
        } else {
            ContentDepth::Metadata
        },
        reference: PlatformRef::Youtube(reference),
        title: snippet.title.as_deref().map(decode_entities),
        authors: snippet
            .channel_title
            .as_deref()
            .map(decode_entities)
            .into_iter()
            .collect(),
        published: snippet.published_at.as_deref().and_then(utc_timestamp),
        data: PlatformItemData::Youtube(YoutubeItemData {
            channel_ref: channel_ref(snippet.channel_id.as_deref()),
            snippet: excerpt,
            duration_seconds: None,
            live_status,
            views: None,
            likes: None,
            comments: None,
            language: None,
            has_captions: None,
            caption_tracks: None,
            description: None,
            chapters: None,
            transcript: None,
            position: None,
        }),
    })
}

fn video_id(item: &PlatformItem) -> &str {
    match &item.reference {
        PlatformRef::Youtube(reference) => reference.id(),
        _ => unreachable!("the youtube route lists youtube videos"),
    }
}

/// Fills each listed video's duration, counts, and caption flag from its `videos` resource.
fn enrich(items: &mut [PlatformItem], videos: Vec<Video>) {
    let mut videos = videos
        .into_iter()
        .map(|video| (video.id.clone(), video))
        .collect::<HashMap<_, _>>();
    for item in items {
        let Some(video) = videos.remove(video_id(item)) else {
            continue;
        };
        let PlatformItemData::Youtube(data) = &mut item.data else {
            continue;
        };
        if let Some(details) = video.content_details {
            data.duration_seconds = details.duration.as_deref().and_then(duration_seconds);
            data.has_captions = caption_flag(details.caption.as_deref());
        }
        if let Some(statistics) = video.statistics {
            data.views = count(statistics.views.as_deref());
            data.likes = count(statistics.likes.as_deref());
            data.comments = count(statistics.comments.as_deref());
        }
    }
}
