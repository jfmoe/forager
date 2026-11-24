//! Polymarket support for the command contract tests: configuration for the endpoints of
//! `polymarket_api` and a runner for its commands.

use std::process::Output;

use super::RunEnvironment;

/// Points `polymarket_api` at the two fixture servers with one attempt per request, so a
/// failure response is terminal at once.
pub(crate) fn config(gamma: &str, clob: &str) -> String {
    format!(
        "[providers.polymarket_api]\nurl = \"{gamma}\"\nclob_url = \"{clob}\"\n\n[retry]\nmax_attempts = 1\n"
    )
}

/// Points `polymarket_api` at a Gamma and a Data API fixture with one attempt per request; the
/// CLOB keeps its default, which history never reaches.
pub(crate) fn history_config(gamma: &str, data: &str) -> String {
    format!(
        "[providers.polymarket_api]\nurl = \"{gamma}\"\ndata_url = \"{data}\"\n\n[retry]\nmax_attempts = 1\n"
    )
}

/// Runs `forager platform polymarket <arguments>`.
pub(crate) fn run(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "polymarket"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}
