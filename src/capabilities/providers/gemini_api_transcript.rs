//! Transcribe on `gemini_api`: the structured output the model must return, and its checked
//! rendering as a model transcript.
//!
//! The model answers in a JSON schema: a language and timed segments in seconds from the start
//! of the video. Output that is not valid JSON in that shape, or a segment the transcript module
//! rejects, fails the whole transcript: a partial transcript is never delivered. Segments past
//! the video's end, beyond a small tolerance, are dropped with a diagnostic.

use serde::Deserialize;
use serde_json::{Value, json};

use super::super::transcript::{Cue, Transcript, TranscriptChapter, TranscriptSubject};
use crate::types::{PlatformItem, PlatformItemData, TranscriptInfo, TranscriptSource};

pub(super) const INSTRUCTION: &str = "Transcribe all speech in this video in the language spoken, without translating it. Split it into segments of one or two sentences, in order, and give each segment's start and end in seconds from the start of the video. Set `language` to the BCP 47 tag of the main spoken language. If nobody speaks, return no segments.";

// A model places segments by its own reading of the video, so its last end may overrun the
// duration the platform reports by a few seconds.
const DURATION_TOLERANCE_SECONDS: f64 = 5.0;

/// Asks for JSON in the transcript schema.
pub(super) fn generation_config() -> Value {
    json!({
        "responseMimeType": "application/json",
        "responseSchema": {
            "type": "OBJECT",
            "properties": {
                "language": {"type": "STRING"},
                "segments": {
                    "type": "ARRAY",
                    "items": {
                        "type": "OBJECT",
                        "properties": {
                            "start": {"type": "NUMBER"},
                            "end": {"type": "NUMBER"},
                            "text": {"type": "STRING"},
                        },
                        "required": ["start", "end", "text"],
                        "propertyOrdering": ["start", "end", "text"],
                    },
                },
            },
            "required": ["language", "segments"],
            "propertyOrdering": ["language", "segments"],
        },
    })
}

#[derive(Deserialize)]
struct ModelTranscript {
    language: String,
    segments: Vec<Segment>,
}

#[derive(Deserialize)]
struct Segment {
    start: f64,
    end: f64,
    text: String,
}

/// A rendered model transcript and the diagnostic of the segments it dropped.
pub(super) struct Rendered {
    pub(super) markdown: String,
    pub(super) info: TranscriptInfo,
    pub(super) diagnostic: Option<String>,
}

/// Checks the model's output and renders it under the header of `video`, the item the metadata
/// check read; the error says why the output is no usable transcript.
pub(super) fn render(output: &str, video: &PlatformItem) -> Result<Rendered, String> {
    let transcript = serde_json::from_str::<ModelTranscript>(output).map_err(|error| {
        format!("the model's transcript is not valid JSON in the requested schema: {error}")
    })?;
    let language = transcript.language.trim();
    if language.is_empty() {
        return Err("the model's transcript names no language".into());
    }
    let PlatformItemData::Youtube(data) = &video.data else {
        unreachable!("only YouTube videos are transcribed")
    };
    let limit = data.duration_seconds.map(|seconds| {
        f64::from(u32::try_from(seconds).unwrap_or(u32::MAX)) + DURATION_TOLERANCE_SECONDS
    });
    let mut dropped = 0;
    let mut cues = Vec::with_capacity(transcript.segments.len());
    for segment in transcript.segments {
        if limit.is_some_and(|limit| segment.end > limit) {
            dropped += 1;
            continue;
        }
        let cue = Cue::from_seconds(segment.start, segment.end, &segment.text)
            .map_err(|reason| format!("the model's transcript is invalid: {reason}"))?;
        cues.push(cue);
    }
    if cues.is_empty() {
        return Err("the model's transcript has no segment within the video".into());
    }
    let description = data.description.as_deref().unwrap_or_default();
    let chapters = data
        .chapters
        .iter()
        .flatten()
        .map(|chapter| TranscriptChapter {
            start_seconds: chapter.start_seconds,
            title: chapter.title.clone(),
        })
        .collect::<Vec<_>>();
    let reference = video.reference.to_string();
    let subject = TranscriptSubject {
        title: video.title.as_deref(),
        reference: &reference,
        channel: video.authors.first().map(String::as_str),
        published: video.published.as_deref(),
        duration_seconds: data.duration_seconds,
        description,
    };
    let rendered = Transcript::new(language, TranscriptSource::Model, cues).render(
        &subject,
        &chapters,
        &|seconds| format!("{}&t={seconds}s", video.url),
    );
    Ok(Rendered {
        markdown: rendered.markdown,
        info: rendered.info,
        diagnostic: (dropped > 0).then(|| {
            format!(
                "dropped {dropped} model transcript segment(s) that end past the video's {}-second length",
                data.duration_seconds.unwrap_or_default()
            )
        }),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::render;
    use crate::types::{
        ContentDepth, PlatformItem, PlatformItemData, PlatformRef, TranscriptSource,
        YoutubeChapter, YoutubeItemData, YoutubeRef,
    };

    fn zoo(duration_seconds: Option<u64>) -> PlatformItem {
        let reference = YoutubeRef::video("jNQXAC9IVRw").expect("video ref");
        PlatformItem {
            url: reference.canonical_url(),
            reference: PlatformRef::Youtube(reference),
            depth: ContentDepth::Abstract,
            title: Some("Me at the zoo".into()),
            authors: vec!["jawed".into()],
            published: Some("2005-04-24T03:31:52Z".into()),
            data: PlatformItemData::Youtube(YoutubeItemData {
                channel_ref: None,
                snippet: None,
                duration_seconds,
                live_status: None,
                views: None,
                likes: None,
                comments: None,
                language: None,
                has_captions: None,
                caption_tracks: None,
                description: Some("The first video.".into()),
                chapters: Some(vec![YoutubeChapter {
                    start_seconds: 0,
                    title: "Intro".into(),
                }]),
                transcript: None,
                position: None,
            }),
        }
    }

    fn output(segments: &serde_json::Value) -> String {
        json!({"language": "en", "segments": segments}).to_string()
    }

    #[test]
    fn a_model_transcript_renders_with_its_chapters_and_drops_segments_past_the_end() {
        let rendered = render(
            &output(&json!([
                {"start": 1.2, "end": 3.4, "text": "All right, so here we are."},
                {"start": 16.9, "end": 23.5, "text": "And that's pretty much all."},
                {"start": 30.0, "end": 31.0, "text": "Invented."}
            ])),
            &zoo(Some(19)),
        )
        .expect("rendered transcript");

        assert_eq!(
            (
                rendered.info.language.as_str(),
                rendered.info.source,
                rendered.info.cues,
                rendered.diagnostic.as_deref(),
            ),
            (
                "en",
                TranscriptSource::Model,
                2,
                Some(
                    "dropped 1 model transcript segment(s) that end past the video's 19-second length"
                ),
            )
        );
        assert!(rendered.markdown.contains(
            "## Description\n\nThe first video.\n\n## Transcript\n\n### Intro\n\n[00:01](https://www.youtube.com/watch?v=jNQXAC9IVRw&t=1s) All right, so here we are."
        ));
    }

    #[test]
    fn output_that_is_no_complete_valid_transcript_is_refused() {
        let refused = [
            r#"{"language": "en", "segments": [{"start": 1.0, "end": 2.0, "te"#.to_owned(),
            json!({"segments": []}).to_string(),
            json!({"language": " ", "segments": [{"start": 0, "end": 1, "text": "a"}]}).to_string(),
            output(&json!([{"start": 3.0, "end": 2.0, "text": "backwards"}])),
            output(&json!([{"start": 1.0, "end": 2.0, "text": "  "}])),
            output(&json!([])),
            output(&json!([{"start": 40.0, "end": 41.0, "text": "past the end"}])),
        ]
        .map(|output| render(&output, &zoo(Some(19))).is_err());

        assert_eq!(refused, [true; 7]);
    }

    #[test]
    fn an_unknown_duration_keeps_every_segment() {
        let rendered = render(
            &output(&json!([{"start": 300.0, "end": 301.0, "text": "Late."}])),
            &zoo(None),
        )
        .expect("rendered transcript");

        assert_eq!((rendered.info.cues, rendered.diagnostic), (1, None));
    }
}
