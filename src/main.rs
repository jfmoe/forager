#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use forager::app::{
    self, Cli, CommandOutput, DocsOutputFormat, ExaOutcome, OutputFormat, ProviderError,
};
use forager::types::{
    AnysearchOutcome, AttemptErrorKind, Context7Outcome, ErrorFamily, ErrorKind, FetchOutcome,
    JournalOutcome, MapOutcome, ResearchError, ResearchOutcome, SearchOutcome,
};
use serde_json::{Value, json};

fn main() -> ExitCode {
    match app::run(Cli::parse()) {
        Ok(CommandOutput::Text {
            stdout,
            stderr,
            exit_code,
        }) => emit(stdout, stderr, exit_code),
        Ok(CommandOutput::Search {
            result,
            journal,
            format,
            output,
            verbose,
        }) => match render_search(result, &journal, format, output, verbose) {
            Ok(rendered) => emit(rendered.stdout, rendered.stderr, rendered.exit_code),
            Err(error) => {
                eprintln!("runtime_error: {error}");
                ExitCode::from(4)
            }
        },
        Ok(CommandOutput::Research {
            result,
            journal,
            format,
            output,
            verbose,
        }) => match render_research(result, &journal, format, output, verbose) {
            Ok(rendered) => emit(rendered.stdout, rendered.stderr, rendered.exit_code),
            Err(error) => {
                eprintln!("runtime_error: {error}");
                ExitCode::from(4)
            }
        },
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
        Ok(CommandOutput::Context7 {
            result,
            format,
            output,
        }) => match render_context7(result, format, output) {
            Ok(rendered) => emit(rendered.stdout, rendered.stderr, rendered.exit_code),
            Err(error) => {
                eprintln!("runtime_error: {error}");
                ExitCode::from(4)
            }
        },
        Ok(CommandOutput::Anysearch {
            result,
            format,
            output,
        }) => match render_anysearch(result, format, output) {
            Ok(rendered) => emit(rendered.stdout, rendered.stderr, rendered.exit_code),
            Err(error) => {
                eprintln!("runtime_error: {error}");
                ExitCode::from(4)
            }
        },
        Ok(CommandOutput::Fetch {
            result,
            format,
            output,
        }) => match render_fetch(result, format, output) {
            Ok(rendered) => emit(rendered.stdout, rendered.stderr, rendered.exit_code),
            Err(error) => {
                eprintln!("runtime_error: {error}");
                ExitCode::from(4)
            }
        },
        Ok(CommandOutput::Map {
            result,
            format,
            output,
        }) => match render_map(result, format, output) {
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

fn render_research(
    result: Result<ResearchOutcome, ResearchError>,
    journal: &JournalOutcome,
    format: DocsOutputFormat,
    output: Option<PathBuf>,
    verbose: bool,
) -> Result<RenderedOutput, String> {
    let (stdout, exit_code, diagnostic) = match result {
        Ok(outcome) => {
            let stdout = match format {
                DocsOutputFormat::Json => {
                    let mut payload =
                        serde_json::to_value(&outcome).map_err(|error| error.to_string())?;
                    add_journal_status(&mut payload, journal)?;
                    if verbose {
                        payload
                            .as_object_mut()
                            .expect("research outcome is an object")
                            .insert(
                                "provider_attempts".into(),
                                serde_json::to_value(&outcome.attempts)
                                    .map_err(|error| error.to_string())?,
                            );
                    }
                    serde_json::to_string(&payload).map_err(|error| error.to_string())?
                }
                DocsOutputFormat::Markdown => {
                    let mut markdown = format!("# Research Report\n\n{}", outcome.final_answer);
                    if !outcome.citations.is_empty() {
                        markdown.push_str("\n\n## Citations\n");
                        for citation in &outcome.citations {
                            let _ = write!(
                                markdown,
                                "\n- [{}]({}) — {}",
                                citation.title, citation.url, citation.provider
                            );
                        }
                    }
                    markdown
                }
                DocsOutputFormat::Content => outcome.content.clone(),
            };
            (stdout, 0, outcome.diagnostic)
        }
        Err(error) => {
            let stdout = match format {
                DocsOutputFormat::Json => format_research_failure_json(&error, journal, verbose)?,
                DocsOutputFormat::Markdown | DocsOutputFormat::Content => format!(
                    "# Research failed\n\n**{}**: {}",
                    error.kind.as_str(),
                    error.message
                ),
            };
            (stdout, postflight_exit_code(error.kind), error.diagnostic)
        }
    };
    let diagnostic = app::combine_diagnostics(
        [
            diagnostic,
            journal
                .warning
                .as_ref()
                .map(|warning| format!("Search Result Journal warning: {warning}")),
        ]
        .into_iter()
        .flatten(),
    );
    apply_tee(
        stdout,
        exit_code,
        format == DocsOutputFormat::Json,
        output,
        diagnostic,
    )
}

fn format_research_failure_json(
    error: &ResearchError,
    journal: &JournalOutcome,
    verbose: bool,
) -> Result<String, String> {
    let mut payload = json!({
        "error_kind": error.kind.as_str(),
        "message": error.message.chars().take(500).collect::<String>(),
        "attempts": bounded_attempt_summary(&error.attempts),
        "capability_gaps": bounded_capability_gaps(&error.capability_gaps),
    });
    add_journal_status(&mut payload, journal)?;
    if verbose {
        payload
            .as_object_mut()
            .expect("research failure is an object")
            .insert(
                "provider_attempts".into(),
                serde_json::to_value(&error.attempts).map_err(|error| error.to_string())?,
            );
    }
    let encoded = serde_json::to_string(&payload).map_err(|error| error.to_string())?;
    if !verbose && encoded.len() > 4096 {
        return Err("default failure payload exceeded 4 KiB".into());
    }
    Ok(encoded)
}

fn render_search(
    result: Result<SearchOutcome, ProviderError>,
    journal: &JournalOutcome,
    format: DocsOutputFormat,
    output: Option<PathBuf>,
    verbose: bool,
) -> Result<RenderedOutput, String> {
    let (stdout, exit_code, provider_diagnostic) = match result {
        Ok(outcome) => {
            let stdout = match format {
                DocsOutputFormat::Json => {
                    let mut payload =
                        serde_json::to_value(&outcome).map_err(|error| error.to_string())?;
                    add_journal_status(&mut payload, journal)?;
                    if verbose {
                        payload
                            .as_object_mut()
                            .expect("search outcome is an object")
                            .insert(
                                "provider_attempts".into(),
                                serde_json::to_value(&outcome.attempts)
                                    .map_err(|error| error.to_string())?,
                            );
                    }
                    serde_json::to_string(&payload).map_err(|error| error.to_string())?
                }
                DocsOutputFormat::Markdown => {
                    let mut markdown = format!("# Search result\n\n{}", outcome.answer);
                    if !outcome.sources.is_empty() {
                        markdown.push_str("\n\n## Primary Sources\n");
                        for source in &outcome.sources {
                            let title = if source.title.is_empty() {
                                &source.url
                            } else {
                                &source.title
                            };
                            let _ = write!(markdown, "\n- [{title}]({})", source.url);
                        }
                    }
                    if !outcome.extra_sources.is_empty() {
                        markdown.push_str("\n\n## Extra Sources\n");
                        for source in &outcome.extra_sources {
                            let title = source.title.as_deref().unwrap_or(&source.url);
                            let _ = write!(
                                markdown,
                                "\n- [{title}]({}) — {}",
                                source.url, source.provider
                            );
                            if let Some(summary) = &source.summary {
                                let _ = write!(markdown, "\n\n  {summary}");
                            }
                        }
                    }
                    if !outcome.vertical_results.is_empty() {
                        markdown.push_str("\n\n## Vertical Results\n");
                        for result in &outcome.vertical_results {
                            if result.url.is_empty() {
                                let _ = write!(
                                    markdown,
                                    "\n- **{}** — {}",
                                    result.title, result.description
                                );
                            } else {
                                let _ = write!(
                                    markdown,
                                    "\n- [{}]({}) — {}",
                                    result.title, result.url, result.description
                                );
                            }
                        }
                    }
                    markdown
                }
                DocsOutputFormat::Content => outcome.answer.clone(),
            };
            (stdout, 0, outcome.diagnostic)
        }
        Err(error) => {
            let stdout = match format {
                DocsOutputFormat::Json => format_search_failure_json(&error, journal)?,
                DocsOutputFormat::Markdown | DocsOutputFormat::Content => format!(
                    "# Search failed\n\n**{}**: {}",
                    error.kind.as_str(),
                    error.message
                ),
            };
            (stdout, postflight_exit_code(error.kind), error.diagnostic)
        }
    };
    let diagnostic = app::combine_diagnostics(
        [
            provider_diagnostic,
            journal
                .warning
                .as_ref()
                .map(|warning| format!("Search Result Journal warning: {warning}")),
        ]
        .into_iter()
        .flatten(),
    );
    apply_tee(
        stdout,
        exit_code,
        format == DocsOutputFormat::Json,
        output,
        diagnostic,
    )
}

fn format_search_failure_json(
    error: &ProviderError,
    journal: &JournalOutcome,
) -> Result<String, String> {
    let mut payload: Value =
        serde_json::from_str(&format_failure_json(error)?).map_err(|error| error.to_string())?;
    add_journal_status(&mut payload, journal)?;
    serde_json::to_string(&payload).map_err(|error| error.to_string())
}

fn add_journal_status(payload: &mut Value, journal: &JournalOutcome) -> Result<(), String> {
    let object = payload
        .as_object_mut()
        .ok_or_else(|| "search output is not a JSON object".to_owned())?;
    object.insert(
        "journal_ref".into(),
        journal
            .reference
            .as_ref()
            .map_or(Value::Null, |reference| Value::String(reference.clone())),
    );
    object.insert(
        "journal_status".into(),
        Value::String(journal.status.into()),
    );
    Ok(())
}

fn render_map(
    result: Result<MapOutcome, ProviderError>,
    format: OutputFormat,
    output: Option<PathBuf>,
) -> Result<RenderedOutput, String> {
    let (stdout, exit_code, diagnostic) = match result {
        Ok(outcome) => {
            let stdout = match format {
                OutputFormat::Json => {
                    serde_json::to_string(&outcome).map_err(|error| error.to_string())?
                }
                OutputFormat::Markdown => {
                    let mut markdown = format!("# Site map: {}\n", outcome.base_url);
                    for result in &outcome.results {
                        let _ = write!(markdown, "\n- <{result}>");
                    }
                    if outcome.results.is_empty() {
                        markdown.push_str("\n\nNo results.");
                    }
                    markdown
                }
            };
            (stdout, 0, outcome.diagnostic)
        }
        Err(error) => {
            let stdout = match format {
                OutputFormat::Json => format_failure_json(&error)?,
                OutputFormat::Markdown => format!(
                    "# Site map failed\n\n**{}**: {}",
                    error.kind.as_str(),
                    error.message
                ),
            };
            (stdout, postflight_exit_code(error.kind), error.diagnostic)
        }
    };
    apply_tee(
        stdout,
        exit_code,
        format == OutputFormat::Json,
        output,
        diagnostic,
    )
}

fn render_fetch(
    result: Result<FetchOutcome, ProviderError>,
    format: DocsOutputFormat,
    output: Option<PathBuf>,
) -> Result<RenderedOutput, String> {
    let (stdout, exit_code, diagnostic) = match result {
        Ok(outcome) => {
            let stdout = match format {
                DocsOutputFormat::Json => {
                    serde_json::to_string(&outcome).map_err(|error| error.to_string())?
                }
                DocsOutputFormat::Markdown => {
                    format!("# Fetched with {}\n\n{}", outcome.provider, outcome.content)
                }
                DocsOutputFormat::Content => outcome.content.clone(),
            };
            (stdout, 0, outcome.diagnostic)
        }
        Err(error) => {
            let stdout = match format {
                DocsOutputFormat::Json => format_failure_json(&error)?,
                DocsOutputFormat::Markdown | DocsOutputFormat::Content => format!(
                    "# Fetch failed\n\n**{}**: {}",
                    error.kind.as_str(),
                    error.message
                ),
            };
            (stdout, postflight_exit_code(error.kind), error.diagnostic)
        }
    };
    apply_tee(
        stdout,
        exit_code,
        format == DocsOutputFormat::Json,
        output,
        diagnostic,
    )
}

fn render_anysearch(
    result: Result<AnysearchOutcome, ProviderError>,
    format: OutputFormat,
    output: Option<PathBuf>,
) -> Result<RenderedOutput, String> {
    let (stdout, exit_code, diagnostic) = match result {
        Ok(outcome) => {
            let diagnostic = match &outcome {
                AnysearchOutcome::Domains(outcome) => outcome.diagnostic.clone(),
                AnysearchOutcome::Search(outcome) => outcome.diagnostic.clone(),
            };
            (format_anysearch_success(&outcome, format)?, 0, diagnostic)
        }
        Err(error) => (
            format_anysearch_failure(&error, format)?,
            postflight_exit_code(error.kind),
            error.diagnostic,
        ),
    };
    apply_tee(
        stdout,
        exit_code,
        format == OutputFormat::Json,
        output,
        diagnostic,
    )
}

fn format_anysearch_failure(error: &ProviderError, format: OutputFormat) -> Result<String, String> {
    match format {
        OutputFormat::Json => format_failure_json(error),
        OutputFormat::Markdown => Ok(format!(
            "# AnySearch request failed\n\n**{}**: {}",
            error.kind.as_str(),
            error.message
        )),
    }
}

fn format_anysearch_success(
    outcome: &AnysearchOutcome,
    format: OutputFormat,
) -> Result<String, String> {
    match (outcome, format) {
        (AnysearchOutcome::Domains(outcome), OutputFormat::Json) => {
            serde_json::to_string(outcome).map_err(|error| error.to_string())
        }
        (AnysearchOutcome::Domains(outcome), OutputFormat::Markdown) => {
            let mut markdown = format!("# AnySearch domains: {}\n", outcome.domain);
            for domain in &outcome.results {
                let _ = write!(
                    markdown,
                    "\n- **{}.{}** — {}",
                    outcome.domain, domain.sub_domain, domain.description
                );
            }
            if outcome.results.is_empty() {
                markdown.push_str("\n\nNo results.");
            }
            Ok(markdown)
        }
        (AnysearchOutcome::Search(outcome), OutputFormat::Json) => {
            serde_json::to_string(outcome).map_err(|error| error.to_string())
        }
        (AnysearchOutcome::Search(outcome), OutputFormat::Markdown) => {
            let mut markdown = format!("# AnySearch {}: {}\n", outcome.operation, outcome.query);
            for result in &outcome.results {
                if result.url.is_empty() {
                    let _ = write!(
                        markdown,
                        "\n- **{}** — {}",
                        result.title, result.description
                    );
                } else {
                    let _ = write!(
                        markdown,
                        "\n- [{}]({}) — {}",
                        result.title, result.url, result.description
                    );
                }
            }
            if outcome.results.is_empty() {
                markdown.push_str("\n\nNo results.");
            }
            Ok(markdown)
        }
    }
}

// Rendering transfers the completed stdout buffer to the terminal as a final output value.
#[expect(clippy::needless_pass_by_value)]
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
    apply_tee(
        stdout,
        exit_code,
        format == OutputFormat::Json,
        output,
        diagnostic,
    )
}

fn render_context7(
    result: Result<Context7Outcome, ProviderError>,
    format: DocsOutputFormat,
    output: Option<PathBuf>,
) -> Result<RenderedOutput, String> {
    let (stdout, exit_code, diagnostic) = match result {
        Ok(outcome) => {
            let diagnostic = match &outcome {
                Context7Outcome::Library(outcome) => outcome.diagnostic.clone(),
                Context7Outcome::Docs(outcome) => outcome.diagnostic.clone(),
            };
            (format_context7_success(&outcome, format)?, 0, diagnostic)
        }
        Err(error) => (
            format_context7_failure(&error, format)?,
            postflight_exit_code(error.kind),
            error.diagnostic,
        ),
    };
    apply_tee(
        stdout,
        exit_code,
        format == DocsOutputFormat::Json,
        output,
        diagnostic,
    )
}

fn format_context7_success(
    outcome: &Context7Outcome,
    format: DocsOutputFormat,
) -> Result<String, String> {
    match (outcome, format) {
        (Context7Outcome::Library(outcome), DocsOutputFormat::Json) => {
            serde_json::to_string(outcome).map_err(|error| error.to_string())
        }
        (Context7Outcome::Library(outcome), DocsOutputFormat::Markdown) => {
            let mut markdown = format!("# Context7 libraries: {}\n", outcome.query);
            for library in &outcome.results {
                let _ = write!(
                    markdown,
                    "\n- **{}** (`{}`) — {}",
                    library.title, library.id, library.description
                );
            }
            if outcome.results.is_empty() {
                markdown.push_str("\n\nNo results.");
            }
            Ok(markdown)
        }
        (Context7Outcome::Library(_), DocsOutputFormat::Content) => {
            Err("content format is only available for context7 docs".into())
        }
        (Context7Outcome::Docs(outcome), DocsOutputFormat::Json) => {
            serde_json::to_string(outcome).map_err(|error| error.to_string())
        }
        (Context7Outcome::Docs(outcome), DocsOutputFormat::Markdown) => Ok(format!(
            "# Context7 docs: {}\n\n{}",
            outcome.library_id, outcome.content
        )),
        (Context7Outcome::Docs(outcome), DocsOutputFormat::Content) => Ok(outcome.content.clone()),
    }
}

fn format_context7_failure(
    error: &ProviderError,
    format: DocsOutputFormat,
) -> Result<String, String> {
    match format {
        DocsOutputFormat::Json => format_failure_json(error),
        DocsOutputFormat::Markdown | DocsOutputFormat::Content => Ok(format!(
            "# Context7 request failed\n\n**{}**: {}",
            error.kind.as_str(),
            error.message
        )),
    }
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
                let _ = write!(markdown, "\n- [{}]({})", source.title, source.url);
                if let Some(published_date) = &source.published_date {
                    let _ = write!(markdown, " — {published_date}");
                }
                if let Some(text) = &source.text {
                    let _ = write!(markdown, "\n\n  {text}");
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
        OutputFormat::Json => format_failure_json(error),
        OutputFormat::Markdown => Ok(format!(
            "# Exa search failed\n\n**{}**: {}",
            error.kind.as_str(),
            error.message
        )),
    }
}

fn format_failure_json(error: &ProviderError) -> Result<String, String> {
    let mut payload = json!({
        "error_kind": error.kind.as_str(),
        "message": error.message.chars().take(500).collect::<String>(),
        "attempts": bounded_attempt_summary(&error.attempts),
        "journal_ref": Value::Null,
        "journal_status": "not_applicable"
    });
    if let Some(target) = &error.redirected_library_id {
        payload
            .as_object_mut()
            .expect("failure payload is an object")
            .insert(
                "redirected_library_id".into(),
                Value::String(target.chars().take(500).collect()),
            );
    }
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

fn bounded_attempt_summary(attempts: &[forager::types::ProviderAttempt]) -> Value {
    let mut by_kind = BTreeMap::new();
    for attempt in attempts {
        if let Some(kind) = attempt.error_kind {
            *by_kind.entry(kind.as_str()).or_insert(0) += 1;
        }
    }
    let by_kind_count = by_kind.len();
    let by_kind = by_kind.into_iter().take(8).collect::<BTreeMap<_, _>>();
    let provider_set = attempts
        .iter()
        .map(|attempt| attempt.provider)
        .collect::<BTreeSet<_>>();
    let providers = provider_set.iter().take(8).copied().collect::<Vec<_>>();
    let by_kind_truncated = by_kind_count > by_kind.len();
    let providers_truncated = provider_set.len() > providers.len();

    json!({
        "total": attempts.len(),
        "by_kind": by_kind,
        "by_kind_truncated": by_kind_truncated,
        "providers": providers,
        "providers_truncated": providers_truncated,
        "truncated": by_kind_truncated || providers_truncated,
    })
}

fn bounded_capability_gaps(gaps: &[forager::types::CapabilityGap]) -> Value {
    Value::Array(
        gaps.iter()
            .map(|gap| {
                let providers = gap.providers_skipped.iter().take(8).collect::<Vec<_>>();
                json!({
                    "capability": gap.capability,
                    "reason": gap.reason,
                    "providers_skipped": providers,
                    "truncated": gap.providers_skipped.len() > providers.len(),
                })
            })
            .collect(),
    )
}

fn apply_tee(
    stdout: String,
    exit_code: u8,
    is_json: bool,
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
            let stdout = if is_json {
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

#[cfg(test)]
mod tests {
    use forager::app::ProviderError;
    use forager::types::{
        AttemptErrorKind, Capability, CapabilityGap, JournalOutcome, ProviderAttempt, ResearchError,
    };
    use serde_json::{Value, json};

    use super::{format_failure_json, format_research_failure_json};

    #[test]
    fn default_failure_payload_truncates_each_list_and_preserves_utf8_message_boundary() {
        let attempts = all_attempt_kinds()
            .into_iter()
            .enumerate()
            .map(|(index, kind)| attempt(PROVIDERS[index], kind))
            .collect::<Vec<_>>();
        let provider_error = ProviderError {
            kind: AttemptErrorKind::Evidence,
            message: "界".repeat(600),
            attempts: attempts.clone(),
            verbose: false,
            diagnostic: None,
            redirected_library_id: Some("库".repeat(2_000)),
        };
        let encoded =
            format_failure_json(&provider_error).expect("bounded provider failure payload");
        let payload: Value = serde_json::from_str(&encoded).expect("provider failure JSON");

        assert!(encoded.len() <= 4096);
        assert_eq!(
            (
                payload["message"]
                    .as_str()
                    .map(str::chars)
                    .map(Iterator::count),
                payload["redirected_library_id"]
                    .as_str()
                    .map(str::chars)
                    .map(Iterator::count),
                payload["attempts"]["by_kind"]
                    .as_object()
                    .map(serde_json::Map::len),
                payload["attempts"]["by_kind_truncated"].as_bool(),
                payload["attempts"]["providers"].as_array().map(Vec::len),
                payload["attempts"]["providers_truncated"].as_bool(),
            ),
            (
                Some(500),
                Some(500),
                Some(8),
                Some(true),
                Some(8),
                Some(true)
            )
        );

        let research_error = ResearchError {
            kind: AttemptErrorKind::Evidence,
            message: "界".repeat(600),
            attempts,
            evidence_items: Vec::new(),
            capability_gaps: vec![CapabilityGap {
                capability: Capability::WebSearch,
                reason: "all_attempts_failed",
                providers_skipped: PROVIDERS
                    .iter()
                    .map(|provider| (*provider).into())
                    .collect(),
            }],
            diagnostic: None,
        };
        let journal = JournalOutcome {
            status: "disabled",
            reference: None,
            warning: None,
        };
        let encoded = format_research_failure_json(&research_error, &journal, false)
            .expect("bounded research failure payload");
        let payload: Value = serde_json::from_str(&encoded).expect("research failure JSON");

        assert!(encoded.len() <= 4096);
        assert_eq!(
            (
                payload["attempts"]["by_kind"]
                    .as_object()
                    .map(serde_json::Map::len),
                payload["attempts"]["by_kind_truncated"].as_bool(),
                payload["capability_gaps"][0]["providers_skipped"]
                    .as_array()
                    .map(Vec::len),
                &payload["capability_gaps"][0]["truncated"],
            ),
            (Some(8), Some(true), Some(8), &json!(true))
        );
    }

    const PROVIDERS: [&str; 9] = [
        "anysearch",
        "classifier",
        "context7",
        "exa",
        "firecrawl",
        "jina",
        "openai_compatible",
        "tavily",
        "xai",
    ];

    fn all_attempt_kinds() -> [AttemptErrorKind; 9] {
        [
            AttemptErrorKind::Auth,
            AttemptErrorKind::RateLimited,
            AttemptErrorKind::QuotaExhausted,
            AttemptErrorKind::Parameter,
            AttemptErrorKind::Timeout,
            AttemptErrorKind::Network,
            AttemptErrorKind::Quality,
            AttemptErrorKind::Evidence,
            AttemptErrorKind::Runtime,
        ]
    }

    fn attempt(provider: &'static str, kind: AttemptErrorKind) -> ProviderAttempt {
        ProviderAttempt {
            provider,
            seam: "acceptance",
            error_kind: Some(kind),
            http_status: None,
            duration_ms: 0,
            credential_index: 0,
            retry_count: 0,
            rotation_count: 0,
            message: String::new(),
            model: None,
            transport: None,
            endpoint_host: None,
            breaker_event: None,
        }
    }
}
