//! Xiaohongshu fetch: one note page, its identity check, its metadata, and its native full text.

use std::collections::BTreeMap;

use chrono::{DateTime, FixedOffset, SecondsFormat};
use serde::Deserialize;

use super::{
    PageFacts, ROUTE, XiaohongshuBrowser, classify_blocks, describe, runtime, without_token,
};
use crate::catalog::PlatformOperation;
use crate::net::AttemptFailure;
use crate::providers::execution::execute_anonymous;
use crate::providers::opencli::{self, EnvelopeStatus};
use crate::providers::shared::{other_platform_message, parameter_error};
use crate::types::{
    AccessToken, AttemptErrorKind, ContentDepth, Deadline, FullTextSource, PlatformFetchOutcome,
    PlatformFetchRequest, PlatformItem, PlatformItemData, PlatformRef, ProviderError,
    XiaohongshuImage, XiaohongshuNoteData, XiaohongshuRef, XiaohongshuVideo,
};

const SITE_HOST: &str = "www.xiaohongshu.com";
const BEIJING_SECONDS: i32 = 8 * 3600;

/// Returns whether the route can fetch the request; it never starts a process. A note opens
/// only with its access token.
pub(crate) fn fetch_support(request: &PlatformFetchRequest) -> Result<(), String> {
    let PlatformRef::Xiaohongshu(_) = &request.reference else {
        return Err(other_platform_message(ROUTE, request.reference.platform()));
    };
    match request.depth {
        ContentDepth::Metadata | ContentDepth::FullText if request.access.is_some() => Ok(()),
        ContentDepth::Metadata | ContentDepth::FullText => {
            Err(format!("{} needs the note's access token", ROUTE.name()))
        }
        depth => Err(format!(
            "{} cannot fetch at depth `{}`",
            ROUTE.name(),
            depth.as_str()
        )),
    }
}

impl XiaohongshuBrowser {
    /// Opens the note page with its access token in one OpenCLI command and reads the note the
    /// page rendered. At full-text depth the same attempt builds the Markdown body, so the body
    /// needs no Web Fetch provider.
    pub(crate) async fn fetch(
        &self,
        request: &PlatformFetchRequest,
    ) -> Result<PlatformFetchOutcome, ProviderError> {
        fetch_support(request).map_err(parameter_error)?;
        let (PlatformRef::Xiaohongshu(requested), Some(token)) =
            (&request.reference, &request.access)
        else {
            unreachable!("support checked")
        };
        let full_text = request.depth == ContentDepth::FullText;
        let command = self.command(
            "note",
            vec![
                ("id", requested.note_id().to_owned()),
                ("xsec-token", token.as_str().to_owned()),
            ],
        );
        let command = &command;
        let execution = execute_anonymous(
            self.settings(PlatformOperation::Fetch),
            move |deadline| async move {
                read_note(command, self, requested, token, full_text, deadline)
                    .await
                    .map(|value| (None, value))
                    .map_err(|failure| without_token(failure, token))
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

async fn read_note(
    command: &opencli::OpenCliCommand<'_>,
    route: &XiaohongshuBrowser,
    requested: &XiaohongshuRef,
    token: &AccessToken,
    full_text: bool,
    deadline: Deadline,
) -> Result<(PlatformItem, FullTextSource), AttemptFailure> {
    let envelope = opencli::run::<NoteData>(command, &route.limiter, deadline).await?;
    if envelope.status != EnvelopeStatus::Ok {
        return Err(runtime(
            "the forager-xhs adapter reported no results instead of page facts".into(),
        ));
    }
    let data = envelope.data;
    classify_blocks(&data.page, |code, notice| {
        format!(
            "Xiaohongshu note unavailable: xiaohongshu:{requested} ({code}: {notice}); the access token may be stale, the note restricted or removed, or the account rate-limited"
        )
    })?;
    let Some(note) = data.note else {
        if data.timed_out && is_note_page(&data.page.url, requested) {
            return Err(AttemptFailure {
                kind: AttemptErrorKind::Timeout,
                status: None,
                message: "the Xiaohongshu note page showed no note before the read deadline".into(),
            });
        }
        return Err(runtime(format!(
            "the forager-xhs adapter stopped at an unexpected page: {}",
            describe(&data.page)
        )));
    };
    let shown = XiaohongshuRef::from_note_id(&note.note_id);
    if shown.as_ref() != Some(requested) {
        return Err(runtime(format!(
            "the Xiaohongshu page shows note `{}` for xiaohongshu:{requested}",
            note.note_id
        )));
    }
    let item = note_item(&note, requested, token);
    if !full_text {
        return Ok((item, FullTextSource::Urls(Vec::new())));
    }
    let body = native_body(&item, note.desc.as_deref()).ok_or_else(|| AttemptFailure {
        kind: AttemptErrorKind::Quality,
        status: None,
        message: format!(
            "the Xiaohongshu note xiaohongshu:{requested} has no title, text, or images"
        ),
    })?;
    Ok((item, FullTextSource::Native(body)))
}

fn is_note_page(url: &str, requested: &XiaohongshuRef) -> bool {
    reqwest::Url::parse(url).is_ok_and(|url| {
        url.host_str() == Some(SITE_HOST)
            && url.path().trim_end_matches('/')
                == format!("/explore/{}", requested.note_id()).as_str()
    })
}

/// The facts the adapter reports for one note page.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct NoteData {
    page: PageFacts,
    /// `__INITIAL_STATE__.note.noteDetailMap[<id>].note`, when the page rendered it.
    note: Option<NoteState>,
    /// The read deadline passed before the note or a final page state appeared.
    timed_out: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct NoteState {
    note_id: String,
    title: Option<String>,
    desc: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    /// Milliseconds since the Unix epoch.
    time: Option<i64>,
    last_update_time: Option<i64>,
    tag_list: Vec<Tag>,
    image_list: Vec<Image>,
    interact_info: Counts,
    user: User,
    ip_location: Option<String>,
    video: Option<Video>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Tag {
    name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Image {
    url_default: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
}

/// The counts, which Xiaohongshu writes as text such as `1.2万`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Counts {
    #[serde(rename = "likedCount")]
    likes: Option<serde_json::Value>,
    #[serde(rename = "collectedCount")]
    collects: Option<serde_json::Value>,
    #[serde(rename = "commentCount")]
    comments: Option<serde_json::Value>,
    #[serde(rename = "shareCount")]
    shares: Option<serde_json::Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct User {
    user_id: Option<String>,
    nickname: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Video {
    capa: VideoCapa,
    media: VideoMedia,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct VideoCapa {
    /// Seconds.
    duration: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct VideoMedia {
    /// Encodings, each with its renditions; only the size of a rendition is read.
    stream: BTreeMap<String, Vec<Rendition>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Rendition {
    width: Option<u32>,
    height: Option<u32>,
}

fn note_item(note: &NoteState, reference: &XiaohongshuRef, token: &AccessToken) -> PlatformItem {
    let text = |value: Option<&String>| {
        value
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    };
    let count = |value: Option<&serde_json::Value>| match value {
        Some(serde_json::Value::String(text)) => Some(text.clone()),
        Some(serde_json::Value::Number(number)) => Some(number.to_string()),
        _ => None,
    };
    let video = note.video.as_ref().map(|video| {
        let rendition = video
            .media
            .stream
            .values()
            .find_map(|renditions| renditions.first());
        XiaohongshuVideo {
            duration_seconds: video.capa.duration,
            width: rendition.and_then(|rendition| rendition.width),
            height: rendition.and_then(|rendition| rendition.height),
        }
    });
    PlatformItem {
        url: reference.canonical_url(),
        depth: ContentDepth::Metadata,
        title: note
            .title
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .to_owned(),
        authors: text(note.user.nickname.as_ref()).into_iter().collect(),
        published: note.time.and_then(beijing_time),
        data: PlatformItemData::XiaohongshuNote(XiaohongshuNoteData {
            updated: note.last_update_time.and_then(beijing_time),
            note_type: note.kind.as_deref().map(|kind| match kind {
                "normal" => "image".to_owned(),
                other => other.to_owned(),
            }),
            author_id: text(note.user.user_id.as_ref()),
            likes: count(note.interact_info.likes.as_ref()),
            collects: count(note.interact_info.collects.as_ref()),
            comments: count(note.interact_info.comments.as_ref()),
            shares: count(note.interact_info.shares.as_ref()),
            tags: note
                .tag_list
                .iter()
                .filter_map(|tag| text(tag.name.as_ref()))
                .collect(),
            images: note
                .image_list
                .iter()
                .filter_map(|image| {
                    let url = image.url_default.as_deref()?.trim();
                    (url.starts_with("https://") || url.starts_with("http://")).then(|| {
                        XiaohongshuImage {
                            url: url.to_owned(),
                            width: image.width,
                            height: image.height,
                        }
                    })
                })
                .collect(),
            video,
            ip_location: text(note.ip_location.as_ref()),
            access_url: reference.access_url(token),
        }),
        reference: PlatformRef::Xiaohongshu(reference.clone()),
    }
}

/// Builds the Markdown body of a note: the title as a heading, the text as the page shows it,
/// the tags, the images, and a line for a video, which is never downloaded. A note without a
/// title, text, or images has no body.
fn native_body(item: &PlatformItem, desc: Option<&str>) -> Option<String> {
    let PlatformItemData::XiaohongshuNote(data) = &item.data else {
        return None;
    };
    let desc = desc.map(str::trim).unwrap_or_default();
    if item.title.is_empty() && desc.is_empty() && data.images.is_empty() {
        return None;
    }
    let mut blocks = Vec::new();
    if !item.title.is_empty() {
        blocks.push(format!("# {}", item.title));
    }
    if !desc.is_empty() {
        blocks.push(desc.to_owned());
    }
    if !data.tags.is_empty() {
        blocks.push(format!("标签：{}", data.tags.join("，")));
    }
    if !data.images.is_empty() {
        blocks.push(
            data.images
                .iter()
                .map(|image| format!("![]({})", image.url))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    if let Some(video) = &data.video {
        let duration = video
            .duration_seconds
            .map_or_else(|| "未知".to_owned(), |seconds| seconds.to_string());
        blocks.push(format!("（视频笔记，时长 {duration} 秒，视频文件未下载）"));
    }
    Some(blocks.join("\n\n"))
}

/// Converts milliseconds since the Unix epoch to an ISO 8601 timestamp in Beijing time.
fn beijing_time(milliseconds: i64) -> Option<String> {
    let beijing = FixedOffset::east_opt(BEIJING_SECONDS)?;
    DateTime::from_timestamp_millis(milliseconds).map(|time| {
        time.with_timezone(&beijing)
            .to_rfc3339_opts(SecondsFormat::Secs, false)
    })
}
