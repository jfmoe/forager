//! CLI parsing and application dispatch.

mod args;
mod browser;
mod dispatch;
mod doctor;
mod invocation;
mod platform;

pub use crate::telemetry::Invocation;
pub use args::{Cli, CommandKind, ContentOutputFormat, OutputFormat, OutputTarget};
pub use dispatch::{
    AppError, CommandOutput, ExaOutcome, Presentation, ProviderError, SearchFailure,
    bounded_attempt_summary, combine_diagnostics, run,
};
pub use invocation::{RetrievalCommand, record_invocation};

/// The names each report vocabulary that the product skill lists serializes to, by vocabulary and
/// in the skill's order, so the skill contract test can bind each list to its enum.
#[must_use]
pub fn report_vocabularies() -> Vec<(&'static str, Vec<String>)> {
    crate::health::vocabularies()
        .into_iter()
        .chain(crate::live::vocabularies())
        .collect()
}
