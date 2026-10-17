//! The `youtube_web` route: YouTube's own web page, player, and caption endpoints, read
//! anonymously.
//!
//! These endpoints are undocumented, so the user enables the route (ADR 0028). One fetch sends
//! three requests in a fixed order, each under the route's access policy and none retried: the
//! watch page (the `InnerTube` key and the player response it embeds, which carries the
//! metadata), the player endpoint (playability and caption tracks), and, at `full_text` depth
//! only, the srv1 XML of the chosen track. Every decision falls inside the attempt of the
//! request whose answer it reads. The client and request shape follow youtube-transcript-api;
//! when YouTube breaks the route, its fix is the reference.

use std::time::Duration;

use chrono::DateTime;
use reqwest::header::ACCEPT_LANGUAGE;
use reqwest::{Client, RequestBuilder};
use serde::Deserialize;
use serde_json::json;

use super::transcript::{Transcript, TranscriptChapter, TranscriptSubject};
use crate::catalog::ProviderId;
use crate::config::HttpRouteRuntimeConfig;
use crate::credentials::CredentialPool;
use crate::net::{AttemptFailure, RetryPolicy, read_complete_protocol, send_provider_request};
use crate::providers::execution::{ExecutionOutcome, ExecutionSettings, execute_anonymous};
use crate::providers::shared::{acquire_window, other_platform_message, parameter_error};
use crate::rate_limit::RateLimiter;
use crate::types::{
    AttemptErrorKind, AttemptTarget, ContentDepth, Deadline, FullTextSource, Platform,
    PlatformFetchOptions, PlatformFetchOutcome, PlatformFetchRequest, PlatformItem,
    PlatformItemData, PlatformRef, ProviderAttempt, ProviderError, TranscriptInfo,
    TranscriptSource, YoutubeItemData, YoutubeKind, YoutubeLiveStatus, YoutubeRef,
    chapters_from_description,
};

#[path = "youtube_web_captions.rs"]
mod captions;

use captions::{Srv1Error, TrackList, srv1_cues, srv1_url};

const ROUTE: ProviderId = ProviderId::YoutubeWeb;
const PLATFORM: Platform = Platform::Youtube;

// The InnerTube client of the player request, as youtube-transcript-api sends it
// (`_settings.py` at 8f150ba): the ANDROID client's caption URLs need no proof-of-origin token.
const PLAYER_PATH: &str = "youtubei/v1/player";
const PLAYER_CLIENT_NAME: &str = "ANDROID";
const PLAYER_CLIENT_VERSION: &str = "20.10.38";
// Keeps YouTube's reasons in English, which the bot check below matches.
const LANGUAGE: &str = "en-US";
const KEY_MARKER: &str = "\"INNERTUBE_API_KEY\":";
const PLAYER_RESPONSE_MARKER: &str = "ytInitialPlayerResponse = ";
const BOT_CHECK_MARKER: &str = "class=\"g-recaptcha\"";
const IP_RESTRICTED: &str =
    "YouTube is temporarily restricting this egress IP; wait, or switch networks, and try again";

fn player_body(video_id: &str) -> serde_json::Value {
    json!({
        "context": {"client": {"clientName": PLAYER_CLIENT_NAME, "clientVersion": PLAYER_CLIENT_VERSION}},
        "videoId": video_id,
    })
}

/// Returns whether the route can fetch the request; it never sends a request.
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
        ContentDepth::Metadata | ContentDepth::Abstract | ContentDepth::FullText => Ok(()),
        depth => Err(format!(
            "{} cannot fetch at depth `{}`",
            ROUTE.name(),
            depth.as_str()
        )),
    }
}

/// The three requests of a fetch.
#[derive(Clone, Copy)]
enum Step {
    Watch,
    Player,
    Captions,
}

impl Step {
    const fn timeout_message(self) -> &'static str {
        match self {
            Self::Watch => "YouTube watch page request timed out",
            Self::Player => "YouTube player request timed out",
            Self::Captions => "YouTube caption request timed out",
        }
    }
}

pub(crate) struct YoutubeWeb {
    config: HttpRouteRuntimeConfig,
    client: Client,
    // The route needs no credentials; the empty pool only drives shared redaction.
    credentials: CredentialPool,
    limiter: RateLimiter,
    deadline: Deadline,
}

impl YoutubeWeb {
    pub(crate) fn new(
        config: HttpRouteRuntimeConfig,
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

    /// Reads one video: the watch page and the player at every depth, and the chosen caption
    /// track at `full_text`, whose transcript is the native full text.
    pub(crate) async fn fetch(
        &self,
        request: &PlatformFetchRequest,
    ) -> Result<PlatformFetchOutcome, ProviderError> {
        fetch_support(request).map_err(parameter_error)?;
        let PlatformRef::Youtube(reference) = &request.reference else {
            unreachable!("the support check accepts only YouTube refs")
        };
        let (languages, require_description) = match &request.options {
            PlatformFetchOptions::Youtube(options) => (
                options.languages.as_slice(),
                !options.allow_empty_description,
            ),
            _ => (&[][..], true),
        };
        let base = self.config.url.trim_end_matches('/');
        let mut attempts = Vec::new();
        let watch_url = format!("{base}/watch");
        let page = carry(
            &mut attempts,
            self.send(
                Step::Watch,
                || self.client.get(&watch_url).query(&[("v", reference.id())]),
                read_watch_page,
            )
            .await,
        )?;
        let player_url = format!("{base}/{PLAYER_PATH}");
        let body = player_body(reference.id());
        let video = carry(
            &mut attempts,
            self.send(
                Step::Player,
                || {
                    self.client
                        .post(&player_url)
                        .query(&[("key", page.key.as_str())])
                        .json(&body)
                },
                |text| {
                    read_player(
                        text,
                        reference,
                        &page,
                        (request.depth, require_description),
                        languages,
                        &self.config.url,
                    )
                },
            )
            .await,
        )?;
        let Video {
            mut item,
            description,
            caption,
        } = video;
        let Some(caption) = caption else {
            return Ok(PlatformFetchOutcome {
                item,
                content_source: FullTextSource::Urls(Vec::new()),
                attempts,
                diagnostic: None,
            });
        };
        let (markdown, info) = carry(
            &mut attempts,
            self.send(
                Step::Captions,
                || self.client.get(caption.url.clone()),
                |text| render_transcript(text, &caption, &item, &description),
            )
            .await,
        )?;
        if let PlatformItemData::Youtube(data) = &mut item.data {
            data.transcript = Some(info);
        }
        Ok(PlatformFetchOutcome {
            item,
            content_source: FullTextSource::Native(markdown),
            attempts,
            diagnostic: None,
        })
    }

    /// Sends one request under the route's access policy, once; `decode` turns the body into
    /// the step's value inside the attempt, so its refusal is that attempt's failure.
    async fn send<T>(
        &self,
        step: Step,
        build: impl Fn() -> RequestBuilder,
        decode: impl Fn(&str) -> Result<T, Refusal>,
    ) -> Result<ExecutionOutcome<T>, ProviderError> {
        let (build, decode) = (&build, &decode);
        execute_anonymous(self.settings(step), |deadline| async move {
            let _permit = acquire_window(&self.limiter, deadline).await?;
            let request = build().header(ACCEPT_LANGUAGE, LANGUAGE);
            let response = send_provider_request(request, &self.credentials).await?;
            let body = read_complete_protocol(response, &self.credentials, failure_message).await?;
            let value = decode(&body.text).map_err(|refusal| AttemptFailure {
                kind: refusal.kind,
                status: Some(body.status),
                message: refusal.message,
            })?;
            Ok((Some(body.status), value))
        })
        .await
    }

    fn settings(&self, step: Step) -> ExecutionSettings {
        ExecutionSettings {
            provider: ROUTE.name(),
            target: AttemptTarget::platform(PLATFORM.as_str(), "fetch"),
            // A retry would hit an endpoint that may be restricting this IP again.
            retry_policy: RetryPolicy::new(1, 1.0, Duration::ZERO),
            deadline: self.deadline,
            attempt_timeout: Duration::from_secs(self.config.timeout_seconds),
            verbose: false,
            timeout_message: step.timeout_message(),
            model: None,
            transport: Some("http"),
            endpoint_host: None,
            breaker_event: None,
        }
    }
}

/// Keeps a step's attempts; a failed step carries every attempt of the fetch so far.
fn carry<T>(
    attempts: &mut Vec<ProviderAttempt>,
    result: Result<ExecutionOutcome<T>, ProviderError>,
) -> Result<T, ProviderError> {
    match result {
        Ok(outcome) => {
            attempts.extend(outcome.attempts);
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

/// Why a successful response cannot become the step's value.
struct Refusal {
    kind: AttemptErrorKind,
    message: String,
}

impl Refusal {
    fn new(kind: AttemptErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

fn failure_message(_body: &str, status: u16) -> String {
    if status == 429 {
        format!("YouTube returned HTTP 429: {IP_RESTRICTED}")
    } else {
        format!("YouTube returned HTTP {status}")
    }
}

/// What the watch page gives: the page's public `InnerTube` key and the player response it
/// embeds.
struct WatchPage {
    key: String,
    player: Option<PlayerResponse>,
}

fn read_watch_page(html: &str) -> Result<WatchPage, Refusal> {
    let Some(key) = innertube_key(html) else {
        return Err(if html.contains(BOT_CHECK_MARKER) {
            Refusal::new(
                AttemptErrorKind::Auth,
                format!("YouTube answered with a bot check; {IP_RESTRICTED}"),
            )
        } else {
            Refusal::new(
                AttemptErrorKind::Runtime,
                "the YouTube watch page carries no InnerTube API key; the page format may have changed",
            )
        });
    };
    Ok(WatchPage {
        key: key.to_owned(),
        player: embedded_player_response(html),
    })
}

/// Finds `"INNERTUBE_API_KEY":"<key>"`, the key the page itself uses for the player endpoint.
fn innertube_key(html: &str) -> Option<&str> {
    let rest = html[html.find(KEY_MARKER)? + KEY_MARKER.len()..].trim_start();
    let rest = rest.strip_prefix('"')?;
    let key = &rest[..rest.find('"')?];
    (!key.is_empty()
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')))
    .then_some(key)
}

/// Parses the player response the page assigns to `ytInitialPlayerResponse`: the first JSON
/// value after the assignment, which a `;` and more script follow.
fn embedded_player_response(html: &str) -> Option<PlayerResponse> {
    let start = html.find(PLAYER_RESPONSE_MARKER)? + PLAYER_RESPONSE_MARKER.len();
    serde_json::Deserializer::from_str(&html[start..])
        .into_iter::<PlayerResponse>()
        .next()?
        .ok()
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlayerResponse {
    playability_status: Option<PlayabilityStatus>,
    video_details: Option<VideoDetails>,
    microformat: Option<Microformat>,
    captions: Option<Captions>,
}

#[derive(Debug, Deserialize)]
struct PlayabilityStatus {
    status: Option<String>,
    reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VideoDetails {
    video_id: Option<String>,
    title: Option<String>,
    author: Option<String>,
    channel_id: Option<String>,
    length_seconds: Option<String>,
    view_count: Option<String>,
    short_description: Option<String>,
    is_live_content: Option<bool>,
    is_upcoming: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Microformat {
    player_microformat_renderer: Option<MicroformatRenderer>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MicroformatRenderer {
    publish_date: Option<String>,
    live_broadcast_details: Option<LiveBroadcastDetails>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LiveBroadcastDetails {
    is_live_now: Option<bool>,
    end_timestamp: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Captions {
    player_captions_tracklist_renderer: Option<TrackList>,
}

/// The video as the first two requests read it, and at `full_text` the caption track to read
/// next.
struct Video {
    item: PlatformItem,
    description: String,
    caption: Option<Caption>,
}

/// The caption track a full-text fetch reads.
struct Caption {
    url: reqwest::Url,
    language: String,
    source: TranscriptSource,
}

/// Picks the caption track a full-text fetch reads; a live stream has none yet.
fn full_text_caption(
    reference: &YoutubeRef,
    live_status: YoutubeLiveStatus,
    tracks: &TrackList,
    languages: &[String],
    configured_url: &str,
) -> Result<Caption, Refusal> {
    if live_status == YoutubeLiveStatus::Live {
        return Err(Refusal::new(
            AttemptErrorKind::Quality,
            format!(
                "youtube:{reference} is streaming live; its captions are not complete until the stream ends"
            ),
        ));
    }
    let track = tracks.select(languages).map_err(|message| {
        Refusal::new(
            AttemptErrorKind::Quality,
            format!("youtube:{reference}: {message}"),
        )
    })?;
    let url = srv1_url(track.base_url(), configured_url)
        .map_err(|message| Refusal::new(AttemptErrorKind::Runtime, message))?;
    Ok(Caption {
        url,
        language: track.language().to_owned(),
        source: track.source().into(),
    })
}

/// Decides the player's answer: playability and identity first, then the item at the requested
/// depth (and whether `abstract` requires a description) and, at `full_text`, the caption track
/// to read.
fn read_player(
    body: &str,
    reference: &YoutubeRef,
    page: &WatchPage,
    (depth, require_description): (ContentDepth, bool),
    languages: &[String],
    configured_url: &str,
) -> Result<Video, Refusal> {
    let player = serde_json::from_str::<PlayerResponse>(body).map_err(|error| {
        Refusal::new(
            AttemptErrorKind::Runtime,
            format!("invalid YouTube player response: {error}"),
        )
    })?;
    check_playability(player.playability_status.as_ref(), reference)?;
    let details = player.video_details.as_ref().ok_or_else(|| {
        Refusal::new(
            AttemptErrorKind::Runtime,
            format!("the YouTube player answered youtube:{reference} without video details"),
        )
    })?;
    if details.video_id.as_deref() != Some(reference.id()) {
        return Err(Refusal::new(
            AttemptErrorKind::Runtime,
            format!(
                "the YouTube player answered youtube:{reference} with video `{}`",
                details.video_id.as_deref().unwrap_or_default()
            ),
        ));
    }
    let embedded = page.player.as_ref().filter(|embedded| {
        embedded
            .video_details
            .as_ref()
            .and_then(|details| details.video_id.as_deref())
            == Some(reference.id())
    });
    let details = embedded
        .and_then(|embedded| embedded.video_details.as_ref())
        .unwrap_or(details);
    let microformat = embedded
        .and_then(|embedded| embedded.microformat.as_ref())
        .and_then(|microformat| microformat.player_microformat_renderer.as_ref());
    let tracks = player
        .captions
        .and_then(|captions| captions.player_captions_tracklist_renderer)
        .unwrap_or_default();
    let live_status = live_status(details, microformat);
    let description = details.short_description.clone().unwrap_or_default();
    let (abstract_fields, caption) = match depth {
        ContentDepth::Abstract => {
            if require_description && description.trim().is_empty() {
                return Err(Refusal::new(
                    AttemptErrorKind::Quality,
                    format!("youtube:{reference} has no description"),
                ));
            }
            let chapters = chapters_from_description(&description);
            ((Some(description.clone()), Some(chapters)), None)
        }
        ContentDepth::FullText => {
            let caption =
                full_text_caption(reference, live_status, &tracks, languages, configured_url)?;
            ((None, None), Some(caption))
        }
        _ => ((None, None), None),
    };
    let item = PlatformItem {
        url: reference.canonical_url(),
        reference: PlatformRef::Youtube(reference.clone()),
        depth,
        title: details.title.clone(),
        authors: details.author.clone().into_iter().collect(),
        published: microformat
            .and_then(|microformat| microformat.publish_date.as_deref())
            .and_then(utc_timestamp),
        data: PlatformItemData::Youtube(YoutubeItemData {
            channel_ref: details
                .channel_id
                .as_deref()
                .and_then(YoutubeRef::channel)
                .map(PlatformRef::Youtube),
            snippet: None,
            duration_seconds: count(details.length_seconds.as_deref())
                .filter(|seconds| *seconds > 0),
            live_status: Some(live_status),
            views: count(details.view_count.as_deref()),
            likes: None,
            comments: None,
            language: tracks.original_language().map(str::to_owned),
            has_captions: Some(!tracks.tracks().is_empty()),
            caption_tracks: Some(tracks.listing()),
            description: abstract_fields.0,
            chapters: abstract_fields.1,
            transcript: None,
            position: None,
        }),
    };
    Ok(Video {
        item,
        description,
        caption,
    })
}

/// Maps the player's playability: a missing or unplayable video is the request's fault, a
/// sign-in demand is Auth, and a bot check says the IP is restricted.
fn check_playability(
    playability: Option<&PlayabilityStatus>,
    reference: &YoutubeRef,
) -> Result<(), Refusal> {
    let Some(playability) = playability else {
        return Ok(());
    };
    let reason = playability.reason.as_deref().unwrap_or("no reason given");
    match playability.status.as_deref() {
        None | Some("OK") => Ok(()),
        Some("ERROR" | "UNPLAYABLE") => Err(Refusal::new(
            AttemptErrorKind::Parameter,
            format!("YouTube video not found or not playable: youtube:{reference} ({reason})"),
        )),
        Some("LOGIN_REQUIRED") if reason.contains("not a bot") => Err(Refusal::new(
            AttemptErrorKind::Auth,
            format!("YouTube asked to confirm this is not a bot ({reason}); {IP_RESTRICTED}"),
        )),
        Some("LOGIN_REQUIRED") => Err(Refusal::new(
            AttemptErrorKind::Auth,
            format!(
                "YouTube requires signing in to play youtube:{reference} ({reason}); youtube_web reads anonymously and cannot open it"
            ),
        )),
        Some(status) => Err(Refusal::new(
            AttemptErrorKind::Parameter,
            format!("YouTube cannot play youtube:{reference}: {status} ({reason})"),
        )),
    }
}

/// Derives the live status: the broadcast details of the microformat decide it when present.
fn live_status(
    details: &VideoDetails,
    microformat: Option<&MicroformatRenderer>,
) -> YoutubeLiveStatus {
    let broadcast = microformat.and_then(|microformat| microformat.live_broadcast_details.as_ref());
    match broadcast {
        _ if details.is_upcoming == Some(true) => YoutubeLiveStatus::Upcoming,
        Some(broadcast) if broadcast.is_live_now == Some(true) => YoutubeLiveStatus::Live,
        Some(broadcast) if broadcast.end_timestamp.is_some() => YoutubeLiveStatus::Completed,
        Some(_) => YoutubeLiveStatus::Upcoming,
        // A stream without broadcast details may still be running, so it counts as live.
        None if details.is_live_content == Some(true) => YoutubeLiveStatus::Live,
        None => YoutubeLiveStatus::None,
    }
}

/// Reads the srv1 track and renders its transcript; the description and chapters go into the
/// document, not the item.
fn render_transcript(
    xml: &str,
    caption: &Caption,
    item: &PlatformItem,
    description: &str,
) -> Result<(String, TranscriptInfo), Refusal> {
    let cues = srv1_cues(xml).map_err(|error| match error {
        Srv1Error::Empty => Refusal::new(
            AttemptErrorKind::Runtime,
            format!("YouTube returned an empty `{}` caption track although the player lists it; YouTube may be softly blocking this egress IP, so wait or switch networks before trying again", caption.language),
        ),
        Srv1Error::Invalid(message) => Refusal::new(AttemptErrorKind::Runtime, message),
    })?;
    let chapters = chapters_from_description(description)
        .into_iter()
        .map(|chapter| TranscriptChapter {
            start_seconds: chapter.start_seconds,
            title: chapter.title,
        })
        .collect::<Vec<_>>();
    let PlatformItemData::Youtube(data) = &item.data else {
        unreachable!("youtube_web builds YouTube items")
    };
    let reference = item.reference.to_string();
    let subject = TranscriptSubject {
        title: item.title.as_deref(),
        reference: &reference,
        channel: item.authors.first().map(String::as_str),
        published: item.published.as_deref(),
        duration_seconds: data.duration_seconds,
        description,
    };
    let rendered = Transcript::new(&caption.language, caption.source, cues).render(
        &subject,
        &chapters,
        &|seconds| format!("{}&t={seconds}s", item.url),
    );
    Ok((rendered.markdown, rendered.info))
}

fn count(value: Option<&str>) -> Option<u64> {
    value.and_then(|value| value.parse().ok())
}

/// Normalizes an RFC 3339 time to a UTC ISO 8601 timestamp to the second.
fn utc_timestamp(value: &str) -> Option<String> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|time| time.to_utc().format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        LiveBroadcastDetails, MicroformatRenderer, PlayabilityStatus, VideoDetails,
        check_playability, embedded_player_response, innertube_key, live_status,
    };
    use crate::types::{AttemptErrorKind, YoutubeLiveStatus, YoutubeRef};

    #[test]
    fn the_watch_page_gives_its_innertube_key_and_embedded_player_response() {
        let page = r#"<script>var ytInitialPlayerResponse = {"videoDetails": {"videoId": "jNQXAC9IVRw", "title": "a;b}"}};var meta = 1;</script><script>ytcfg.set({"INNERTUBE_API_KEY": "AIza_Key-1","X":"y"})</script>"#;

        let embedded = embedded_player_response(page)
            .and_then(|player| player.video_details)
            .and_then(|details| details.title);

        assert_eq!(
            (innertube_key(page), embedded.as_deref()),
            (Some("AIza_Key-1"), Some("a;b}"))
        );
        assert_eq!(
            [
                innertube_key(r#""INNERTUBE_API_KEY":"""#),
                innertube_key(r#""INNERTUBE_API_KEY":"a b""#),
                innertube_key("<html></html>"),
            ],
            [None, None, None]
        );
    }

    fn playability(status: Option<&str>, reason: Option<&str>) -> Option<AttemptErrorKind> {
        let reference = YoutubeRef::video("jNQXAC9IVRw").expect("video ref");
        let status = PlayabilityStatus {
            status: status.map(str::to_owned),
            reason: reason.map(str::to_owned),
        };
        check_playability(Some(&status), &reference)
            .err()
            .map(|refusal| refusal.kind)
    }

    #[test]
    fn playability_maps_missing_videos_to_parameter_and_sign_in_demands_to_auth() {
        let kinds = [
            playability(Some("OK"), None),
            playability(None, None),
            playability(Some("ERROR"), Some("This video is unavailable")),
            playability(Some("UNPLAYABLE"), Some("Video unavailable")),
            playability(
                Some("LOGIN_REQUIRED"),
                Some("Sign in to confirm you’re not a bot"),
            ),
            playability(Some("LOGIN_REQUIRED"), Some("Sign in to confirm your age")),
            playability(Some("CONTENT_CHECK_REQUIRED"), None),
        ];

        assert_eq!(
            kinds,
            [
                None,
                None,
                Some(AttemptErrorKind::Parameter),
                Some(AttemptErrorKind::Parameter),
                Some(AttemptErrorKind::Auth),
                Some(AttemptErrorKind::Auth),
                Some(AttemptErrorKind::Parameter),
            ]
        );
    }

    fn details(live_content: bool, upcoming: Option<bool>) -> VideoDetails {
        VideoDetails {
            video_id: None,
            title: None,
            author: None,
            channel_id: None,
            length_seconds: None,
            view_count: None,
            short_description: None,
            is_live_content: Some(live_content),
            is_upcoming: upcoming,
        }
    }

    fn broadcast(live_now: bool, ended: bool) -> MicroformatRenderer {
        MicroformatRenderer {
            publish_date: None,
            live_broadcast_details: Some(LiveBroadcastDetails {
                is_live_now: Some(live_now),
                end_timestamp: ended.then(|| "2026-10-17T11:00:00+00:00".to_owned()),
            }),
        }
    }

    #[test]
    fn broadcast_details_decide_the_live_status() {
        let statuses = [
            live_status(&details(false, None), None),
            live_status(&details(true, Some(true)), Some(&broadcast(false, false))),
            live_status(&details(true, None), Some(&broadcast(true, false))),
            live_status(&details(true, None), Some(&broadcast(false, true))),
            live_status(&details(true, None), Some(&broadcast(false, false))),
            live_status(&details(true, None), None),
        ];

        assert_eq!(
            statuses,
            [
                YoutubeLiveStatus::None,
                YoutubeLiveStatus::Upcoming,
                YoutubeLiveStatus::Live,
                YoutubeLiveStatus::Completed,
                YoutubeLiveStatus::Upcoming,
                YoutubeLiveStatus::Live,
            ]
        );
    }
}
