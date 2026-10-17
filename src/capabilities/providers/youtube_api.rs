//! The `youtube_api` route: the official YouTube Data API v3 with the user's own key.
//!
//! The key travels only in the `X-Goog-Api-Key` header, so no request URL carries it. Every
//! request follows the route's access policy and costs quota even when it fails. Errors are
//! attributed by the reason the API names, not by the status alone: an invalid key arrives as a
//! 400 and an exhausted quota as a 403.

use std::time::Duration;

use chrono::DateTime;
use reqwest::Client;
use serde::Deserialize;

use crate::catalog::{PlatformOperation, ProviderId, YOUTUBE_WEB_OPT_IN};
use crate::config::KeyedHttpRouteRuntimeConfig;
use crate::credentials::CredentialPool;
use crate::net::{
    AttemptFailure, RetryPolicy, read_complete_protocol_with_reasons, send_provider_request,
};
use crate::providers::execution::{ExecutionOutcome, ExecutionSettings, execute_v2};
use crate::providers::shared::{acquire_window, other_platform_message, parameter_error};
use crate::rate_limit::RateLimiter;
use crate::types::{
    AttemptErrorKind, AttemptTarget, ContentDepth, Deadline, FullTextSource, Platform,
    PlatformFetchOutcome, PlatformFetchRequest, PlatformItem, PlatformItemData, PlatformRef,
    ProviderError, YoutubeItemData, YoutubeKind, YoutubeLiveStatus, YoutubeRef,
    chapters_from_description,
};

#[path = "youtube_api_search.rs"]
mod search;

pub(crate) use search::search_support;

const ROUTE: ProviderId = ProviderId::YoutubeApi;
const PLATFORM: Platform = Platform::Youtube;
const KEY_HEADER: &str = "x-goog-api-key";
const FETCH_PARTS: &str = "snippet,contentDetails,statistics,status,liveStreamingDetails";

/// Returns whether the route can fetch the request; it never sends a request. The Data API
/// carries no captions this route may read, so it serves metadata and abstract only; refusing
/// the full text, it says how to enable the route that reads captions.
pub(crate) fn fetch_support(request: &PlatformFetchRequest) -> Result<(), String> {
    let PlatformRef::Youtube(reference) = &request.reference else {
        return Err(other_platform_message(ROUTE, request.reference.platform()));
    };
    if reference.kind() != YoutubeKind::Video {
        return Err(format!(
            "{} fetches videos, not a {}",
            ROUTE.name(),
            reference.kind().as_str()
        ));
    }
    match request.depth {
        ContentDepth::Metadata | ContentDepth::Abstract => Ok(()),
        ContentDepth::FullText => Err(format!(
            "{} cannot fetch at depth `full_text`: the Data API does not serve captions. {YOUTUBE_WEB_OPT_IN}",
            ROUTE.name()
        )),
        depth => Err(format!(
            "{} cannot fetch at depth `{}`; it reads metadata and the description",
            ROUTE.name(),
            depth.as_str()
        )),
    }
}

pub(crate) struct YoutubeApi {
    config: KeyedHttpRouteRuntimeConfig,
    client: Client,
    credentials: CredentialPool,
    limiter: RateLimiter,
    retry_policy: RetryPolicy,
    deadline: Deadline,
}

impl YoutubeApi {
    pub(crate) fn new(
        config: KeyedHttpRouteRuntimeConfig,
        client: Client,
        credentials: CredentialPool,
        limiter: RateLimiter,
        retry_policy: RetryPolicy,
        deadline: Deadline,
    ) -> Self {
        Self {
            config,
            client,
            credentials,
            limiter,
            retry_policy,
            deadline,
        }
    }

    /// Reads one video with `videos.list`. A video the API does not return is missing or not
    /// public; at `abstract` depth, a video without a description fails as Quality.
    pub(crate) async fn fetch(
        &self,
        request: &PlatformFetchRequest,
    ) -> Result<PlatformFetchOutcome, ProviderError> {
        fetch_support(request).map_err(parameter_error)?;
        let PlatformRef::Youtube(reference) = &request.reference else {
            unreachable!("the support check accepts only YouTube refs")
        };
        let depth = request.depth;
        let query = [
            ("part", FETCH_PARTS.to_owned()),
            ("id", reference.id().to_owned()),
        ];
        let outcome = self
            .get(
                "videos",
                &query,
                PlatformOperation::Fetch.as_str(),
                |body| {
                    let response = decode::<VideoListResponse>(body)?;
                    fetched_item(reference, depth, response.items)
                },
            )
            .await?;
        Ok(PlatformFetchOutcome {
            item: outcome.value,
            content_source: FullTextSource::Urls(Vec::new()),
            attempts: outcome.attempts,
            diagnostic: outcome.diagnostic,
        })
    }

    /// Sends one keyed GET to a Data API resource under the route's access policy; `decode`
    /// turns the body into the operation's value inside the attempt, so its refusal is that
    /// attempt's failure.
    async fn get<T>(
        &self,
        resource: &str,
        query: &[(&str, String)],
        operation: &'static str,
        decode: impl Fn(&str) -> Result<T, Refusal>,
    ) -> Result<ExecutionOutcome<T>, ProviderError> {
        let url = &format!("{}/{resource}", self.config.url.trim_end_matches('/'));
        let decode = &decode;
        execute_v2(
            &self.credentials,
            self.settings(operation),
            |credential, deadline| async move {
                let _permit = acquire_window(&self.limiter, deadline).await?;
                let request = self
                    .client
                    .get(url)
                    .query(query)
                    .header(KEY_HEADER, credential.expose());
                let response = send_provider_request(request, &self.credentials).await?;
                let body = read_complete_protocol_with_reasons(
                    response,
                    &self.credentials,
                    failure_message,
                    reason_kind,
                )
                .await?;
                let value = decode(&body.text).map_err(|refusal| AttemptFailure {
                    kind: refusal.kind,
                    status: Some(body.status),
                    message: refusal.message,
                })?;
                Ok((Some(body.status), value))
            },
        )
        .await
    }

    fn settings(&self, operation: &'static str) -> ExecutionSettings {
        ExecutionSettings {
            provider: ROUTE.name(),
            target: AttemptTarget::platform(PLATFORM.as_str(), operation),
            retry_policy: self.retry_policy,
            deadline: self.deadline,
            attempt_timeout: Duration::from_secs(self.config.timeout_seconds),
            verbose: false,
            timeout_message: "YouTube Data API request timed out",
            model: None,
            transport: Some("http"),
            endpoint_host: None,
            breaker_event: None,
        }
    }
}

/// Why a successful response cannot become the operation's value.
struct Refusal {
    kind: AttemptErrorKind,
    message: String,
}

impl Refusal {
    fn runtime(message: String) -> Self {
        Self {
            kind: AttemptErrorKind::Runtime,
            message,
        }
    }
}

fn decode<T: serde::de::DeserializeOwned>(body: &str) -> Result<T, Refusal> {
    serde_json::from_str(body)
        .map_err(|error| Refusal::runtime(format!("invalid YouTube Data API response: {error}")))
}

#[derive(Deserialize)]
struct VideoListResponse {
    items: Vec<Video>,
}

/// One `videos` resource; every part is optional because each request names its own parts.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Video {
    id: String,
    snippet: Option<VideoSnippet>,
    content_details: Option<ContentDetails>,
    statistics: Option<Statistics>,
    live_streaming_details: Option<LiveStreamingDetails>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VideoSnippet {
    published_at: Option<String>,
    channel_id: Option<String>,
    title: Option<String>,
    description: Option<String>,
    channel_title: Option<String>,
    live_broadcast_content: Option<String>,
    default_language: Option<String>,
    default_audio_language: Option<String>,
}

#[derive(Deserialize)]
struct ContentDetails {
    duration: Option<String>,
    /// `"true"` or `"false"`: the API sends a string, not a boolean.
    caption: Option<String>,
}

#[derive(Deserialize)]
struct Statistics {
    #[serde(rename = "viewCount")]
    views: Option<String>,
    #[serde(rename = "likeCount")]
    likes: Option<String>,
    #[serde(rename = "commentCount")]
    comments: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LiveStreamingDetails {
    actual_start_time: Option<String>,
    actual_end_time: Option<String>,
}

/// Builds the fetched item from the one video the API returned for the request.
fn fetched_item(
    reference: &YoutubeRef,
    depth: ContentDepth,
    items: Vec<Video>,
) -> Result<PlatformItem, Refusal> {
    let mut items = items.into_iter();
    let Some(video) = items.next() else {
        return Err(Refusal {
            kind: AttemptErrorKind::Parameter,
            message: format!("YouTube video not found or not public: youtube:{reference}"),
        });
    };
    if video.id != reference.id() || items.next().is_some() {
        return Err(Refusal::runtime(format!(
            "YouTube Data API answered youtube:{reference} with video `{}`",
            video.id
        )));
    }
    let snippet = video.snippet.ok_or_else(|| {
        Refusal::runtime(format!(
            "YouTube Data API returned youtube:{reference} without a snippet"
        ))
    })?;
    let live_status = live_status(
        snippet.live_broadcast_content.as_deref(),
        video.live_streaming_details.as_ref(),
    );
    let description = snippet.description.unwrap_or_default();
    let (description, chapters) = if depth == ContentDepth::Abstract {
        if description.trim().is_empty() {
            return Err(Refusal {
                kind: AttemptErrorKind::Quality,
                message: format!("youtube:{reference} has no description"),
            });
        }
        let chapters = chapters_from_description(&description);
        (Some(description), Some(chapters))
    } else {
        (None, None)
    };
    let details = video.content_details;
    let statistics = video.statistics;
    Ok(PlatformItem {
        url: reference.canonical_url(),
        reference: PlatformRef::Youtube(reference.clone()),
        depth,
        title: snippet.title,
        authors: snippet.channel_title.into_iter().collect(),
        published: snippet.published_at.as_deref().and_then(utc_timestamp),
        data: PlatformItemData::Youtube(YoutubeItemData {
            channel_ref: channel_ref(snippet.channel_id.as_deref()),
            snippet: None,
            duration_seconds: details
                .as_ref()
                .and_then(|details| details.duration.as_deref())
                .and_then(duration_seconds),
            live_status: Some(live_status),
            views: count(statistics.as_ref().and_then(|s| s.views.as_deref())),
            likes: count(statistics.as_ref().and_then(|s| s.likes.as_deref())),
            comments: count(statistics.as_ref().and_then(|s| s.comments.as_deref())),
            language: snippet.default_audio_language.or(snippet.default_language),
            has_captions: details.and_then(|details| caption_flag(details.caption.as_deref())),
            caption_tracks: None,
            description,
            chapters,
            transcript: None,
        }),
    })
}

/// Derives the live status: a stream's details decide it when present, since a finished
/// stream's `liveBroadcastContent` reads `none` like an ordinary upload.
fn live_status(
    broadcast: Option<&str>,
    stream: Option<&LiveStreamingDetails>,
) -> YoutubeLiveStatus {
    match (stream, broadcast) {
        (Some(stream), _) if stream.actual_end_time.is_some() => YoutubeLiveStatus::Completed,
        (Some(stream), _) if stream.actual_start_time.is_some() => YoutubeLiveStatus::Live,
        (Some(_), _) | (None, Some("upcoming")) => YoutubeLiveStatus::Upcoming,
        (None, Some("live")) => YoutubeLiveStatus::Live,
        (None, _) => YoutubeLiveStatus::None,
    }
}

fn channel_ref(channel_id: Option<&str>) -> Option<PlatformRef> {
    channel_id
        .and_then(YoutubeRef::channel)
        .map(PlatformRef::Youtube)
}

fn count(value: Option<&str>) -> Option<u64> {
    value.and_then(|value| value.parse().ok())
}

fn caption_flag(value: Option<&str>) -> Option<bool> {
    match value? {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Normalizes an RFC 3339 time to a UTC ISO 8601 timestamp to the second.
fn utc_timestamp(value: &str) -> Option<String> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|time| time.to_utc().format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

/// Reads an ISO 8601 duration such as `PT21M3S` or `P1DT2H` as seconds. A zero duration, which
/// the API reports for live and upcoming streams, is unknown.
fn duration_seconds(value: &str) -> Option<u64> {
    let rest = value.strip_prefix('P')?;
    let (date, time) = rest.split_once('T').unwrap_or((rest, ""));
    let date_seconds = sum_units(date, &[('W', 604_800), ('D', 86_400)])?;
    let time_seconds = sum_units(time, &[('H', 3_600), ('M', 60), ('S', 1)])?;
    let seconds = date_seconds.checked_add(time_seconds)?;
    (seconds > 0).then_some(seconds)
}

/// Sums `<number><unit>` pairs whose units appear in the given order, each at most once.
fn sum_units(mut text: &str, units: &[(char, u64)]) -> Option<u64> {
    let mut total = 0_u64;
    let mut allowed = units;
    while !text.is_empty() {
        let digits = text.find(|character: char| !character.is_ascii_digit())?;
        if digits == 0 {
            return None;
        }
        let number = text[..digits].parse::<u64>().ok()?;
        let unit = text[digits..].chars().next()?;
        let position = allowed.iter().position(|(known, _)| *known == unit)?;
        total = total.checked_add(number.checked_mul(allowed[position].1)?)?;
        allowed = &allowed[position + 1..];
        text = &text[digits + unit.len_utf8()..];
    }
    Some(total)
}

#[derive(Deserialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Deserialize)]
struct ErrorBody {
    #[serde(default)]
    message: String,
    #[serde(default)]
    errors: Vec<ErrorReason>,
    #[serde(default)]
    details: Vec<ErrorReason>,
}

/// An entry of `errors[]`, or a `details[]` entry such as `google.rpc.ErrorInfo`.
#[derive(Deserialize)]
struct ErrorReason {
    reason: Option<String>,
}

fn error_body(body: &str) -> Option<ErrorBody> {
    serde_json::from_str::<ErrorEnvelope>(body)
        .ok()
        .map(|envelope| envelope.error)
}

/// Names the error kind of a reason the status alone cannot tell: an invalid key comes as a
/// 400 whose reason sits only in `ErrorInfo`, and an exhausted quota as a 403.
fn reason_kind(body: &str) -> Option<AttemptErrorKind> {
    let body = error_body(body)?;
    let reasons = body
        .errors
        .iter()
        .chain(&body.details)
        .filter_map(|entry| entry.reason.as_deref())
        .collect::<Vec<_>>();
    if reasons.contains(&"API_KEY_INVALID") {
        Some(AttemptErrorKind::Auth)
    } else if reasons
        .iter()
        .any(|reason| matches!(*reason, "quotaExceeded" | "dailyLimitExceeded"))
    {
        Some(AttemptErrorKind::QuotaExhausted)
    } else {
        None
    }
}

fn failure_message(body: &str, status: u16) -> String {
    match error_body(body) {
        Some(error) if !error.message.is_empty() => {
            let reason = error
                .errors
                .iter()
                .find_map(|entry| entry.reason.as_deref())
                .map(|reason| format!(" ({reason})"))
                .unwrap_or_default();
            format!(
                "YouTube Data API returned HTTP {status}{reason}: {}",
                error.message
            )
        }
        _ => format!("YouTube Data API returned HTTP {status}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{LiveStreamingDetails, duration_seconds, live_status, reason_kind};
    use crate::types::{AttemptErrorKind, YoutubeLiveStatus};

    #[test]
    fn iso_durations_read_as_seconds_and_zero_is_unknown() {
        let read = [
            "PT19S", "PT21M3S", "PT1H2M3S", "P1DT2H", "PT0S", "P0D", "PT", "PT3S4M", "21M",
        ]
        .map(duration_seconds);

        assert_eq!(
            read,
            [
                Some(19),
                Some(1263),
                Some(3723),
                Some(93_600),
                None,
                None,
                None,
                None,
                None
            ]
        );
    }

    #[test]
    fn stream_details_decide_the_live_status_before_the_broadcast_flag() {
        let details = |start: Option<&str>, end: Option<&str>| LiveStreamingDetails {
            actual_start_time: start.map(str::to_owned),
            actual_end_time: end.map(str::to_owned),
        };
        let statuses = [
            live_status(Some("none"), None),
            live_status(Some("upcoming"), None),
            live_status(Some("live"), None),
            live_status(Some("none"), Some(&details(Some("t"), Some("u")))),
            live_status(Some("live"), Some(&details(Some("t"), None))),
            live_status(Some("upcoming"), Some(&details(None, None))),
        ];

        assert_eq!(
            statuses,
            [
                YoutubeLiveStatus::None,
                YoutubeLiveStatus::Upcoming,
                YoutubeLiveStatus::Live,
                YoutubeLiveStatus::Completed,
                YoutubeLiveStatus::Live,
                YoutubeLiveStatus::Upcoming,
            ]
        );
    }

    #[test]
    fn error_reasons_override_the_status_only_where_the_status_misleads() {
        let body = |reason: &str, info: Option<&str>| {
            let details = info.map_or_else(
                || "[]".to_owned(),
                |info| format!(r#"[{{"@type": "type.googleapis.com/google.rpc.ErrorInfo", "reason": "{info}"}}]"#),
            );
            format!(
                r#"{{"error": {{"code": 400, "message": "m", "errors": [{{"reason": "{reason}"}}], "details": {details}}}}}"#
            )
        };
        let kinds = [
            reason_kind(&body("badRequest", Some("API_KEY_INVALID"))),
            reason_kind(&body("quotaExceeded", None)),
            reason_kind(&body("dailyLimitExceeded", None)),
            reason_kind(&body("invalid", None)),
            reason_kind("not json"),
        ];

        assert_eq!(
            kinds,
            [
                Some(AttemptErrorKind::Auth),
                Some(AttemptErrorKind::QuotaExhausted),
                Some(AttemptErrorKind::QuotaExhausted),
                None,
                None,
            ]
        );
    }
}
