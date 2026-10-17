//! The caption tracks of a YouTube player response: which one a fetch reads, where to read it,
//! and its srv1 XML timed text.

use reqwest::Url;
use serde::Deserialize;

use super::super::markup::markup_text;
use super::super::transcript::Cue;
use crate::types::{YoutubeCaptionSource, YoutubeCaptionTrack};

/// Names the operation that reads a video without a qualifying caption track.
const TRANSCRIBE_HINT: &str = "To have a model transcribe the video instead, not verbatim, run `forager platform youtube transcribe`";

/// The caption section of a player response; machine translations live elsewhere in it and are
/// never read.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TrackList {
    #[serde(default)]
    caption_tracks: Vec<Track>,
    #[serde(default)]
    audio_tracks: Vec<AudioTrack>,
    default_audio_track_index: Option<usize>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Track {
    base_url: String,
    language_code: String,
    /// `asr` for automatic speech recognition; absent for captions the publisher uploaded.
    kind: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AudioTrack {
    default_caption_track_index: Option<usize>,
}

impl Track {
    pub(super) fn language(&self) -> &str {
        &self.language_code
    }

    pub(super) fn source(&self) -> YoutubeCaptionSource {
        if self.kind.as_deref() == Some("asr") {
            YoutubeCaptionSource::Asr
        } else {
            YoutubeCaptionSource::Manual
        }
    }

    pub(super) fn base_url(&self) -> &str {
        &self.base_url
    }
}

impl TrackList {
    pub(super) fn tracks(&self) -> &[Track] {
        &self.caption_tracks
    }

    /// Lists the tracks as the item reports them.
    pub(super) fn listing(&self) -> Vec<YoutubeCaptionTrack> {
        self.caption_tracks
            .iter()
            .map(|track| YoutubeCaptionTrack {
                language: track.language_code.clone(),
                source: track.source(),
            })
            .collect()
    }

    /// Returns the video's original language: the language of its only ASR track, which
    /// recognized the speech; without exactly one, the language of the track the player shows
    /// by default.
    pub(super) fn original_language(&self) -> Option<&str> {
        let mut asr = self
            .caption_tracks
            .iter()
            .filter(|track| track.source() == YoutubeCaptionSource::Asr);
        if let (Some(only), None) = (asr.next(), asr.next()) {
            return Some(only.language());
        }
        let audio = self
            .audio_tracks
            .get(self.default_audio_track_index.unwrap_or(0))?;
        self.caption_tracks
            .get(audio.default_caption_track_index?)
            .map(Track::language)
    }

    /// Picks the track a full-text fetch reads. With `languages`, the first publisher track in
    /// their order, then the first ASR track in their order; without, the publisher track of
    /// the original language, then its ASR track. A machine translation never qualifies.
    ///
    /// # Errors
    ///
    /// Says which tracks exist when none qualifies.
    pub(super) fn select(&self, languages: &[String]) -> Result<&Track, String> {
        let picked = if languages.is_empty() {
            self.original_language().and_then(|original| {
                let primary = primary_subtag(original);
                self.first(YoutubeCaptionSource::Manual, |track| {
                    primary_subtag(track.language()).eq_ignore_ascii_case(primary)
                })
                .or_else(|| {
                    self.first(YoutubeCaptionSource::Asr, |track| {
                        primary_subtag(track.language()).eq_ignore_ascii_case(primary)
                    })
                })
            })
        } else {
            [YoutubeCaptionSource::Manual, YoutubeCaptionSource::Asr]
                .into_iter()
                .flat_map(|source| languages.iter().map(move |tag| (source, tag)))
                .find_map(|(source, tag)| {
                    self.first(source, |track| language_matches(tag, track.language()))
                })
        };
        picked.ok_or_else(|| self.no_track_message(languages))
    }

    fn first(
        &self,
        source: YoutubeCaptionSource,
        matches: impl Fn(&Track) -> bool,
    ) -> Option<&Track> {
        self.caption_tracks
            .iter()
            .find(|track| track.source() == source && matches(track))
    }

    fn no_track_message(&self, languages: &[String]) -> String {
        let wanted = if languages.is_empty() {
            self.original_language().map_or_else(
                || "a known original language".to_owned(),
                |original| format!("its original language `{original}`"),
            )
        } else {
            format!("`{}`", languages.join("`, `"))
        };
        let listed = |source| {
            self.caption_tracks
                .iter()
                .filter(|track| track.source() == source)
                .map(Track::language)
                .collect::<Vec<_>>()
                .join(", ")
        };
        let (manual, asr) = (
            listed(YoutubeCaptionSource::Manual),
            listed(YoutubeCaptionSource::Asr),
        );
        let available = match (manual.is_empty(), asr.is_empty()) {
            (true, true) => "it has no captions".to_owned(),
            (false, true) => format!("its captions are in {manual}; pass --lang to pick one"),
            (true, false) => {
                format!("its automatic captions are in {asr}; pass --lang to pick one")
            }
            (false, false) => format!(
                "its captions are in {manual}, its automatic captions in {asr}; pass --lang to pick one"
            ),
        };
        format!(
            "no caption track in {wanted} qualifies ({available}); machine translations are never used. {TRANSCRIBE_HINT}"
        )
    }
}

/// Matches a requested tag against a track language: a tag with a subtag must match exactly,
/// a bare tag matches any variant of its language. Letter case never matters.
fn language_matches(tag: &str, language: &str) -> bool {
    if tag.contains('-') {
        tag.eq_ignore_ascii_case(language)
    } else {
        tag.eq_ignore_ascii_case(primary_subtag(language))
    }
}

fn primary_subtag(language: &str) -> &str {
    language.split('-').next().unwrap_or(language)
}

/// Returns the address of a track's srv1 XML: its `baseUrl` without the `fmt` parameter. The
/// route reads only from the origin it is configured for, and a track that needs a proof of
/// origin token (`exp=xpe`) cannot be read anonymously.
///
/// # Errors
///
/// Says why the track cannot be read.
pub(super) fn srv1_url(base_url: &str, configured: &str) -> Result<Url, String> {
    let url = Url::parse(base_url)
        .map_err(|error| format!("the caption track URL is not a URL ({error})"))?;
    let configured = Url::parse(configured)
        .map_err(|error| format!("providers.youtube_web.url is not a URL ({error})"))?;
    if url.origin() != configured.origin() {
        return Err(format!(
            "the caption track lives at {}, outside the configured origin {}; youtube_web reads only from its own origin",
            url.origin().ascii_serialization(),
            configured.origin().ascii_serialization()
        ));
    }
    if url
        .query_pairs()
        .any(|(name, value)| name == "exp" && value == "xpe")
    {
        return Err(
            "the caption track needs a proof-of-origin token that an anonymous reader cannot obtain"
                .into(),
        );
    }
    let kept = url
        .query_pairs()
        .filter(|(name, _)| name != "fmt")
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    let mut srv1 = url;
    srv1.query_pairs_mut().clear().extend_pairs(kept);
    Ok(srv1)
}

#[derive(Deserialize)]
struct Srv1 {
    #[serde(rename = "text", default)]
    texts: Vec<Srv1Text>,
}

#[derive(Deserialize)]
struct Srv1Text {
    #[serde(rename = "@start")]
    start: f64,
    #[serde(rename = "@dur", default)]
    duration: f64,
    #[serde(rename = "$text", default)]
    text: String,
}

/// Why an srv1 document yields no transcript.
#[derive(Debug, PartialEq)]
pub(super) enum Srv1Error {
    /// The body or every cue is empty: YouTube answers a softly blocked IP this way.
    Empty,
    Invalid(String),
}

/// Reads the cues of an srv1 document. Each cue's text is HTML inside XML, so after the XML
/// unescaping its tags are dropped and its entities decoded; a whitespace-only cue is skipped.
///
/// # Errors
///
/// [`Srv1Error::Empty`] when no cue has text, [`Srv1Error::Invalid`] when the document or a
/// cue time is malformed.
pub(super) fn srv1_cues(xml: &str) -> Result<Vec<Cue>, Srv1Error> {
    if xml.trim().is_empty() {
        return Err(Srv1Error::Empty);
    }
    let document = quick_xml::de::from_str::<Srv1>(xml)
        .map_err(|error| Srv1Error::Invalid(format!("invalid srv1 caption XML: {error}")))?;
    let mut cues = Vec::new();
    for text in document.texts {
        let plain = markup_text(&text.text);
        if plain.is_empty() {
            continue;
        }
        let cue = Cue::from_seconds(text.start, text.start + text.duration, &plain)
            .map_err(|error| Srv1Error::Invalid(format!("invalid srv1 caption cue: {error}")))?;
        cues.push(cue);
    }
    if cues.is_empty() {
        Err(Srv1Error::Empty)
    } else {
        Ok(cues)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{Srv1Error, TRANSCRIBE_HINT, TrackList, srv1_cues, srv1_url};
    use crate::providers::transcript::Cue;

    fn tracks(tracks: &[(&str, bool)], default_track: Option<usize>) -> TrackList {
        let caption_tracks = tracks
            .iter()
            .map(|(language, asr)| {
                let mut track = json!({
                    "baseUrl": format!("https://www.youtube.com/api/timedtext?lang={language}"),
                    "languageCode": language,
                    "name": {"runs": [{"text": language}]}
                });
                if *asr {
                    track["kind"] = json!("asr");
                }
                track
            })
            .collect::<Vec<_>>();
        serde_json::from_value(json!({
            "captionTracks": caption_tracks,
            "audioTracks": [{"defaultCaptionTrackIndex": default_track}],
            "defaultAudioTrackIndex": 0
        }))
        .expect("track list")
    }

    fn pick(list: &TrackList, languages: &[&str]) -> Result<(String, bool), String> {
        let languages = languages
            .iter()
            .map(|tag| (*tag).to_owned())
            .collect::<Vec<_>>();
        list.select(&languages).map(|track| {
            (
                track.language().to_owned(),
                track.kind.as_deref() == Some("asr"),
            )
        })
    }

    #[test]
    fn the_original_language_is_the_only_asr_track_or_else_the_default_track() {
        let languages = [
            tracks(&[("de", false), ("en", false), ("en", true)], Some(0)),
            tracks(&[("en", false), ("de", false)], Some(1)),
            tracks(&[("en", true), ("fr", true)], Some(1)),
            tracks(&[("en", false)], None),
        ]
        .map(|list| list.original_language().map(str::to_owned));

        assert_eq!(
            languages,
            [
                Some("en".into()),
                Some("de".into()),
                Some("fr".into()),
                None
            ]
        );
    }

    #[test]
    fn without_languages_the_publisher_track_of_the_original_language_wins_over_its_asr_track() {
        let picks = [
            pick(
                &tracks(&[("de", false), ("en-GB", false), ("en", true)], Some(0)),
                &[],
            ),
            pick(&tracks(&[("de", false), ("en", true)], Some(0)), &[]),
            pick(&tracks(&[("en", false), ("de", false)], Some(0)), &[]),
        ];

        assert_eq!(
            picks,
            [
                Ok(("en-GB".into(), false)),
                Ok(("en".into(), true)),
                Ok(("en".into(), false)),
            ]
        );
    }

    #[test]
    fn requested_languages_try_every_publisher_track_before_any_asr_track() {
        let list = tracks(
            &[("fr", true), ("de", false), ("pt-BR", false), ("en", true)],
            Some(1),
        );

        let picks = [
            pick(&list, &["fr", "de"]),
            pick(&list, &["fr", "en"]),
            pick(&list, &["PT"]),
            pick(&list, &["pt-PT", "en"]),
        ];

        assert_eq!(
            picks,
            [
                Ok(("de".into(), false)),
                Ok(("fr".into(), true)),
                Ok(("pt-BR".into(), false)),
                Ok(("en".into(), true)),
            ]
        );
    }

    #[test]
    fn no_qualifying_track_names_the_available_languages_and_suggests_transcribe() {
        let list = tracks(&[("de", false), ("fr", false)], None);

        assert_eq!(
            pick(&list, &[]),
            Err(format!(
                "no caption track in a known original language qualifies (its captions are in de, fr; pass --lang to pick one); machine translations are never used. {TRANSCRIBE_HINT}"
            ))
        );
        assert_eq!(
            pick(&tracks(&[("de", false), ("en", true)], Some(0)), &["ja"]),
            Err(format!(
                "no caption track in `ja` qualifies (its captions are in de, its automatic captions in en; pass --lang to pick one); machine translations are never used. {TRANSCRIBE_HINT}"
            ))
        );
        assert_eq!(
            pick(&tracks(&[], None), &[]),
            Err(format!(
                "no caption track in a known original language qualifies (it has no captions); machine translations are never used. {TRANSCRIBE_HINT}"
            ))
        );
    }

    #[test]
    fn the_srv1_url_drops_fmt_and_stays_on_the_configured_origin() {
        let read = srv1_url(
            "https://www.youtube.com/api/timedtext?v=jNQXAC9IVRw&fmt=srv3&lang=en",
            "https://www.youtube.com",
        )
        .map(String::from);
        let elsewhere = srv1_url(
            "https://evil.example/api/timedtext?v=jNQXAC9IVRw",
            "https://www.youtube.com/",
        );
        let token_bound = srv1_url(
            "https://www.youtube.com/api/timedtext?v=jNQXAC9IVRw&exp=xpe",
            "https://www.youtube.com",
        );

        assert_eq!(
            read,
            Ok("https://www.youtube.com/api/timedtext?v=jNQXAC9IVRw&lang=en".to_owned())
        );
        assert!(elsewhere.is_err_and(|message| message.contains("outside the configured origin")));
        assert!(token_bound.is_err_and(|message| message.contains("proof-of-origin token")));
    }

    #[test]
    fn srv1_cues_unescape_html_inside_xml_and_skip_blank_cues() {
        let xml = r#"<?xml version="1.0" encoding="utf-8" ?><transcript><text start="1.2" dur="2.16">All right, so here we are, in front of the
elephants</text><text start="3" dur="1"> </text><text start="12.616" dur="1.751">and that&amp;#39;s &lt;i&gt;cool&lt;/i&gt;</text><text start="19">end</text></transcript>"#;

        assert_eq!(
            srv1_cues(xml),
            Ok(vec![
                Cue::from_seconds(
                    1.2,
                    3.36,
                    "All right, so here we are, in front of the elephants"
                )
                .expect("cue"),
                Cue::from_seconds(12.616, 14.367, "and that's cool").expect("cue"),
                Cue::from_seconds(19.0, 19.0, "end").expect("cue"),
            ])
        );
    }

    #[test]
    fn an_empty_srv1_body_or_one_without_text_is_empty_and_garbage_is_invalid() {
        let results = [
            srv1_cues(""),
            srv1_cues("<?xml version=\"1.0\" ?><transcript></transcript>"),
            srv1_cues("<transcript><text start=\"1\" dur=\"1\">\n</text></transcript>"),
        ];

        assert_eq!(
            results,
            [
                Err(Srv1Error::Empty),
                Err(Srv1Error::Empty),
                Err(Srv1Error::Empty)
            ]
        );
        assert!(matches!(
            srv1_cues("<html>blocked"),
            Err(Srv1Error::Invalid(_))
        ));
        assert!(matches!(
            srv1_cues("<transcript><text start=\"-1\">x</text></transcript>"),
            Err(Srv1Error::Invalid(_))
        ));
    }
}
