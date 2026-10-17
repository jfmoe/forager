//! The transcript document of a YouTube video, shared by every route that produces one: the
//! item's header fields, its chapters, and moment links into the watch page.
//!
//! A producer passes the video item it read and the description its document carries; this
//! module binds them to the platform-neutral `providers/transcript` renderer.

use super::transcript::{RenderedTranscript, Transcript, TranscriptChapter, TranscriptSubject};
use crate::types::{
    PlatformItem, PlatformItemData, PlatformRef, YoutubeChapter, YoutubeItemData, YoutubeRef,
};

/// A YouTube video item with its YouTube fields at hand.
#[derive(Clone, Copy)]
pub(super) struct YoutubeVideo<'a> {
    item: &'a PlatformItem,
    reference: &'a YoutubeRef,
    pub(super) data: &'a YoutubeItemData,
}

impl<'a> YoutubeVideo<'a> {
    /// Views an item that a YouTube route read or a YouTube precheck delivered.
    pub(super) fn of(item: &'a PlatformItem) -> Self {
        let (PlatformRef::Youtube(reference), PlatformItemData::Youtube(data)) =
            (&item.reference, &item.data)
        else {
            unreachable!("only YouTube routes read or are handed YouTube videos")
        };
        Self {
            item,
            reference,
            data,
        }
    }

    /// Renders `transcript` under this video's header, with the description and chapters the
    /// document carries; each paragraph links to its moment of the video.
    pub(super) fn render(
        self,
        transcript: &Transcript,
        description: &str,
        chapters: &[YoutubeChapter],
    ) -> RenderedTranscript {
        let chapters = chapters
            .iter()
            .map(|chapter| TranscriptChapter {
                start_seconds: chapter.start_seconds,
                title: chapter.title.clone(),
            })
            .collect::<Vec<_>>();
        let reference = self.item.reference.to_string();
        let subject = TranscriptSubject {
            title: self.item.title.as_deref(),
            reference: &reference,
            channel: self.item.authors.first().map(String::as_str),
            published: self.item.published.as_deref(),
            duration_seconds: self.data.duration_seconds,
            description,
        };
        transcript.render(&subject, &chapters, &|seconds| {
            self.reference.moment_url(seconds)
        })
    }
}
