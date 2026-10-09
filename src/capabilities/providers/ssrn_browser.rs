//! The `ssrn_browser` route: SSRN search results and paper pages, read in the user's own Chrome
//! through the forager OpenCLI adapter for `ssrn`.
//!
//! The JavaScript adapter applies search conditions and reads page facts. This route checks
//! the page identity and effective search state, then normalizes the facts into items.

use std::time::Duration;

use serde::Deserialize;

use crate::catalog::{PlatformOperation, ProviderId, ProviderTransport, registration};
use crate::config::ProcessRouteRuntimeConfig;
use crate::net::{AttemptFailure, RetryPolicy};
use crate::providers::execution::{ExecutionSettings, execute_anonymous};
use crate::providers::opencli::{self, EnvelopeStatus, OpenCliCommand, Window};
use crate::providers::shared::{other_platform_message, parameter_error};
use crate::rate_limit::RateLimiter;
use crate::types::{
    AttemptErrorKind, AttemptTarget, ContentDepth, Deadline, FullTextSource, LocalFile,
    LocalMediaType, Platform, PlatformFetchOutcome, PlatformFetchRequest, PlatformItem,
    PlatformItemData, PlatformRef, ProviderError, SsrnItemData, SsrnRef,
};

const ROUTE: ProviderId = ProviderId::SsrnBrowser;
#[path = "ssrn_browser_search.rs"]
mod search;

pub(crate) use search::search_support;

/// Returns whether the route can fetch at the requested depth; it never starts a process.
pub(crate) fn fetch_support(request: &PlatformFetchRequest) -> Result<(), String> {
    match request.depth {
        ContentDepth::Metadata | ContentDepth::Abstract | ContentDepth::FullText => Ok(()),
        depth => Err(format!(
            "{} cannot fetch at depth `{}`",
            ROUTE.name(),
            depth.as_str()
        )),
    }
}

pub(crate) struct SsrnBrowser {
    config: ProcessRouteRuntimeConfig,
    limiter: RateLimiter,
    deadline: Deadline,
}

impl SsrnBrowser {
    pub(crate) fn new(
        config: ProcessRouteRuntimeConfig,
        limiter: RateLimiter,
        deadline: Deadline,
    ) -> Self {
        Self {
            config,
            limiter,
            deadline,
        }
    }

    /// Reads the paper page and checks that it shows the requested paper. At full-text depth
    /// the same command also downloads the PDF, and the attempt requires the download to be
    /// complete, the file to exist and start with `%PDF-`, and the page id to match; any
    /// failure of these checks is Quality.
    pub(crate) async fn fetch(
        &self,
        request: &PlatformFetchRequest,
    ) -> Result<PlatformFetchOutcome, ProviderError> {
        let PlatformRef::Ssrn(requested) = &request.reference else {
            return Err(parameter_error(other_platform_message(
                ROUTE,
                request.reference.platform(),
            )));
        };
        let full_text = request.depth == ContentDepth::FullText;
        let mut options = vec![("id", requested.id().to_owned())];
        if full_text {
            options.push(("download", "true".to_owned()));
        }
        let command = self.command("paper", options);
        let command = &command;
        let execution = execute_anonymous(
            self.settings(PlatformOperation::Fetch),
            move |deadline| async move {
                let envelope = opencli::run::<PaperPage>(command, &self.limiter, deadline).await?;
                if full_text {
                    let file = read_download(envelope.data.download.as_ref()).await?;
                    let item = read_paper(
                        envelope.status,
                        &envelope.data,
                        requested,
                        AttemptErrorKind::Quality,
                    )?;
                    return Ok((None, (item, FullTextSource::LocalFile(file))));
                }
                let item = read_paper(
                    envelope.status,
                    &envelope.data,
                    requested,
                    AttemptErrorKind::Runtime,
                )?;
                if request.depth == ContentDepth::Abstract && item.depth != ContentDepth::Abstract {
                    return Err(AttemptFailure {
                        kind: AttemptErrorKind::Quality,
                        status: None,
                        message: format!("the SSRN page has no abstract for ssrn:{requested}"),
                    });
                }
                Ok((None, (item, FullTextSource::Urls(Vec::new()))))
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

    fn command(
        &self,
        command: &'static str,
        options: Vec<(&'static str, String)>,
    ) -> OpenCliCommand<'_> {
        let ProviderTransport::OpenCli(adapter) = registration(ROUTE).transport else {
            unreachable!("ssrn_browser registers an OpenCLI transport");
        };
        OpenCliCommand {
            executable: &self.config.command,
            adapter,
            command,
            options,
            window: Window::Background,
        }
    }

    // The route never retries: a failed browser operation falls through to the next route.
    fn settings(&self, operation: PlatformOperation) -> ExecutionSettings {
        ExecutionSettings {
            provider: ROUTE.name(),
            target: AttemptTarget::platform(Platform::Ssrn.as_str(), operation.as_str()),
            retry_policy: RetryPolicy::new(1, 1.0, Duration::ZERO),
            deadline: self.deadline,
            attempt_timeout: Duration::from_secs(self.config.timeout_seconds),
            verbose: false,
            timeout_message: "OpenCLI command timed out",
            model: None,
            transport: Some("process"),
            endpoint_host: None,
            breaker_event: None,
        }
    }
}

fn runtime(message: String) -> AttemptFailure {
    AttemptFailure {
        kind: AttemptErrorKind::Runtime,
        status: None,
        message,
    }
}

/// The facts the adapter reads from a paper page. For `no_results`, the adapter found the
/// site's notice that the paper is unavailable, in `notice`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct PaperPage {
    url: String,
    canonical_url: Option<String>,
    doi: Option<String>,
    title: String,
    authors: Vec<String>,
    abstract_paragraphs: Vec<String>,
    /// The note line parts, such as `37 Pages`, `Posted: 19 Apr 2012`.
    notes: Vec<String>,
    /// The line such as `Date Written: October 1, 2016`.
    date_written: Option<String>,
    notice: Option<String>,
    /// The download the adapter completed when asked to; never carries the download URL.
    download: Option<DownloadFact>,
}

/// The download outcome the adapter reports: the browser's download state and the local file.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct DownloadFact {
    state: String,
    file: Option<String>,
}

/// Checks the file the adapter downloaded: the download completed with a file name, the file
/// can be read, and it starts with the PDF magic. Every failure is Quality: the browser could
/// not deliver the paper body in this attempt.
async fn read_download(download: Option<&DownloadFact>) -> Result<LocalFile, AttemptFailure> {
    let quality = |message: String| AttemptFailure {
        kind: AttemptErrorKind::Quality,
        status: None,
        message,
    };
    let state = download.map_or("none", |fact| fact.state.as_str());
    let path = download
        .and_then(|fact| fact.file.as_deref())
        .filter(|path| !path.is_empty());
    let Some(path) = (state == "complete").then_some(path).flatten() else {
        return Err(quality(format!(
            "the SSRN download did not complete with a file (state: {state})"
        )));
    };
    let path = std::path::PathBuf::from(path);
    let unreadable = |error: std::io::Error| {
        quality(format!(
            "the downloaded file {} cannot be read: {error}",
            path.display()
        ))
    };
    let mut file = tokio::fs::File::open(&path).await.map_err(unreadable)?;
    let mut header = [0_u8; 5];
    tokio::io::AsyncReadExt::read_exact(&mut file, &mut header)
        .await
        .map_err(unreadable)?;
    if &header != b"%PDF-" {
        return Err(quality(format!(
            "the downloaded file {} is not a PDF",
            path.display()
        )));
    }
    Ok(LocalFile {
        path,
        media_type: LocalMediaType::Pdf,
    })
}

fn read_paper(
    status: EnvelopeStatus,
    page: &PaperPage,
    requested: &SsrnRef,
    id_check: AttemptErrorKind,
) -> Result<PlatformItem, AttemptFailure> {
    if status == EnvelopeStatus::NoResults {
        return Err(AttemptFailure {
            kind: AttemptErrorKind::Parameter,
            status: None,
            message: format!(
                "SSRN paper not available: ssrn:{requested} ({})",
                page.notice.as_deref().map_or("no notice", str::trim)
            ),
        });
    }
    let shown = page
        .canonical_url
        .as_deref()
        .and_then(|url| SsrnRef::parse(url).ok())
        .or_else(|| page.doi.as_deref().and_then(SsrnRef::from_doi))
        .ok_or_else(|| AttemptFailure {
            kind: id_check,
            status: None,
            message: format!("the page `{}` shows no SSRN abstract ID", page.url),
        })?;
    if &shown != requested {
        return Err(AttemptFailure {
            kind: id_check,
            status: None,
            message: format!("the SSRN page shows ssrn:{shown} for ssrn:{requested}"),
        });
    }
    let title = fold_whitespace(&page.title);
    if title.is_empty() {
        return Err(runtime(format!(
            "the SSRN page for ssrn:{requested} has no title"
        )));
    }
    let paragraphs = page
        .abstract_paragraphs
        .iter()
        .map(|paragraph| fold_whitespace(paragraph))
        .filter(|paragraph| !paragraph.is_empty())
        .collect::<Vec<_>>();
    let abstract_text = (!paragraphs.is_empty()).then(|| paragraphs.join("\n\n"));
    let posted = labeled_value(page.notes.iter().map(String::as_str), "Posted:");
    Ok(PlatformItem {
        url: requested.canonical_url(),
        reference: PlatformRef::Ssrn(requested.clone()),
        depth: if abstract_text.is_some() {
            ContentDepth::Abstract
        } else {
            ContentDepth::Metadata
        },
        title,
        authors: names(&page.authors),
        published: posted.as_deref().and_then(iso_date),
        data: PlatformItemData::Ssrn(SsrnItemData {
            abstract_text,
            last_revised: labeled_value(page.notes.iter().map(String::as_str), "Last revised:"),
            date_written: page
                .date_written
                .as_deref()
                .and_then(|line| labeled_value([line], "Date Written:")),
            posted,
            ..SsrnItemData::default()
        }),
    })
}

fn names(authors: &[String]) -> Vec<String> {
    authors
        .iter()
        .map(|author| fold_whitespace(author))
        .filter(|author| !author.is_empty())
        .collect()
}

/// Returns the text after `label` in the first part that starts with it, as the page shows it.
fn labeled_value<'a>(parts: impl IntoIterator<Item = &'a str>, label: &str) -> Option<String> {
    parts.into_iter().find_map(|part| {
        let value = fold_whitespace(part.trim().strip_prefix(label)?);
        (!value.is_empty()).then_some(value)
    })
}

/// Converts an SSRN page date such as `08 Sep 2019` to `2019-09-08`, or a lone year to itself.
fn iso_date(value: &str) -> Option<String> {
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    fn year(text: &str) -> Option<&str> {
        (text.len() == 4 && text.bytes().all(|byte| byte.is_ascii_digit())).then_some(text)
    }
    let parts = value.split_whitespace().collect::<Vec<_>>();
    match parts[..] {
        [only] => year(only).map(str::to_owned),
        [day, month, year_text] => {
            let day = day
                .parse::<u8>()
                .ok()
                .filter(|day| (1..=31).contains(day))?;
            let month = month.get(..3)?.to_ascii_lowercase();
            let month = MONTHS.iter().position(|name| *name == month)? + 1;
            Some(format!("{}-{month:02}-{day:02}", year(year_text)?))
        }
        _ => None,
    }
}

fn fold_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::iso_date;
    #[test]
    fn page_dates_become_iso_dates_at_their_precision() {
        let dates = [
            "08 Sep 2019",
            "5 Jul 2000",
            "1994",
            "Sept 2019",
            "31 Foo 2019",
        ]
        .map(iso_date);

        assert_eq!(
            dates,
            [
                Some("2019-09-08".to_owned()),
                Some("2000-07-05".to_owned()),
                Some("1994".to_owned()),
                None,
                None
            ]
        );
    }
}
