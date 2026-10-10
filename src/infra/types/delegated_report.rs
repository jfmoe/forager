//! The Delegated Research Report a third-party research agent delivers, with the sources it
//! cites. It is not Research Evidence (ADR 0023). Every provider that delivers one shares these
//! shapes, so the report file, the sources file, and the `content` output have one layout.

use std::fmt::Write as _;

use serde::Serialize;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
/// A source the report cites as `[N]`.
pub struct ReportSource {
    pub id: u64,
    /// `None` when the research agent named no title for the source.
    pub title: Option<String>,
    pub url: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// A completed Delegated Research Report: its Markdown body and the sources it cites, ordered
/// by citation number.
pub struct DelegatedReport {
    pub title: String,
    pub body: String,
    pub sources: Vec<ReportSource>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Where a `research result` command wrote a completed report and its sources.
pub struct ReportFiles {
    pub report_path: String,
    pub sources_path: String,
}

impl DelegatedReport {
    /// The body, verbatim, followed by a `## Sources` list that resolves every `[N]` without
    /// the research agent.
    #[must_use]
    pub fn markdown(&self) -> String {
        let mut markdown = self.body.clone();
        if !markdown.ends_with('\n') {
            markdown.push('\n');
        }
        markdown.push_str("\n## Sources\n");
        for source in &self.sources {
            let _ = write!(markdown, "\n- [{}] ", source.id);
            if let Some(title) = &source.title {
                let _ = write!(markdown, "{title} ");
            }
            let _ = write!(markdown, "<{}>", source.url);
        }
        markdown.push('\n');
        markdown
    }

    /// The length of the body in characters, without the sources list.
    #[must_use]
    pub fn content_len(&self) -> usize {
        self.body.chars().count()
    }
}

#[cfg(test)]
mod tests {
    use super::{DelegatedReport, ReportSource};

    #[test]
    fn markdown_keeps_the_body_verbatim_before_the_sources() {
        let report = DelegatedReport {
            title: "Title".into(),
            body: "Body  \n\n".into(),
            sources: vec![
                ReportSource {
                    id: 1,
                    title: Some("Source".into()),
                    url: "https://example.test".into(),
                },
                ReportSource {
                    id: 2,
                    title: None,
                    url: "https://example.test/untitled".into(),
                },
            ],
        };

        assert_eq!(
            report.markdown(),
            "Body  \n\n\n## Sources\n\n- [1] Source <https://example.test>\n- [2] <https://example.test/untitled>\n"
        );
    }
}
