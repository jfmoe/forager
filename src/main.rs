#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use forager::app::{self, Cli, CommandOutput, ExaOutcome, OutputFormat, ProviderError};
use forager::types::{AttemptErrorKind, ErrorFamily, ErrorKind};
use serde_json::{Value, json};

fn main() -> ExitCode {
    match app::run(Cli::parse()) {
        Ok(CommandOutput::Text {
            stdout,
            stderr,
            exit_code,
        }) => emit(stdout, stderr, exit_code),
        Ok(CommandOutput::Exa {
            result,
            format,
            output,
        }) => match render_exa(result, format, output) {
            Ok(rendered) => emit(rendered.stdout, rendered.stderr, rendered.exit_code),
            Err(error) => {
                eprintln!("runtime_error: {error}");
                ExitCode::from(4)
            }
        },
        Err(error) => {
            eprintln!("{}: {error}", error.category());
            ExitCode::from(error.exit_code())
        }
    }
}

fn emit(stdout: String, stderr: Option<String>, exit_code: u8) -> ExitCode {
    if !stdout.is_empty() {
        println!("{stdout}");
    }
    if let Some(diagnostic) = stderr {
        eprintln!("{diagnostic}");
    }
    ExitCode::from(exit_code)
}

struct RenderedOutput {
    stdout: String,
    stderr: Option<String>,
    exit_code: u8,
}

fn render_exa(
    result: Result<ExaOutcome, ProviderError>,
    format: OutputFormat,
    output: Option<PathBuf>,
) -> Result<RenderedOutput, String> {
    let (stdout, exit_code, diagnostic) = match result {
        Ok(outcome) => (format_success(&outcome, format)?, 0, outcome.diagnostic),
        Err(error) => (
            format_failure(&error, format)?,
            postflight_exit_code(error.kind),
            error.diagnostic,
        ),
    };
    apply_tee(stdout, exit_code, format, output, diagnostic)
}

fn format_success(outcome: &ExaOutcome, format: OutputFormat) -> Result<String, String> {
    match format {
        OutputFormat::Json => serde_json::to_string(outcome).map_err(|error| error.to_string()),
        OutputFormat::Markdown => {
            let mut markdown = format!(
                "# Exa {}: {}\n",
                outcome.input.operation(),
                outcome.input.value()
            );
            for source in &outcome.results {
                markdown.push_str(&format!("\n- [{}]({})", source.title, source.url));
                if let Some(published_date) = &source.published_date {
                    markdown.push_str(&format!(" — {published_date}"));
                }
                if let Some(text) = &source.text {
                    markdown.push_str(&format!("\n\n  {text}"));
                }
            }
            if outcome.results.is_empty() {
                markdown.push_str("\n\nNo results.");
            }
            Ok(markdown)
        }
    }
}

fn format_failure(error: &ProviderError, format: OutputFormat) -> Result<String, String> {
    match format {
        OutputFormat::Json => {
            let mut by_kind = BTreeMap::new();
            for attempt in &error.attempts {
                if let Some(kind) = attempt.error_kind {
                    *by_kind.entry(kind.as_str()).or_insert(0) += 1;
                }
            }
            let mut payload = json!({
                "error_kind": error.kind.as_str(),
                "message": error.message.chars().take(500).collect::<String>(),
                "attempts": {
                    "total": error.attempts.len(),
                    "by_kind": by_kind,
                    "providers": if error.attempts.is_empty() { Vec::<&str>::new() } else { vec!["exa"] },
                    "truncated": false
                },
                "journal_ref": Value::Null,
                "journal_status": "not_applicable"
            });
            if error.verbose {
                payload
                    .as_object_mut()
                    .expect("failure payload is an object")
                    .insert(
                        "provider_attempts".into(),
                        serde_json::to_value(&error.attempts)
                            .map_err(|serialize_error| serialize_error.to_string())?,
                    );
            }
            let encoded = serde_json::to_string(&payload).map_err(|error| error.to_string())?;
            if !error.verbose && encoded.len() > 4096 {
                return Err("default failure payload exceeded 4 KiB".into());
            }
            Ok(encoded)
        }
        OutputFormat::Markdown => Ok(format!(
            "# Exa search failed\n\n**{}**: {}",
            error.kind.as_str(),
            error.message
        )),
    }
}

fn apply_tee(
    stdout: String,
    exit_code: u8,
    format: OutputFormat,
    output: Option<PathBuf>,
    diagnostic: Option<String>,
) -> Result<RenderedOutput, String> {
    let Some(path) = output else {
        return Ok(RenderedOutput {
            stdout,
            stderr: diagnostic,
            exit_code,
        });
    };
    let tee_bytes = format!("{stdout}\n");
    match fs::write(&path, tee_bytes.as_bytes()) {
        Ok(()) => Ok(RenderedOutput {
            stdout,
            stderr: diagnostic,
            exit_code,
        }),
        Err(error) => {
            let output_diagnostic = format!("cannot write output to {}: {error}", path.display());
            let stdout = if format == OutputFormat::Json {
                annotate_output_failure(&stdout, &output_diagnostic)?
            } else {
                stdout
            };
            Ok(RenderedOutput {
                stdout,
                stderr: Some(match diagnostic {
                    Some(diagnostic) => format!("{diagnostic}\n{output_diagnostic}"),
                    None => output_diagnostic,
                }),
                exit_code: 3,
            })
        }
    }
}

fn annotate_output_failure(stdout: &str, diagnostic: &str) -> Result<String, String> {
    let mut payload: Value = serde_json::from_str(stdout).map_err(|error| error.to_string())?;
    let object = payload
        .as_object_mut()
        .ok_or_else(|| "cannot annotate non-object JSON output".to_owned())?;
    object.insert("output_status".into(), Value::String("failed".into()));
    object.insert(
        "output_error".into(),
        Value::String(diagnostic.chars().take(500).collect()),
    );
    serde_json::to_string(&payload).map_err(|error| error.to_string())
}

fn postflight_exit_code(kind: AttemptErrorKind) -> u8 {
    match ErrorKind::from(kind)
        .family()
        .expect("postflight errors always have a family")
    {
        ErrorFamily::Transport => 4,
        ErrorFamily::Content => 5,
    }
}
