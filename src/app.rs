//! CLI parsing and application dispatch.

use std::io::{self, Read};

use clap::{Parser, Subcommand};
use thiserror::Error;

use crate::config::{self, ConfigError, ConfigLocation, EditError};

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
    #[command(visible_alias = "ls")]
    List,
    Set {
        key: String,
        #[arg(
            help = "TOML value, or - for standard input; argument values may be saved in shell history"
        )]
        value: String,
    },
    Unset {
        key: String,
    },
}

/// Text emitted by a successful command.
#[derive(Debug)]
pub struct CommandOutput {
    /// Standard output without an automatically appended newline.
    pub stdout: String,
    /// Optional diagnostic emitted on standard error.
    pub stderr: Option<String>,
}

/// Executes a parsed command.
///
/// # Errors
///
/// Returns [`AppError`] when arguments, configuration, standard input, or
/// persistence are invalid.
pub fn run(cli: Cli) -> Result<CommandOutput, AppError> {
    match cli.command {
        Command::Config {
            command: ConfigCommand::Path,
        } => Ok(CommandOutput {
            stdout: ConfigLocation::discover()?
                .config_file()
                .display()
                .to_string(),
            stderr: None,
        }),
        Command::Config {
            command: ConfigCommand::List,
        } => Ok(CommandOutput {
            stdout: config::effective_view_json()?,
            stderr: None,
        }),
        Command::Config {
            command: ConfigCommand::Set { key, value },
        } => {
            let value = if value == "-" { read_stdin()? } else { value };
            config::set_file_value(&key, &value)?;
            Ok(CommandOutput {
                stdout: String::new(),
                stderr: None,
            })
        }
        Command::Config {
            command: ConfigCommand::Unset { key },
        } => {
            let overridden = config::unset_file_value(&key)?;
            Ok(CommandOutput {
                stdout: String::new(),
                stderr: overridden
                    .then(|| format!("environment override for `{key}` is still effective")),
            })
        }
    }
}

fn read_stdin() -> Result<String, AppError> {
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .map_err(AppError::Stdin)?;
    if input.ends_with("\r\n") {
        input.truncate(input.len() - 2);
    } else if input.ends_with('\n') {
        input.pop();
    }
    Ok(input)
}

/// Application errors with stable exit categories.
#[derive(Debug, Error)]
pub enum AppError {
    /// A configuration could not be loaded or persisted.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// A set or unset operation failed.
    #[error(transparent)]
    Edit(#[from] EditError),
    /// Standard input could not be read.
    #[error("cannot read standard input: {0}")]
    Stdin(io::Error),
}

impl AppError {
    /// Returns the CLI exit status for this error.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Edit(EditError::Argument(_)) => 2,
            Self::Config(_) | Self::Edit(EditError::Config(_)) | Self::Stdin(_) => 3,
        }
    }

    /// Returns the stable human-readable error category.
    pub fn category(&self) -> &'static str {
        if self.exit_code() == 2 {
            "argument_error"
        } else {
            "config_error"
        }
    }
}
