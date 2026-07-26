//! CLI parsing and application dispatch.

use clap::{Parser, Subcommand};

use crate::config::{ConfigError, ConfigLocation};

/// Parsed `forager` command-line arguments.
#[derive(Debug, Parser)]
#[command(name = "forager", infer_subcommands = false)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    Path,
}

/// Executes a parsed command and returns its stdout payload.
///
/// # Errors
///
/// Returns [`ConfigError`] when the configuration location cannot be resolved.
pub fn run(cli: Cli) -> Result<String, ConfigError> {
    match cli.command {
        Command::Config {
            command: ConfigCommand::Path,
        } => Ok(ConfigLocation::discover()?
            .config_file()
            .display()
            .to_string()),
    }
}
