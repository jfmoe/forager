//! CLI parsing and application dispatch.

use std::io::{self, BufRead, IsTerminal, Read, Write};

use clap::{Parser, Subcommand, ValueEnum};
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
    Setup {
        #[arg(long)]
        non_interactive: bool,
        #[arg(long, value_enum)]
        lang: Option<Language>,
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

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Language {
    Zh,
    En,
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
        Command::Setup {
            non_interactive: true,
            ..
        } => {
            let path = config::create_setup_template()?;
            Ok(CommandOutput {
                stdout: format!(
                    "Created {}. Run `forager doctor` to check the configuration.",
                    path.display()
                ),
                stderr: None,
            })
        }
        Command::Setup {
            non_interactive: false,
            lang,
        } => run_interactive_setup(lang),
    }
}

fn run_interactive_setup(language: Option<Language>) -> Result<CommandOutput, AppError> {
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let language = match language {
        Some(language) => language,
        None => choose_language(&mut input)?,
    };
    let mut document = config::SetupDocument::load()?;

    prompt_stage(language, 2, false);
    let current_backend = document.primary_backend().to_owned();
    let backend = choose_backend(&mut input, language, &current_backend)?;
    configure_model(&mut input, language, &mut document, &backend)?;

    let classifier_configured = document.classifier_is_configured();
    prompt_stage(language, 3, classifier_configured);
    configure_classifier(&mut input, language, &mut document, classifier_configured)?;

    prompt_stage(language, 4, false);
    for provider in [
        "exa",
        "context7",
        "jina",
        "tavily",
        "firecrawl",
        "anysearch",
    ] {
        let prompt = match language {
            Language::Zh => format!("{provider} keys（逗号分隔；回车跳过并保留现值）: "),
            Language::En => {
                format!("{provider} keys (comma-separated; Enter skips and preserves): ")
            }
        };
        if let Some(keys) = read_keys(&mut input, &prompt)? {
            document.set_strings(&format!("providers.{provider}.keys"), &keys)?;
        }
    }

    document.save()?;
    let stdout = match language {
        Language::Zh => "配置已保存。请运行 `forager doctor` 检查配置。".to_owned(),
        Language::En => "Configuration saved. Run `forager doctor` to check it.".to_owned(),
    };
    Ok(CommandOutput {
        stdout,
        stderr: None,
    })
}

fn choose_language(input: &mut impl BufRead) -> Result<Language, AppError> {
    loop {
        let value = prompt_line(input, "语言 / Language [zh/en]（默认 zh）: ")?;
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "zh" | "1" => return Ok(Language::Zh),
            "en" | "2" => return Ok(Language::En),
            _ => eprintln!("请输入 zh 或 en / Enter zh or en."),
        }
    }
}

fn prompt_stage(language: Language, stage: u8, classifier_configured: bool) {
    let label = match (language, stage, classifier_configured) {
        (Language::Zh, 2, _) => "第 2 步：主模型",
        (Language::Zh, 3, true) => "第 3 步：分类器（跳过会保留现值）",
        (Language::Zh, 3, false) => "第 3 步：分类器（跳过后无法自动路由或生成 research 计划）",
        (Language::Zh, 4, _) => "第 4 步：补强 provider",
        (Language::En, 2, _) => "Step 2: main model",
        (Language::En, 3, true) => "Step 3: classifier (skip preserves current values)",
        (Language::En, 3, false) => {
            "Step 3: classifier (skipping disables automatic routing and research plans)"
        }
        (Language::En, 4, _) => "Step 4: supplemental providers",
        _ => return,
    };
    eprintln!("{label}");
}

fn choose_backend(
    input: &mut impl BufRead,
    language: Language,
    current: &str,
) -> Result<String, AppError> {
    loop {
        let prompt = match language {
            Language::Zh => {
                format!("主模型 backend [xai/openai_compatible]（默认 {current}）: ")
            }
            Language::En => {
                format!("Main backend [xai/openai_compatible] (default {current}): ")
            }
        };
        let value = prompt_line(input, &prompt)?;
        match value.trim().to_ascii_lowercase().as_str() {
            "" => return Ok(current.to_owned()),
            "xai" | "1" => return Ok("xai".to_owned()),
            "openai_compatible" | "2" => return Ok("openai_compatible".to_owned()),
            _ => match language {
                Language::Zh => eprintln!("请输入 xai 或 openai_compatible。"),
                Language::En => eprintln!("Enter xai or openai_compatible."),
            },
        }
    }
}

fn configure_model(
    input: &mut impl BufRead,
    language: Language,
    document: &mut config::SetupDocument,
    backend: &str,
) -> Result<(), AppError> {
    let prefix = format!("providers.{backend}");
    let url_path = format!("{prefix}.url");
    let model_path = format!("{prefix}.model");
    let keys_path = format!("{prefix}.keys");
    let url = read_preserving(
        input,
        language,
        &format!("{backend} URL"),
        document.string(&url_path),
    )?;
    let keys_prompt = match language {
        Language::Zh => format!("{backend} keys（逗号分隔；回车保留现值）: "),
        Language::En => format!("{backend} keys (comma-separated; Enter preserves): "),
    };
    let keys = read_keys(input, &keys_prompt)?;
    let model = read_preserving(
        input,
        language,
        &format!("{backend} model"),
        document.string(&model_path),
    )?;

    document.set_string(&url_path, &url)?;
    if let Some(keys) = keys {
        document.set_strings(&keys_path, &keys)?;
    }
    document.set_string(&model_path, &model)?;
    Ok(())
}

fn configure_classifier(
    input: &mut impl BufRead,
    language: Language,
    document: &mut config::SetupDocument,
    configured: bool,
) -> Result<(), AppError> {
    let prompt = match (language, configured) {
        (Language::Zh, true) => "配置分类器？[Y/n]（跳过会保留现值）: ",
        (Language::Zh, false) => "配置分类器？[y/N]（跳过后无法自动路由或生成 research 计划）: ",
        (Language::En, true) => "Configure classifier? [Y/n] (skip preserves current values): ",
        (Language::En, false) => {
            "Configure classifier? [y/N] (skip disables automatic routing and research plans): "
        }
    };
    if !read_confirmation(input, prompt, configured, language)? {
        return Ok(());
    }

    let url = read_preserving(
        input,
        language,
        "classifier URL",
        document.string("classifier.url"),
    )?;
    let keys_prompt = match language {
        Language::Zh => "classifier keys（逗号分隔；回车保留现值）: ",
        Language::En => "classifier keys (comma-separated; Enter preserves): ",
    };
    let keys = read_keys(input, keys_prompt)?;
    let model = read_preserving(
        input,
        language,
        "classifier model",
        document.string("classifier.model"),
    )?;
    document.set_string("classifier.url", &url)?;
    if let Some(keys) = keys {
        document.set_strings("classifier.keys", &keys)?;
    }
    document.set_string("classifier.model", &model)?;
    Ok(())
}

fn read_confirmation(
    input: &mut impl BufRead,
    prompt: &str,
    default: bool,
    language: Language,
) -> Result<bool, AppError> {
    loop {
        let value = prompt_line(input, prompt)?;
        match value.trim().to_ascii_lowercase().as_str() {
            "" => return Ok(default),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => match language {
                Language::Zh => eprintln!("请输入 y 或 n。"),
                Language::En => eprintln!("Enter y or n."),
            },
        }
    }
}

fn read_preserving(
    input: &mut impl BufRead,
    language: Language,
    label: &str,
    current: &str,
) -> Result<String, AppError> {
    let prompt = match language {
        Language::Zh => format!("{label}（回车使用当前值）: "),
        Language::En => format!("{label} (Enter keeps the current value): "),
    };
    let value = prompt_line(input, &prompt)?;
    if value.is_empty() {
        Ok(current.to_owned())
    } else {
        Ok(value)
    }
}

fn read_keys(input: &mut impl BufRead, prompt: &str) -> Result<Option<Vec<String>>, AppError> {
    let value = read_secret(
        input,
        prompt,
        io::stdin().is_terminal(),
        rpassword::read_password,
    )?;
    if value.trim().is_empty() {
        return Ok(None);
    }
    let keys = value
        .split(',')
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    Ok(Some(keys))
}

fn read_secret(
    input: &mut impl BufRead,
    prompt: &str,
    terminal: bool,
    read_hidden: impl FnOnce() -> io::Result<String>,
) -> Result<String, AppError> {
    if terminal {
        eprint!("{prompt}");
        io::stderr().flush().map_err(AppError::Stdin)?;
        read_hidden().map_err(AppError::Stdin)
    } else {
        prompt_line(input, prompt)
    }
}

fn prompt_line(input: &mut impl BufRead, prompt: &str) -> Result<String, AppError> {
    eprint!("{prompt}");
    io::stderr().flush().map_err(AppError::Stdin)?;
    let mut value = String::new();
    if input.read_line(&mut value).map_err(AppError::Stdin)? == 0 {
        return Err(AppError::Stdin(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "interactive setup input ended",
        )));
    }
    if value.ends_with("\r\n") {
        value.truncate(value.len() - 2);
    } else if value.ends_with('\n') {
        value.pop();
    }
    Ok(value)
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

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::io::Cursor;

    use super::read_secret;

    #[test]
    fn read_secret_uses_hidden_reader_for_a_terminal() {
        let hidden_reader_called = Cell::new(false);
        let mut piped = Cursor::new("piped-secret\n");

        let value = read_secret(&mut piped, "secret: ", true, || {
            hidden_reader_called.set(true);
            Ok("terminal-secret".to_owned())
        })
        .expect("read terminal secret");

        assert_eq!(
            (value, hidden_reader_called.get(), piped.position()),
            ("terminal-secret".to_owned(), true, 0)
        );
    }

    #[test]
    fn read_secret_uses_bufread_for_a_pipe() {
        let hidden_reader_called = Cell::new(false);
        let mut piped = Cursor::new("piped-secret\n");

        let value = read_secret(&mut piped, "secret: ", false, || {
            hidden_reader_called.set(true);
            Ok("terminal-secret".to_owned())
        })
        .expect("read piped secret");

        assert_eq!(
            (value, hidden_reader_called.get()),
            ("piped-secret".to_owned(), false)
        );
    }
}
