//! CLI parsing and application dispatch.

mod args;
mod browser;
mod dispatch;
mod invocation;
mod platform;

pub use crate::telemetry::Invocation;
pub use args::{Cli, CommandKind, ContentOutputFormat, OutputFormat, OutputTarget};
pub use dispatch::{
    AppError, CommandOutput, ExaOutcome, ProviderError, SearchFailure, bounded_attempt_summary,
    combine_diagnostics, run,
};
pub use invocation::{RetrievalCommand, record_invocation};
