//! CLI parsing and application dispatch.

mod args;
mod dispatch;

pub use args::{Cli, DocsOutputFormat, OutputFormat, OutputTarget};
pub use dispatch::{
    AppError, CommandOutput, ExaOutcome, ProviderError, ResearchFailure, ResearchTerminal,
    SearchFailure, bounded_attempt_summary, combine_diagnostics, run,
};
