//! The `forager browser` command group: installing the Managed Browser and logging a user into
//! a platform's Browser Profile. Results go to stdout as one JSON object, progress and prompts
//! to stderr; the flows themselves are in `ops/browser_setup`.

use std::time::Duration;

use clap::Subcommand;
use serde::Serialize;

use super::dispatch::{AppError, CommandOutput, NetworkDependencies};
use crate::browser_setup::{self, SetupFailure};
use crate::config::ConfigError;
use crate::types::{Deadline, Platform};

#[derive(Clone, Copy, Debug, Subcommand)]
pub(super) enum BrowserCommand {
    /// Download the pinned Managed Browser, verify its SHA256, and install it into the cache.
    Install {
        #[arg(long, default_value_t = 600, value_parser = clap::value_parser!(u64).range(1..))]
        timeout: u64,
    },
    /// Open a browser window on the platform's login page and wait until you have logged in.
    Login {
        /// The platform whose Browser Profile to log into.
        #[arg(value_parser = parse_platform)]
        platform: Platform,
        /// Seconds to wait for the login.
        #[arg(long, default_value_t = 600, value_parser = clap::value_parser!(u64).range(10..))]
        timeout: u64,
    },
}

pub(super) fn parse_platform(value: &str) -> Result<Platform, String> {
    Platform::ALL
        .into_iter()
        .find(|platform| platform.as_str() == value)
        .ok_or_else(|| {
            format!(
                "unknown platform `{value}`; expected one of: {}",
                Platform::ALL.map(Platform::as_str).join(", ")
            )
        })
}

pub(super) fn run(command: BrowserCommand) -> Result<CommandOutput, AppError> {
    match command {
        BrowserCommand::Install { timeout } => install(timeout),
        BrowserCommand::Login { platform, timeout } => login(platform, timeout),
    }
}

fn install(timeout: u64) -> Result<CommandOutput, AppError> {
    let dependencies = NetworkDependencies::load()?;
    if let Some(path) = &dependencies.config.browser.path {
        eprintln!(
            "browser.path is set, so browser commands launch {} instead of the Managed Browser",
            path.display()
        );
    }
    let deadline = Deadline::new(Duration::from_secs(timeout));
    let installed = dependencies.runtime.block_on(browser_setup::install(
        &dependencies.client,
        deadline,
        &mut |line| eprintln!("{line}"),
    ));
    finish(installed)
}

fn login(platform: Platform, timeout: u64) -> Result<CommandOutput, AppError> {
    let login = browser_setup::platform_login(platform).map_err(AppError::Argument)?;
    let dependencies = NetworkDependencies::load()?;
    let logged_in = dependencies.runtime.block_on(browser_setup::login(
        &login,
        &dependencies.config.browser,
        Duration::from_secs(timeout),
        &mut |line| eprintln!("{line}"),
    ));
    finish(logged_in)
}

/// Renders a setup result: the result on stdout, a missing browser as a preflight config
/// error, and any other failure as the thin failure payload the binary renders.
fn finish(result: Result<impl Serialize, SetupFailure>) -> Result<CommandOutput, AppError> {
    match result {
        Ok(value) => Ok(CommandOutput::Text {
            stdout: serde_json::to_string(&value)
                .map_err(|error| AppError::Runtime(error.to_string()))?,
            stderr: None,
            exit_code: 0,
        }),
        Err(SetupFailure::Config(message)) => Err(AppError::Config(ConfigError::Message(message))),
        Err(SetupFailure::Failed(failure)) => Ok(CommandOutput::Failure {
            kind: failure.kind,
            message: failure.message,
        }),
    }
}
