//! YouTube shapes: video, playlist, and channel refs and their URLs, search options, and
//! video fields.

use std::fmt;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use super::platform::{Platform, PlatformRef, PlatformRefError};

const YOUTUBE_MAX_LIMIT: u16 = 50;
const REF_PREFIX: &str = "youtube:";
const VIDEO_ID_LENGTH: usize = 11;
const CHANNEL_ID_LENGTH: usize = 24;
const MAX_PLAYLIST_ID_LENGTH: usize = 64;
const SITE_HOSTS: [&str; 3] = ["youtube.com", "www.youtube.com", "m.youtube.com"];
const NOCOOKIE_HOSTS: [&str; 2] = ["youtube-nocookie.com", "www.youtube-nocookie.com"];
const SHORT_HOST: &str = "youtu.be";
const MIN_CHAPTERS: usize = 3;
const TITLE_SEPARATORS: [char; 5] = ['-', '–', '—', '|', ':'];

const REF_HINT: &str = "pass a `youtube:<video_id>`, `youtube:playlist/<id>`, or `youtube:channel/<UC…>` ref, or a YouTube URL such as https://www.youtube.com/watch?v=<id>, https://youtu.be/<id>, https://www.youtube.com/playlist?list=<id>, or https://www.youtube.com/channel/<UC…>";
const HANDLE_HINT: &str = "an `@handle` channel page is not a ref; pass the channel's https://www.youtube.com/channel/<UC…> URL or a `youtube:channel/<UC…>` ref";
const LEGACY_CHANNEL_HINT: &str = "legacy /c/ and /user/ channel URLs need a network lookup to resolve; open the channel and pass its https://www.youtube.com/channel/<UC…> URL";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// What a YouTube ref names: one of three identity spaces.
pub enum YoutubeKind {
    /// A video; uploads, Shorts, live streams, and their recordings share one ID space.
    Video,
    /// A playlist.
    Playlist,
    /// A channel, by its `UC…` ID.
    Channel,
}

impl YoutubeKind {
    /// Returns the stable kind identifier.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Video => "video",
            Self::Playlist => "playlist",
            Self::Channel => "channel",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// A YouTube identity: a video, a playlist, or a channel. IDs are case-sensitive opaque strings,
/// and nothing has versions: a video's description and captions may change under the same ref.
pub struct YoutubeRef {
    kind: YoutubeKind,
    id: String,
}

impl YoutubeRef {
    /// Parses a `youtube:` ref or a recognizable YouTube URL.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformRefError`] for any other input.
    pub fn parse(input: &str) -> Result<Self, PlatformRefError> {
        let trimmed = input.trim();
        let parsed = match trimmed.get(..REF_PREFIX.len()) {
            Some(prefix) if prefix.eq_ignore_ascii_case(REF_PREFIX) => {
                parse_ref_body(&trimmed[REF_PREFIX.len()..]).ok_or(REF_HINT)
            }
            _ => parse_url(trimmed),
        };
        parsed.map_err(|hint| PlatformRefError {
            platform: Platform::Youtube,
            input: Some(input.to_owned()),
            hint,
        })
    }

    /// Returns the ref of a video ID: 11 letters, digits, `_`, or `-`.
    #[must_use]
    pub fn video(id: &str) -> Option<Self> {
        Self::new(YoutubeKind::Video, id)
    }

    /// Returns the ref of a playlist ID.
    #[must_use]
    pub fn playlist(id: &str) -> Option<Self> {
        Self::new(YoutubeKind::Playlist, id)
    }

    /// Returns the ref of a channel ID: `UC` and 22 letters, digits, `_`, or `-`.
    #[must_use]
    pub fn channel(id: &str) -> Option<Self> {
        Self::new(YoutubeKind::Channel, id)
    }

    fn new(kind: YoutubeKind, id: &str) -> Option<Self> {
        let length_fits = match kind {
            YoutubeKind::Video => id.len() == VIDEO_ID_LENGTH,
            YoutubeKind::Playlist => (2..=MAX_PLAYLIST_ID_LENGTH).contains(&id.len()),
            YoutubeKind::Channel => id.len() == CHANNEL_ID_LENGTH && id.starts_with("UC"),
        };
        let well_formed = length_fits
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));
        well_formed.then(|| Self {
            kind,
            id: id.to_owned(),
        })
    }

    /// Returns what the ref names.
    #[must_use]
    pub const fn kind(&self) -> YoutubeKind {
        self.kind
    }

    /// Returns the bare video, playlist, or channel ID.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the canonical page of the video, playlist, or channel.
    #[must_use]
    pub fn canonical_url(&self) -> String {
        match self.kind {
            YoutubeKind::Video => format!("https://www.youtube.com/watch?v={}", self.id),
            YoutubeKind::Playlist => format!("https://www.youtube.com/playlist?list={}", self.id),
            YoutubeKind::Channel => format!("https://www.youtube.com/channel/{}", self.id),
        }
    }
}

impl fmt::Display for YoutubeRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            YoutubeKind::Video => formatter.write_str(&self.id),
            YoutubeKind::Playlist => write!(formatter, "playlist/{}", self.id),
            YoutubeKind::Channel => write!(formatter, "channel/{}", self.id),
        }
    }
}

/// Parses what follows `youtube:`: a video ID, `playlist/<id>`, or `channel/<id>`, the kind
/// segment in any letter case.
fn parse_ref_body(body: &str) -> Option<YoutubeRef> {
    let Some((kind, id)) = body.split_once('/') else {
        return YoutubeRef::video(body);
    };
    if kind.eq_ignore_ascii_case("playlist") {
        YoutubeRef::playlist(id)
    } else if kind.eq_ignore_ascii_case("channel") {
        YoutubeRef::channel(id)
    } else {
        None
    }
}

/// Parses a YouTube page URL over http or https, ignoring viewing parameters, the fragment,
/// and a trailing `/`.
fn parse_url(input: &str) -> Result<YoutubeRef, &'static str> {
    let rest = strip_scheme(input).ok_or(REF_HINT)?;
    let rest = rest.split('#').next().unwrap_or_default();
    let (location, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (host, path) = location.split_once('/').unwrap_or((location, ""));
    let segments = path.trim_end_matches('/').split('/').collect::<Vec<_>>();
    let on = |hosts: &[&str]| hosts.iter().any(|known| host.eq_ignore_ascii_case(known));
    let parsed = if host.eq_ignore_ascii_case(SHORT_HOST) {
        match segments.as_slice() {
            [id] => YoutubeRef::video(id),
            _ => None,
        }
    } else if on(&NOCOOKIE_HOSTS) {
        match segments.as_slice() {
            ["embed", id] => YoutubeRef::video(id),
            _ => None,
        }
    } else if on(&SITE_HOSTS) {
        match segments.as_slice() {
            ["watch"] => single_parameter(query, "v").and_then(YoutubeRef::video),
            ["shorts" | "live" | "embed", id] => YoutubeRef::video(id),
            ["playlist"] => single_parameter(query, "list").and_then(YoutubeRef::playlist),
            ["channel", id] => YoutubeRef::channel(id),
            [handle, ..] if handle.starts_with('@') => return Err(HANDLE_HINT),
            ["c" | "user", ..] => return Err(LEGACY_CHANNEL_HINT),
            _ => None,
        }
    } else {
        None
    };
    parsed.ok_or(REF_HINT)
}

fn strip_scheme(input: &str) -> Option<&str> {
    let (scheme, rest) = input.split_once("://")?;
    (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")).then_some(rest)
}

/// Returns the value of a query parameter that occurs exactly once.
fn single_parameter<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    let mut values = query
        .split('&')
        .filter_map(|pair| pair.split_once('=').filter(|(key, _)| *key == name))
        .map(|(_, value)| value);
    let value = values.next()?;
    values.next().is_none().then_some(value)
}

/// Optional YouTube search filters and order; the Data API enforces each of them. A cursor
/// encodes them completely.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct YoutubeSearchOptions {
    pub sort: YoutubeSort,
    /// The first included UTC publication day.
    pub from: Option<NaiveDate>,
    /// The last included UTC publication day.
    pub to: Option<NaiveDate>,
    pub duration: Option<YoutubeDuration>,
    /// Only videos of this channel, by its `UC…` ID.
    pub channel: Option<String>,
    /// Only videos that have captions, in any language.
    pub has_captions: bool,
}

impl YoutubeSearchOptions {
    pub(super) fn validate(&self, query: &str, limit: u16) -> Result<(), String> {
        if !(1..=YOUTUBE_MAX_LIMIT).contains(&limit) {
            return Err(format!("--limit must be between 1 and {YOUTUBE_MAX_LIMIT}"));
        }
        if query.trim().is_empty() {
            return Err("youtube search needs a query".into());
        }
        if let Some(channel) = &self.channel
            && YoutubeRef::channel(channel).is_none()
        {
            return Err(format!(
                "invalid YouTube channel ID `{channel}`; a channel ID is `UC` and 22 letters, digits, `_`, or `-`"
            ));
        }
        if let (Some(from), Some(to)) = (self.from, self.to)
            && from > to
        {
            return Err("--from must not be later than --to".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// The order of YouTube search results.
pub enum YoutubeSort {
    /// Best match first.
    #[default]
    Relevance,
    /// Newest first.
    Date,
    /// Most viewed first.
    Views,
    /// Highest rated first.
    Rating,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// A video length class, as the Data API defines it.
pub enum YoutubeDuration {
    /// Under 4 minutes.
    Short,
    /// 4 to 20 minutes.
    Medium,
    /// Over 20 minutes.
    Long,
}

#[derive(Clone, Debug, Serialize)]
/// The YouTube fields of a video. A field a route cannot read is `null`, never guessed.
pub struct YoutubeItemData {
    /// The channel that published the video.
    pub channel_ref: Option<PlatformRef>,
    /// The description excerpt a listing carries; `null` outside listings.
    pub snippet: Option<String>,
    pub duration_seconds: Option<u64>,
    pub live_status: Option<YoutubeLiveStatus>,
    pub views: Option<u64>,
    pub likes: Option<u64>,
    pub comments: Option<u64>,
    /// The audio language, or else the language of the title and description.
    pub language: Option<String>,
    /// Whether the video has captions; it does not say in which languages.
    pub has_captions: Option<bool>,
    /// The caption tracks a route that reads the player lists, without machine translations;
    /// `null` for a route that cannot list them.
    pub caption_tracks: Option<Vec<YoutubeCaptionTrack>>,
    /// The complete description, at `abstract` depth.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The chapters the description's timestamps define, at `abstract` depth.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chapters: Option<Vec<YoutubeChapter>>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// Whether a video is, was, or will be a live stream.
pub enum YoutubeLiveStatus {
    /// An ordinary upload.
    None,
    /// A scheduled stream that has not started.
    Upcoming,
    /// A stream in progress.
    Live,
    /// The recording of a stream that has ended.
    Completed,
}

#[derive(Clone, Debug, Serialize)]
/// One caption track of a video.
pub struct YoutubeCaptionTrack {
    pub language: String,
    pub source: YoutubeCaptionSource,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// Who made a caption track.
pub enum YoutubeCaptionSource {
    /// The publisher uploaded it.
    Manual,
    /// YouTube's automatic speech recognition produced it.
    Asr,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
/// One chapter of a video: where it starts and its title.
pub struct YoutubeChapter {
    pub start_seconds: u64,
    pub title: String,
}

/// Reads the chapters a description defines, by YouTube's own rule: lines that start with a
/// timestamp, the first at `0:00`, at least three, in strictly increasing order. A description
/// that breaks the rule has no chapters.
pub(crate) fn chapters_from_description(description: &str) -> Vec<YoutubeChapter> {
    let chapters = description
        .lines()
        .filter_map(chapter_line)
        .collect::<Vec<_>>();
    let starts_at_zero = chapters
        .first()
        .is_some_and(|first| first.start_seconds == 0);
    let increasing = chapters
        .windows(2)
        .all(|pair| pair[0].start_seconds < pair[1].start_seconds);
    if starts_at_zero && increasing && chapters.len() >= MIN_CHAPTERS {
        chapters
    } else {
        Vec::new()
    }
}

/// Reads a line that starts with an `M:SS`, `MM:SS`, or `H:MM:SS` timestamp; the title is the
/// rest of the line without a leading separator.
fn chapter_line(line: &str) -> Option<YoutubeChapter> {
    let line = line.trim();
    let end = line
        .find(|character: char| !(character.is_ascii_digit() || character == ':'))
        .unwrap_or(line.len());
    let (timestamp, rest) = line.split_at(end);
    if !rest.is_empty() && !rest.starts_with(|character: char| character.is_whitespace()) {
        return None;
    }
    let start_seconds = timestamp_seconds(timestamp)?;
    let title = rest
        .trim_start_matches(|character: char| {
            character.is_whitespace() || TITLE_SEPARATORS.contains(&character)
        })
        .trim_end();
    Some(YoutubeChapter {
        start_seconds,
        title: title.to_owned(),
    })
}

fn timestamp_seconds(timestamp: &str) -> Option<u64> {
    let fields = timestamp.split(':').collect::<Vec<_>>();
    let (leading, rest) = fields.split_first()?;
    if !(1..=2).contains(&rest.len()) || leading.is_empty() {
        return None;
    }
    let mut seconds = leading.parse::<u64>().ok()?;
    for field in rest {
        if field.len() != 2 {
            return None;
        }
        let value = field.parse::<u64>().ok().filter(|value| *value < 60)?;
        seconds = seconds.checked_mul(60)?.checked_add(value)?;
    }
    Some(seconds)
}

#[cfg(test)]
mod tests {
    use super::{YoutubeChapter, chapters_from_description};
    use crate::types::{Platform, PlatformRef};

    fn chapter(start_seconds: u64, title: &str) -> YoutubeChapter {
        YoutubeChapter {
            start_seconds,
            title: title.to_owned(),
        }
    }

    #[test]
    fn timestamped_lines_from_zero_become_chapters_with_their_titles() {
        let description = "A lesson on attention.\n\n0:00 - Intro\n1:05 Recap: embeddings\n12:30 – Query, key, value\n1:02:03 | Wrap-up\nThanks for watching 3:00";

        assert_eq!(
            chapters_from_description(description),
            [
                chapter(0, "Intro"),
                chapter(65, "Recap: embeddings"),
                chapter(750, "Query, key, value"),
                chapter(3723, "Wrap-up"),
            ]
        );
    }

    #[test]
    fn descriptions_that_break_the_chapter_rule_have_no_chapters() {
        let broken = [
            "0:10 Intro\n1:00 Middle\n2:00 End",
            "0:00 Intro\n1:00 End",
            "0:00 Intro\n2:00 Middle\n1:00 End",
            "0:00 Intro\n1:00 Middle\n1:00 End",
            "0:00 Intro\n1:75 Middle\n2:00 End",
            "No timestamps at all",
        ]
        .map(chapters_from_description);

        assert_eq!(broken, [const { Vec::new() }; 6]);
    }

    fn youtube(input: &str) -> Option<String> {
        PlatformRef::parse(Platform::Youtube, input)
            .ok()
            .map(|reference| reference.to_string())
    }

    #[test]
    fn video_refs_and_every_video_url_form_parse_to_one_ref() {
        let parsed = [
            "youtube:jNQXAC9IVRw",
            "YouTube:jNQXAC9IVRw",
            " youtube:jNQXAC9IVRw ",
            "https://www.youtube.com/watch?v=jNQXAC9IVRw",
            "http://youtube.com/watch?v=jNQXAC9IVRw",
            "https://m.youtube.com/watch?v=jNQXAC9IVRw",
            "https://WWW.YouTube.com/watch?v=jNQXAC9IVRw",
            "https://www.youtube.com/watch?t=42&v=jNQXAC9IVRw&list=PLZHQObOWTQDPD3MizzM2xVFitgF8hE_ab&index=3&si=abc&pp=xyz",
            "https://www.youtube.com/watch?v=jNQXAC9IVRw#comments",
            "https://www.youtube.com/shorts/jNQXAC9IVRw",
            "https://www.youtube.com/live/jNQXAC9IVRw?si=abc",
            "https://www.youtube.com/embed/jNQXAC9IVRw?start=10",
            "https://www.youtube-nocookie.com/embed/jNQXAC9IVRw",
            "https://youtu.be/jNQXAC9IVRw?t=12",
            "https://youtu.be/jNQXAC9IVRw/",
        ]
        .map(youtube);

        assert_eq!(
            parsed,
            [(); 15].map(|()| Some("youtube:jNQXAC9IVRw".to_owned()))
        );
    }

    #[test]
    fn playlist_and_channel_refs_and_urls_parse_to_their_kind() {
        let parsed = [
            "youtube:playlist/PLZHQObOWTQDPD3MizzM2xVFitgF8hE_ab",
            "youtube:Playlist/PLZHQObOWTQDPD3MizzM2xVFitgF8hE_ab",
            "https://www.youtube.com/playlist?list=PLZHQObOWTQDPD3MizzM2xVFitgF8hE_ab",
            "youtube:channel/UCYO_jab_esuFRV4b17AJtAw",
            "https://www.youtube.com/channel/UCYO_jab_esuFRV4b17AJtAw",
            "https://m.youtube.com/channel/UCYO_jab_esuFRV4b17AJtAw/",
        ]
        .map(youtube);

        assert_eq!(
            parsed,
            [
                "youtube:playlist/PLZHQObOWTQDPD3MizzM2xVFitgF8hE_ab",
                "youtube:playlist/PLZHQObOWTQDPD3MizzM2xVFitgF8hE_ab",
                "youtube:playlist/PLZHQObOWTQDPD3MizzM2xVFitgF8hE_ab",
                "youtube:channel/UCYO_jab_esuFRV4b17AJtAw",
                "youtube:channel/UCYO_jab_esuFRV4b17AJtAw",
                "youtube:channel/UCYO_jab_esuFRV4b17AJtAw",
            ]
            .map(|reference| Some(reference.to_owned()))
        );
    }

    #[test]
    fn malformed_ids_other_paths_and_lookalike_hosts_are_rejected() {
        let parsed = [
            "jNQXAC9IVRw",
            "youtube:",
            "youtube:jNQXAC9IVR",
            "youtube:jNQXAC9IVRww",
            "youtube:jNQXAC9IV.w",
            "youtube:channel/PLZHQObOWTQDPD3MizzM2xVFitgF8hE_ab",
            "youtube:channel/UCYO_jab",
            "youtube:playlist/",
            "youtube:video/jNQXAC9IVRw",
            "https://www.youtube.com/watch?v=jNQXAC9IVRw&v=Ks-_Mh1QhMc",
            "https://www.youtube.com/watch?v=jNQXAC9IVR",
            "https://www.youtube.com/watch",
            "https://www.youtube.com/playlist?list=PLZHQObOWTQDPD3MizzM2xVFitgF8hE_ab&list=PLx",
            "https://www.youtube.com/shorts/jNQXAC9IVRw/extra",
            "https://www.youtube.com/results?search_query=zoo",
            "https://www.youtube-nocookie.com/watch?v=jNQXAC9IVRw",
            "https://youtube.com.evil.example/watch?v=jNQXAC9IVRw",
            "https://music.youtube.com/watch?v=jNQXAC9IVRw",
            "ftp://www.youtube.com/watch?v=jNQXAC9IVRw",
            "www.youtube.com/watch?v=jNQXAC9IVRw",
        ]
        .map(youtube);

        assert_eq!(parsed, [const { None }; 20]);
    }

    #[test]
    fn handle_and_legacy_channel_urls_explain_what_to_pass_instead() {
        let messages = [
            "https://www.youtube.com/@3blue1brown",
            "https://www.youtube.com/c/3blue1brown",
            "https://www.youtube.com/user/3blue1brown",
        ]
        .map(|input| {
            PlatformRef::parse(Platform::Youtube, input)
                .unwrap_err()
                .to_string()
        });

        assert!(
            messages[0].contains("handle") && messages[1..].iter().all(|m| m.contains("legacy")),
            "{messages:#?}"
        );
    }

    #[test]
    fn canonical_urls_of_every_kind_round_trip_to_the_ref() {
        let checked = [
            "youtube:jNQXAC9IVRw",
            "youtube:playlist/PLZHQObOWTQDPD3MizzM2xVFitgF8hE_ab",
            "youtube:channel/UCYO_jab_esuFRV4b17AJtAw",
        ]
        .map(|input| {
            let reference = PlatformRef::parse(Platform::Youtube, input).expect("ref");
            let round_trip = PlatformRef::parse(Platform::Youtube, &reference.canonical_url());
            (
                reference.canonical_url(),
                reference.kind(),
                round_trip.ok() == Some(reference),
            )
        });

        assert_eq!(
            checked,
            [
                (
                    "https://www.youtube.com/watch?v=jNQXAC9IVRw".to_owned(),
                    "video",
                    true
                ),
                (
                    "https://www.youtube.com/playlist?list=PLZHQObOWTQDPD3MizzM2xVFitgF8hE_ab"
                        .to_owned(),
                    "playlist",
                    true
                ),
                (
                    "https://www.youtube.com/channel/UCYO_jab_esuFRV4b17AJtAw".to_owned(),
                    "channel",
                    true
                ),
            ]
        );
    }
}
