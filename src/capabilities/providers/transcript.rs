//! Transcripts: one timed text of a video rendered as a Markdown document.
//!
//! The module knows no platform. A producer (a route that reads platform captions, or a model
//! that transcribed the video) validates its timed cues here, then renders them with the item's
//! header fields, its chapters, and a function that turns a moment of the video into a URL. The
//! document breaks paragraphs at chapter boundaries, after about 45 seconds, and at a pause of
//! two seconds; punctuated text prefers to break at a sentence end.

use std::fmt::Write as _;

use crate::types::{TranscriptInfo, TranscriptSource};

const PARAGRAPH_SPAN_MS: u64 = 45_000;
// A punctuated paragraph that finds no sentence end still breaks here.
const MAX_PARAGRAPH_SPAN_MS: u64 = 90_000;
const PAUSE_MS: u64 = 2_000;
const SENTENCE_ENDS: [char; 8] = ['.', '?', '!', '…', '。', '？', '！', '؟'];
const CLOSING_MARKS: [char; 6] = ['"', '\'', ')', '”', '’', '」'];

/// One timed piece of text, in milliseconds from the start of the video.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Cue {
    start_ms: u64,
    end_ms: u64,
    text: String,
}

impl Cue {
    /// Builds a cue from times in seconds. Cues may overlap and leave silent gaps; each one
    /// needs finite, non-negative times, an end no earlier than its start, and text.
    pub(super) fn from_seconds(start: f64, end: f64, text: &str) -> Result<Self, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err(format!("the cue at {start}s has no text"));
        }
        let (Some(start_ms), Some(end_ms)) = (milliseconds(start), milliseconds(end)) else {
            return Err(format!(
                "the cue `{text}` has an invalid time ({start}s to {end}s)"
            ));
        };
        if end_ms < start_ms {
            return Err(format!(
                "the cue `{text}` ends ({end}s) before it starts ({start}s)"
            ));
        }
        Ok(Self {
            start_ms,
            end_ms,
            text: text.to_owned(),
        })
    }
}

const MAX_EXACT_MILLISECONDS: f64 = 9_007_199_254_740_992.0;

fn milliseconds(seconds: f64) -> Option<u64> {
    let milliseconds = (seconds * 1000.0).round();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the value is finite, non-negative, and below 2^53"
    )]
    (0.0..MAX_EXACT_MILLISECONDS)
        .contains(&milliseconds)
        .then_some(milliseconds as u64)
}

/// A chapter of the video: where it starts and its title.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct TranscriptChapter {
    pub(super) start_seconds: u64,
    pub(super) title: String,
}

/// The item fields the document header shows; a missing field is left out.
pub(super) struct TranscriptSubject<'a> {
    pub(super) title: Option<&'a str>,
    /// The item's ref, such as `youtube:jNQXAC9IVRw`.
    pub(super) reference: &'a str,
    pub(super) channel: Option<&'a str>,
    pub(super) published: Option<&'a str>,
    pub(super) duration_seconds: Option<u64>,
    pub(super) description: &'a str,
}

/// The timed text of one video in one language, from one source.
pub(super) struct Transcript {
    language: String,
    source: TranscriptSource,
    cues: Vec<Cue>,
}

/// A rendered transcript document and what it holds.
pub(super) struct RenderedTranscript {
    pub(super) markdown: String,
    pub(super) info: TranscriptInfo,
}

impl Transcript {
    pub(super) fn new(language: &str, source: TranscriptSource, mut cues: Vec<Cue>) -> Self {
        cues.sort_by_key(|cue| cue.start_ms);
        Self {
            language: language.to_owned(),
            source,
            cues,
        }
    }

    /// Renders the document: the header, the description, then the paragraphs under their
    /// chapter headings, each opened by a link to its moment that `moment_url` builds from a
    /// whole second.
    pub(super) fn render(
        &self,
        subject: &TranscriptSubject<'_>,
        chapters: &[TranscriptChapter],
        moment_url: &dyn Fn(u64) -> String,
    ) -> RenderedTranscript {
        let paragraphs = self.paragraphs(chapters);
        let mut markdown = header(subject, &self.language, self.source);
        markdown.push_str("## Transcript\n");
        let mut chapter = None;
        for paragraph in &paragraphs {
            if paragraph.chapter != chapter {
                chapter = paragraph.chapter;
                if let Some(index) = chapter {
                    let _ = write!(markdown, "\n### {}\n", chapters[index].title);
                }
            }
            let seconds = paragraph.start_ms / 1000;
            let _ = write!(
                markdown,
                "\n[{}]({}) {}\n",
                timestamp(seconds),
                moment_url(seconds),
                paragraph.text
            );
        }
        RenderedTranscript {
            markdown,
            info: TranscriptInfo {
                language: self.language.clone(),
                source: self.source,
                cues: self.cues.len(),
                paragraphs: paragraphs.len(),
            },
        }
    }

    /// Merges the cues into paragraphs; a chapter boundary or a pause always breaks one.
    fn paragraphs(&self, chapters: &[TranscriptChapter]) -> Vec<Paragraph> {
        let punctuated = self.source != TranscriptSource::Asr;
        let mut paragraphs: Vec<Paragraph> = Vec::new();
        let mut open = false;
        let mut last_end_ms = 0;
        for cue in &self.cues {
            let chapter = chapter_at(chapters, cue.start_ms);
            let continues = open
                && paragraphs
                    .last()
                    .is_some_and(|paragraph| paragraph.chapter == chapter)
                && cue.start_ms.saturating_sub(last_end_ms) < PAUSE_MS;
            match paragraphs.last_mut() {
                Some(paragraph) if continues => {
                    if needs_space(&paragraph.text, &cue.text) {
                        paragraph.text.push(' ');
                    }
                    paragraph.text.push_str(&cue.text);
                }
                _ => paragraphs.push(Paragraph {
                    chapter,
                    start_ms: cue.start_ms,
                    text: cue.text.clone(),
                }),
            }
            last_end_ms = last_end_ms.max(cue.end_ms);
            let span = cue
                .end_ms
                .saturating_sub(paragraphs.last().map_or(0, |p| p.start_ms));
            open = if punctuated {
                span < MAX_PARAGRAPH_SPAN_MS
                    && (span < PARAGRAPH_SPAN_MS || !ends_sentence(&cue.text))
            } else {
                span < PARAGRAPH_SPAN_MS
            };
        }
        paragraphs
    }
}

struct Paragraph {
    chapter: Option<usize>,
    start_ms: u64,
    text: String,
}

/// Returns the index of the chapter that holds a moment: the last one starting at or before it.
fn chapter_at(chapters: &[TranscriptChapter], moment_ms: u64) -> Option<usize> {
    chapters
        .iter()
        .rposition(|chapter| chapter.start_seconds.saturating_mul(1000) <= moment_ms)
}

/// Says whether two joined cues need a space between them: Chinese and Japanese text, with its
/// full-width punctuation, runs on without spaces; every other script, Korean included, keeps one.
fn needs_space(before: &str, after: &str) -> bool {
    let unspaced = |character: Option<char>| character.is_some_and(writes_without_spaces);
    !(unspaced(before.chars().next_back()) && unspaced(after.chars().next()))
}

fn writes_without_spaces(character: char) -> bool {
    matches!(
        character,
        '\u{3000}'..='\u{30FF}' // CJK punctuation, hiragana, katakana
            | '\u{3100}'..='\u{312F}' // bopomofo
            | '\u{3400}'..='\u{4DBF}' // CJK extension A
            | '\u{4E00}'..='\u{9FFF}' // CJK unified ideographs
            | '\u{F900}'..='\u{FAFF}' // CJK compatibility ideographs
            | '\u{FF00}'..='\u{FFEF}' // full-width forms
            | '\u{20000}'..='\u{3134F}' // CJK extensions B to G
    )
}

fn ends_sentence(text: &str) -> bool {
    text.trim_end()
        .trim_end_matches(CLOSING_MARKS)
        .ends_with(SENTENCE_ENDS)
}

fn header(subject: &TranscriptSubject<'_>, language: &str, source: TranscriptSource) -> String {
    let mut header = format!("# {}\n\n", subject.title.unwrap_or(subject.reference));
    let _ = writeln!(header, "- Ref: `{}`", subject.reference);
    if let Some(channel) = subject.channel {
        let _ = writeln!(header, "- Channel: {channel}");
    }
    if let Some(published) = subject.published {
        let _ = writeln!(header, "- Published: {published}");
    }
    if let Some(seconds) = subject.duration_seconds {
        let _ = writeln!(header, "- Duration: {}", timestamp(seconds));
    }
    let (origin, notice) = match source {
        TranscriptSource::Manual => (
            "captions the publisher uploaded",
            "This document holds only caption text; nothing here describes the video's picture or its non-speech sounds.",
        ),
        TranscriptSource::Asr => (
            "the platform's automatic speech recognition",
            "This document holds only caption text; nothing here describes the video's picture or its non-speech sounds.",
        ),
        TranscriptSource::Model => (
            "generated by a model that watched and listened to the video",
            "A model generated this transcript; it is not guaranteed to be verbatim.",
        ),
    };
    let _ = writeln!(header, "- Transcript: `{language}`, {origin}");
    let _ = write!(header, "\n> {notice}\n\n## Description\n\n");
    let description = subject.description.trim();
    if description.is_empty() {
        header.push_str("(none)\n\n");
    } else {
        let _ = write!(header, "{description}\n\n");
    }
    header
}

/// Formats a moment as `mm:ss`, or `h:mm:ss` from one hour on.
fn timestamp(seconds: u64) -> String {
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::{Cue, Transcript, TranscriptChapter, TranscriptSubject, timestamp};
    use crate::types::TranscriptSource;

    fn cue(start: f64, end: f64, text: &str) -> Cue {
        Cue::from_seconds(start, end, text).expect("valid cue")
    }

    fn chapter(start_seconds: u64, title: &str) -> TranscriptChapter {
        TranscriptChapter {
            start_seconds,
            title: title.to_owned(),
        }
    }

    fn subject(description: &str) -> TranscriptSubject<'_> {
        TranscriptSubject {
            title: Some("Me at the zoo"),
            reference: "youtube:jNQXAC9IVRw",
            channel: Some("jawed"),
            published: Some("2005-04-24T03:31:52Z"),
            duration_seconds: Some(19),
            description,
        }
    }

    fn moment(seconds: u64) -> String {
        format!("https://v.test/watch?v=x&t={seconds}s")
    }

    /// Returns the transcript section's lines: chapter headings and paragraph openings.
    fn body(
        source: TranscriptSource,
        cues: Vec<Cue>,
        chapters: &[TranscriptChapter],
    ) -> Vec<String> {
        let rendered = Transcript::new("en", source, cues).render(&subject(""), chapters, &moment);
        let (_, body) = rendered
            .markdown
            .split_once("## Transcript\n")
            .expect("transcript section");
        body.lines()
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn cues_need_text_and_ordered_finite_non_negative_times() {
        let checked = [
            Cue::from_seconds(1.2, 3.36, "  elephants ")
                .map(|cue| (cue.start_ms, cue.end_ms, cue.text)),
            Cue::from_seconds(2.0, 2.0, "instant").map(|cue| (cue.start_ms, cue.end_ms, cue.text)),
        ];
        let rejected = [
            Cue::from_seconds(1.0, 2.0, " \n "),
            Cue::from_seconds(-0.5, 2.0, "early"),
            Cue::from_seconds(f64::NAN, 2.0, "unknown"),
            Cue::from_seconds(1.0, f64::INFINITY, "endless"),
            Cue::from_seconds(3.0, 2.0, "backwards"),
        ]
        .map(|cue| cue.is_err());

        assert_eq!(
            checked,
            [
                Ok((1200, 3360, "elephants".to_owned())),
                Ok((2000, 2000, "instant".to_owned()))
            ]
        );
        assert_eq!(rejected, [true; 5]);
    }

    #[test]
    fn moments_read_as_minutes_and_seconds_and_hours_from_one_hour() {
        let formatted = [0, 59, 65, 3599, 3600, 3723, 36_000].map(timestamp);

        assert_eq!(
            formatted,
            [
                "00:00", "00:59", "01:05", "59:59", "1:00:00", "1:02:03", "10:00:00"
            ]
        );
    }

    #[test]
    fn a_zoo_transcript_renders_its_header_description_chapters_and_moment_links() {
        let cues = vec![
            cue(
                1.2,
                3.36,
                "All right, so here we are, in front of the elephants",
            ),
            cue(
                5.318,
                7.974,
                "the cool thing about these guys is that they have really...",
            ),
            cue(7.974, 12.616, "really really long trunks"),
            cue(12.616, 14.367, "and that's cool"),
            cue(14.421, 15.733, "(baaaaaaaaaaahhh!!)"),
            cue(16.881, 18.881, "and that's pretty much all there is to say"),
        ];
        let chapters = [
            chapter(0, "Intro"),
            chapter(5, "The cool thing"),
            chapter(14, "End"),
        ];

        let rendered = Transcript::new("en", TranscriptSource::Manual, cues).render(
            &subject("The first video.\n\n00:00 Intro\n00:05 The cool thing\n00:14 End"),
            &chapters,
            &moment,
        );

        assert_eq!(
            rendered.markdown,
            "# Me at the zoo\n\n\
             - Ref: `youtube:jNQXAC9IVRw`\n\
             - Channel: jawed\n\
             - Published: 2005-04-24T03:31:52Z\n\
             - Duration: 00:19\n\
             - Transcript: `en`, captions the publisher uploaded\n\n\
             > This document holds only caption text; nothing here describes the video's picture or its non-speech sounds.\n\n\
             ## Description\n\n\
             The first video.\n\n00:00 Intro\n00:05 The cool thing\n00:14 End\n\n\
             ## Transcript\n\n\
             ### Intro\n\n\
             [00:01](https://v.test/watch?v=x&t=1s) All right, so here we are, in front of the elephants\n\n\
             ### The cool thing\n\n\
             [00:05](https://v.test/watch?v=x&t=5s) the cool thing about these guys is that they have really... really really long trunks and that's cool\n\n\
             ### End\n\n\
             [00:14](https://v.test/watch?v=x&t=14s) (baaaaaaaaaaahhh!!) and that's pretty much all there is to say\n"
        );
        assert_eq!(
            (
                rendered.info.language.as_str(),
                rendered.info.source,
                rendered.info.cues,
                rendered.info.paragraphs
            ),
            ("en", TranscriptSource::Manual, 6, 3)
        );
    }

    #[test]
    fn asr_paragraphs_break_after_about_45_seconds_without_waiting_for_punctuation() {
        let cues = (0..12)
            .map(|index| {
                let start = f64::from(index) * 5.0;
                cue(start, start + 5.0, &format!("word{index}"))
            })
            .collect();

        assert_eq!(
            body(TranscriptSource::Asr, cues, &[]),
            [
                "[00:00](https://v.test/watch?v=x&t=0s) word0 word1 word2 word3 word4 word5 word6 word7 word8",
                "[00:45](https://v.test/watch?v=x&t=45s) word9 word10 word11",
            ]
        );
    }

    #[test]
    fn punctuated_paragraphs_wait_for_a_sentence_end_up_to_a_hard_limit() {
        let sentences = (0..12)
            .map(|index| {
                let start = f64::from(index) * 5.0;
                let text = if index == 10 { "ends here." } else { "and on" };
                cue(start, start + 5.0, text)
            })
            .collect();
        let unpunctuated = (0..20)
            .map(|index| {
                let start = f64::from(index) * 5.0;
                cue(start, start + 5.0, "on")
            })
            .collect();

        let waited = body(TranscriptSource::Model, sentences, &[]);
        let capped = body(TranscriptSource::Manual, unpunctuated, &[]);

        assert_eq!(
            waited.iter().map(|line| &line[..6]).collect::<Vec<_>>(),
            ["[00:00", "[00:55"]
        );
        assert_eq!(
            capped.iter().map(|line| &line[..6]).collect::<Vec<_>>(),
            ["[00:00", "[01:30"]
        );
    }

    #[test]
    fn a_pause_of_two_seconds_breaks_a_paragraph() {
        let cues = vec![
            cue(0.0, 1.0, "one"),
            cue(2.9, 3.5, "two"),
            cue(5.5, 6.0, "three"),
        ];

        assert_eq!(
            body(TranscriptSource::Asr, cues, &[]),
            [
                "[00:00](https://v.test/watch?v=x&t=0s) one two",
                "[00:05](https://v.test/watch?v=x&t=5s) three",
            ]
        );
    }

    #[test]
    fn cues_join_without_a_space_only_between_scripts_that_write_without_spaces() {
        let cues = vec![
            cue(0.0, 1.0, "各位同学大家好。"),
            cue(1.0, 2.0, "这个十一你出门了吗？"),
            cue(2.0, 3.0, "東京へ"),
            cue(3.0, 4.0, "行きます"),
            cue(4.0, 5.0, "GPT"),
            cue(5.0, 6.0, "模型"),
            cue(6.0, 7.0, "안녕하세요"),
            cue(7.0, 8.0, "여러분"),
        ];

        assert_eq!(
            body(TranscriptSource::Model, cues, &[]),
            [
                "[00:00](https://v.test/watch?v=x&t=0s) 各位同学大家好。这个十一你出门了吗？東京へ行きます GPT 模型 안녕하세요 여러분"
            ]
        );
    }

    #[test]
    fn a_model_transcript_says_it_is_not_verbatim() {
        let rendered = Transcript::new(
            "de",
            TranscriptSource::Model,
            vec![cue(0.0, 1.0, "Hallo.")],
        )
        .render(&subject(""), &[], &moment);

        assert!(rendered.markdown.contains(
            "- Transcript: `de`, generated by a model that watched and listened to the video\n\n> A model generated this transcript; it is not guaranteed to be verbatim.\n\n## Description\n\n(none)\n"
        ));
    }
}
