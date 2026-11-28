//! What a finished command leaves for Invocation Telemetry, and the write the binary's terminal
//! function asks for once the exit code and the `--output` copy are settled.

use chrono::Utc;

use super::dispatch::{AppError, CommandOutput};
use crate::telemetry::{self, Invocation, LocalKind, ResultSignal, TerminalKind};
use crate::types::{
    AttemptErrorKind, PlatformAskResult, PlatformCommentsPage, PlatformFetchResult,
    ProviderAttempt, ProviderError,
};

pub use crate::telemetry::RetrievalCommand;

impl CommandOutput {
    /// What this output leaves for the telemetry record of its command.
    #[must_use]
    pub fn invocation(&self) -> Invocation {
        match self {
            Self::Text { .. } => Invocation::default(),
            Self::Failure { kind, .. } => Invocation::failed(&[], *kind),
            Self::SearchPreflight { error, .. } => error.invocation(),
            Self::Search { result, .. } => match result {
                Ok(outcome) => Invocation::succeeded(
                    &outcome.attempts,
                    outcome.provider,
                    ResultSignal::content(&outcome.answer).with_items(outcome.sources.len()),
                ),
                Err(failure) => failed(&failure.error),
            },
            Self::Exa { result, .. } => answered(result, |outcome| {
                (
                    &outcome.attempts,
                    outcome.provider,
                    ResultSignal::items(outcome.results.len()),
                )
            }),
            Self::Fetch { result, host, .. } => answered(result, |outcome| {
                (
                    &outcome.attempts,
                    outcome.provider,
                    ResultSignal::content(&outcome.content),
                )
            })
            .with_host(host.clone()),
            Self::Map { result, .. } => answered(result, |outcome| {
                (
                    &outcome.attempts,
                    outcome.provider,
                    ResultSignal::items(outcome.results.len()),
                )
            }),
            Self::PlatformSearch { result, .. } => answered(result, |page| {
                (
                    &page.attempts,
                    page.provider,
                    ResultSignal::items(page.items.len()),
                )
            }),
            Self::PlatformFetch { result, .. } => answered(result, fetched),
            Self::PlatformComments { result, .. } => answered(result, |page| match page {
                PlatformCommentsPage::Xiaohongshu(page) => (
                    &page.attempts,
                    page.provider,
                    ResultSignal::items(page.comments.len()),
                ),
                PlatformCommentsPage::Youtube(page) => (
                    &page.attempts,
                    page.provider,
                    ResultSignal::items(page.comments.len()),
                ),
            }),
            Self::PlatformAsk { result, .. } => answered(result, |answer| match answer {
                PlatformAskResult::X(answer) => (
                    &answer.attempts,
                    answer.provider,
                    ResultSignal::content(&answer.answer),
                ),
                PlatformAskResult::Youtube(answer) => (
                    &answer.attempts,
                    answer.provider,
                    ResultSignal::content(&answer.answer),
                ),
            }),
            Self::PlatformTranscribe { result, .. } => {
                answered(result, |transcribed| fetched(&transcribed.fetched))
            }
            Self::HnStories { result, .. } => answered(result, |listing| {
                (
                    &listing.attempts,
                    listing.provider,
                    ResultSignal::items(listing.items.len()),
                )
            }),
            Self::PolymarketHistory { result, .. } => answered(result, |history| {
                (
                    &history.attempts,
                    history.provider,
                    ResultSignal::items(history.points.len()),
                )
            }),
        }
    }
}

impl AppError {
    /// What a command that ended with this error before its result leaves for its record.
    #[must_use]
    pub fn invocation(&self) -> Invocation {
        Invocation::preflight(match self.exit_code() {
            2 => TerminalKind::Local(LocalKind::Argument),
            3 => TerminalKind::Local(LocalKind::Config),
            _ => TerminalKind::Attempt(AttemptErrorKind::Runtime),
        })
    }
}

/// The invocation of a successful `result`, from its attempts, the provider that answered and
/// the shape of the answer, or of its failure.
fn answered<T>(
    result: &Result<T, ProviderError>,
    answer: impl FnOnce(&T) -> (&[ProviderAttempt], &'static str, ResultSignal),
) -> Invocation {
    match result {
        Ok(value) => {
            let (attempts, provider, signal) = answer(value);
            Invocation::succeeded(attempts, provider, signal)
        }
        Err(error) => failed(error),
    }
}

fn failed(error: &ProviderError) -> Invocation {
    Invocation::failed(&error.attempts, error.kind)
}

fn fetched(fetched: &PlatformFetchResult) -> (&[ProviderAttempt], &'static str, ResultSignal) {
    let signal = fetched
        .content
        .as_ref()
        .map_or_else(ResultSignal::default, |content| {
            ResultSignal::content(&content.text)
        });
    (&fetched.attempts, fetched.provider, signal)
}

/// Writes the telemetry record of one invocation of `command` that ended with `exit_code`,
/// unless the configuration turns telemetry off or cannot be read. A record that cannot be
/// written is dropped: telemetry never changes a command's output or exit code.
pub fn record_invocation(command: RetrievalCommand, invocation: Invocation, exit_code: u8) {
    if !crate::config::telemetry_enabled().unwrap_or(false) {
        return;
    }
    let Some(directory) = telemetry::directory() else {
        return;
    };
    let _ = telemetry::record(&directory, command, invocation, exit_code, Utc::now());
}
