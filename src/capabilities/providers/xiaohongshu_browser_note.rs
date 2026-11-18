//! Xiaohongshu fetch: opening one note page with its access token, then judging what it
//! showed: its identity check, its metadata, and its native full text.

use std::collections::BTreeMap;
use std::time::Duration;

use reqwest::Url;
use serde::Deserialize;

use super::page::{Reading, page_script, site_capture};
use super::{
    PageFacts, ROUTE, XiaohongshuBrowser, beijing_time, classify_blocks, count_text, is_note_page,
    note_unavailable, runtime, unexpected_page, unfinished_read, without_token,
};
use crate::catalog::PlatformOperation;
use crate::net::AttemptFailure;
use crate::providers::browser::{CaptureLevel, Page};
use crate::providers::execution::execute_anonymous;
use crate::providers::shared::{other_platform_message, parameter_error};
use crate::types::{
    AccessToken, AttemptErrorKind, ContentDepth, FullTextSource, PlatformFetchOutcome,
    PlatformFetchRequest, PlatformItem, PlatformItemData, PlatformRef, ProviderError,
    XiaohongshuImage, XiaohongshuNoteData, XiaohongshuRef, XiaohongshuVideo,
};

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

const POLL: Duration = Duration::from_millis(500);

impl XiaohongshuBrowser {
    /// Opens the note page with its access token in one browser command and reads the note the
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
        let execution = execute_anonymous(
            self.settings(PlatformOperation::Fetch.as_str()),
            move |deadline| async move {
                let read = async {
                    let data = self
                        .browser
                        .read(capture, deadline, async |page| {
                            observe_note(page, requested, token).await
                        })
                        .await?;
                    note_outcome(&data, requested, token, full_text)
                };
                read.await
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

fn capture(url: &Url) -> CaptureLevel {
    site_capture(url, &[])
}

// Reads the note the page rendered from its server-side state, keeping only the fields forager
// decodes. The signed, expiring video stream URLs and the page's own access token never leave
// the page.
fn note_script(id: &str) -> String {
    page_script(&format!(
        r"
    const id = {id};
    const map = noteDetailMap();
    const entry = map && map[id];
    const raw = entry && entry.note;
    if (!raw || typeof raw !== 'object' || !raw.noteId) return null;
    const note = JSON.parse(JSON.stringify(raw));
    let video = null;
    if (note.video && typeof note.video === 'object') {{
      const stream = {{}};
      const source = (note.video.media && note.video.media.stream) || {{}};
      for (const [encoding, renditions] of Object.entries(source)) {{
        if (Array.isArray(renditions)) stream[encoding] = renditions.map((item) => pick(item, ['width', 'height']));
      }}
      video = {{ capa: pick(note.video.capa, ['duration']), media: {{ stream }} }};
    }}
    return {{
      ...pick(note, ['noteId', 'title', 'desc', 'type', 'time', 'lastUpdateTime', 'ipLocation']),
      tagList: (note.tagList || []).map((tag) => pick(tag, ['name'])),
      imageList: (note.imageList || []).map((image) => pick(image, ['urlDefault', 'width', 'height'])),
      interactInfo: pick(note.interactInfo, ['likedCount', 'collectedCount', 'commentCount', 'shareCount']),
      user: pick(note.user, ['userId', 'nickname']),
      video,
    }};",
        id = serde_json::Value::from(id),
    ))
}

/// Drives one note read: opens the note page with its access token and polls until the page
/// renders the note, the page facts end the read, or the read deadline passes.
async fn observe_note(
    page: &mut Page,
    requested: &XiaohongshuRef,
    token: &AccessToken,
) -> Result<NoteData, AttemptFailure> {
    let script = note_script(requested.note_id());
    let mut reading = Reading::new(page);
    reading.navigate(&requested.access_url(token)).await?;
    let mut note = None;
    let mut timed_out = true;
    while reading.page().reading() {
        note = reading
            .page()
            .evaluate::<Option<NoteState>>(&script)
            .await
            .ok()
            .flatten();
        let ended = reading.poll().await?;
        if note.is_some() || ended {
            timed_out = false;
            break;
        }
        reading.page().rest(POLL).await;
    }
    if timed_out {
        reading.mark_timed_out();
    }
    if note.is_some() {
        reading.clear_notice();
    }
    let observed = reading.finish()?;
    Ok(NoteData {
        page: observed.page,
        note,
        timed_out: observed.timed_out,
    })
}

/// Judges one note page observation: classifies the page facts, checks that the page shows the
/// requested note, and decodes it, with its Native body at full-text depth.
fn note_outcome(
    data: &NoteData,
    requested: &XiaohongshuRef,
    token: &AccessToken,
    full_text: bool,
) -> Result<(PlatformItem, FullTextSource), AttemptFailure> {
    classify_blocks(&data.page, |code, notice| {
        note_unavailable(requested, code, notice)
    })?;
    let Some(note) = &data.note else {
        let on_note = is_note_page(&data.page.url, requested);
        return Err(unfinished_read(&data.page, data.timed_out, on_note, || {
            "the Xiaohongshu note page showed no note before the read deadline".into()
        })
        .unwrap_or_else(|| unexpected_page(&data.page)));
    };
    let shown = XiaohongshuRef::from_note_id(&note.note_id);
    if shown.as_ref() != Some(requested) {
        return Err(runtime(format!(
            "the Xiaohongshu page shows note `{}` for xiaohongshu:{requested}",
            note.note_id
        )));
    }
    let item = note_item(note, requested, token);
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

/// What one note page read observed.
#[derive(Debug, Default)]
#[cfg_attr(test, derive(Deserialize))]
#[cfg_attr(test, serde(default))]
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
        title: Some(
            note.title
                .as_deref()
                .map(str::trim)
                .unwrap_or_default()
                .to_owned(),
        ),
        authors: text(note.user.nickname.as_ref()).into_iter().collect(),
        published: note.time.and_then(beijing_time),
        data: PlatformItemData::XiaohongshuNote(XiaohongshuNoteData {
            updated: note.last_update_time.and_then(beijing_time),
            note_type: note.kind.as_deref().map(|kind| match kind {
                "normal" => "image".to_owned(),
                other => other.to_owned(),
            }),
            author_id: text(note.user.user_id.as_ref()),
            likes: count_text(note.interact_info.likes.as_ref()),
            collects: count_text(note.interact_info.collects.as_ref()),
            comments: count_text(note.interact_info.comments.as_ref()),
            shares: count_text(note.interact_info.shares.as_ref()),
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
    let title = item.title.as_deref().unwrap_or_default();
    if title.is_empty() && desc.is_empty() && data.images.is_empty() {
        return None;
    }
    let mut blocks = Vec::new();
    if !title.is_empty() {
        blocks.push(format!("# {title}"));
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

#[cfg(test)]
#[path = "xiaohongshu_browser_note_tests.rs"]
mod tests;
