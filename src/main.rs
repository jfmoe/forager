#![forbid(unsafe_code)]

use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use clap::Parser;
use forager::app::{
    self, AppError, Cli, CommandOutput, ContentOutputFormat, ExaOutcome, OutputFormat,
    OutputTarget, ProviderError, SearchFailure, bounded_attempt_summary,
};
use forager::types::{
    AttemptErrorKind, ErrorFamily, ErrorKind, FetchOutcome, JournalOutcome, MapOutcome,
    SearchCandidate, SearchOutcome,
};
use serde_json::{Value, json};

#[path = "cli/platform_render.rs"]
mod platform_render;

const FAILURE_PAYLOAD_TARGET_BYTES: usize = 4096;

// One arm per typed command output; each arm only hands its fields to that output's renderer.
#[expect(clippy::too_many_lines)]
fn main() -> ExitCode {
    let cli = Cli::parse();
    let json_preflight_errors = cli.uses_json_preflight_errors();
    let json_preflight_output = cli.json_preflight_output();
    match app::run(cli) {
        Ok(CommandOutput::Text {
            stdout,
            stderr,
            exit_code,
        }) => emit(stdout, stderr, exit_code),
        Ok(CommandOutput::Failure { kind, message }) => emit(
            json!({
                "error_kind": kind.as_str(),
                "message": message.chars().take(500).collect::<String>(),
            })
            .to_string(),
            None,
            postflight_exit_code(kind),
        ),
        Ok(CommandOutput::SearchPreflight {
            error,
            journal,
            format,
            output,
        }) => emit_search_preflight(&error, &journal, format, output),
        Ok(CommandOutput::Search {
            result,
            journal,
            format,
            output,
            verbose,
            attempt_log,
        }) => emit_logged(
            render_search(result, &journal, format, output, verbose),
            attempt_log,
        ),
        Ok(CommandOutput::Exa {
            result,
            format,
            output,
            attempt_log,
        }) => emit_logged(render_exa(result, format, output), attempt_log),
        Ok(CommandOutput::Fetch {
            result,
            format,
            output,
            attempt_log,
        }) => emit_logged(render_fetch(result, format, output), attempt_log),
        Ok(CommandOutput::Map {
            result,
            format,
            output,
            attempt_log,
        }) => emit_logged(render_map(result, format, output), attempt_log),
        Ok(CommandOutput::PlatformSearch {
            result,
            format,
            output,
            attempt_log,
        }) => emit_logged(
            platform_render::render_search(result, format, output),
            attempt_log,
        ),
        Ok(CommandOutput::PlatformFetch {
            result,
            format,
            output,
            attempt_log,
        }) => emit_logged(
            platform_render::render_fetch(*result, format, output),
            attempt_log,
        ),
        Ok(CommandOutput::PlatformComments {
            result,
            format,
            output,
            attempt_log,
        }) => emit_logged(
            platform_render::render_comments(result, format, output),
            attempt_log,
        ),
        Ok(CommandOutput::PlatformAsk {
            platform,
            result,
            format,
            output,
            attempt_log,
        }) => emit_logged(
            platform_render::render_ask(platform, result, format, output),
            attempt_log,
        ),
        Ok(CommandOutput::PlatformTranscribe {
            result,
            format,
            output,
            attempt_log,
        }) => emit_logged(
            platform_render::render_transcribe(*result, format, output),
            attempt_log,
        ),
        Ok(CommandOutput::HnStories {
            result,
            format,
            output,
            attempt_log,
        }) => emit_logged(
            platform_render::render_stories(result, format, output),
            attempt_log,
        ),
        Ok(CommandOutput::PolymarketHistory {
            result,
            format,
            output,
            attempt_log,
        }) => emit_logged(
            platform_render::render_history(result, format, output),
            attempt_log,
        ),
        Err(error) => emit_error(&error, json_preflight_errors, json_preflight_output),
    }
}

/// Emits a failure that ended the command before a rendered result: as the JSON preflight
/// payload when JSON was selected, else as one stderr line.
fn emit_error(error: &AppError, json: bool, output: Option<OutputTarget>) -> ExitCode {
    if !json {
        eprintln!("{}: {error}", error.category());
        return ExitCode::from(error.exit_code());
    }
    emit_rendered(apply_tee(
        error.json_preflight_payload().to_string(),
        error.exit_code(),
        true,
        output,
        None,
    ))
}

fn render_search(
    result: Result<SearchOutcome, SearchFailure>,
    journal: &JournalOutcome,
    format: ContentOutputFormat,
    output: Option<OutputTarget>,
    verbose: bool,
) -> Result<RenderedOutput, String> {
    let (stdout, exit_code, provider_diagnostic) = match result {
        Ok(outcome) => {
            let stdout = match format {
                ContentOutputFormat::Json => {
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
                ContentOutputFormat::Markdown => {
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
                    append_extra_sources_markdown(&mut markdown, &outcome.extra_sources);
                    markdown
                }
                ContentOutputFormat::Content => outcome.answer.clone(),
            };
            (stdout, 0, outcome.diagnostic)
        }
        Err(failure) => {
            let error = &failure.error;
            let stdout = match format {
                ContentOutputFormat::Json => format_search_failure_json(&failure, journal)?,
                ContentOutputFormat::Markdown | ContentOutputFormat::Content => {
                    let mut rendered = format!(
                        "# Search failed\n\n**{}**: {}",
                        error.kind.as_str(),
                        error.message
                    );
                    if format == ContentOutputFormat::Markdown {
                        append_extra_sources_markdown(&mut rendered, &failure.extra_sources);
                    }
                    rendered
                }
            };
            (
                stdout,
                postflight_exit_code(error.kind),
                failure.error.diagnostic,
            )
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
        format == ContentOutputFormat::Json,
        output,
        diagnostic,
    )
}

fn render_search_preflight(
    error: &app::AppError,
    journal: &JournalOutcome,
    format: ContentOutputFormat,
    output: Option<OutputTarget>,
) -> Result<RenderedOutput, String> {
    let journal_warning = journal
        .warning
        .as_ref()
        .map(|warning| format!("Search Result Journal warning: {warning}"));
    let (stdout, stderr) = match format {
        ContentOutputFormat::Json => {
            let mut payload = error.json_preflight_payload();
            add_journal_status(&mut payload, journal)?;
            (payload.to_string(), journal_warning)
        }
        ContentOutputFormat::Markdown | ContentOutputFormat::Content => (
            String::new(),
            app::combine_diagnostics(
                [
                    Some(format!("{}: {error}", error.category())),
                    journal_warning,
                ]
                .into_iter()
                .flatten(),
            ),
        ),
    };
    apply_tee(
        stdout,
        error.exit_code(),
        format == ContentOutputFormat::Json,
        output,
        stderr,
    )
}

fn emit_search_preflight(
    error: &app::AppError,
    journal: &JournalOutcome,
    format: ContentOutputFormat,
    output: Option<OutputTarget>,
) -> ExitCode {
    emit_rendered(render_search_preflight(error, journal, format, output))
}

fn emit_rendered(rendered: Result<RenderedOutput, String>) -> ExitCode {
    match rendered {
        Ok(rendered) => emit(rendered.stdout, rendered.stderr, rendered.exit_code),
        Err(error) => {
            eprintln!("runtime_error: {error}");
            ExitCode::from(4)
        }
    }
}

fn emit_logged(rendered: Result<RenderedOutput, String>, attempt_log: Option<String>) -> ExitCode {
    emit_rendered(rendered.map(|mut rendered| {
        rendered.stderr =
            app::combine_diagnostics([rendered.stderr.take(), attempt_log].into_iter().flatten());
        rendered
    }))
}

fn append_extra_sources_markdown(markdown: &mut String, extra_sources: &[SearchCandidate]) {
    if extra_sources.is_empty() {
        return;
    }
    markdown.push_str("\n\n## Extra Sources\n");
    for source in extra_sources {
        let url = source.url();
        let title = source.title().unwrap_or(url);
        let _ = write!(markdown, "\n- [{title}]({url}) — {}", source.provider());
        if let Some(summary) = source.summary() {
            let _ = write!(markdown, "\n\n  {summary}");
        }
    }
}

fn format_search_failure_json(
    failure: &SearchFailure,
    journal: &JournalOutcome,
) -> Result<String, String> {
    let mut payload: Value = serde_json::from_str(&format_failure_json(&failure.error)?)
        .map_err(|error| error.to_string())?;
    add_journal_status(&mut payload, journal)?;
    let object = payload
        .as_object_mut()
        .ok_or_else(|| "search failure output is not a JSON object".to_owned())?;
    if !failure.capability_gaps.is_empty() {
        object.insert(
            "capability_gaps".into(),
            serde_json::to_value(&failure.capability_gaps).map_err(|error| error.to_string())?,
        );
    }
    if !failure.extra_sources.is_empty() {
        let candidates =
            serde_json::to_value(&failure.extra_sources).map_err(|error| error.to_string())?;
        let Value::Array(mut candidates) = candidates else {
            return Err("extra_sources did not serialize as an array".into());
        };
        if failure.error.verbose {
            object.insert("extra_sources".into(), Value::Array(candidates));
        } else {
            for candidate in &mut candidates {
                if let Some(fields) = candidate.as_object_mut() {
                    fields.remove("summary");
                }
            }
            object.insert("extra_sources".into(), Value::Array(Vec::new()));
            object.insert("extra_sources_truncated".into(), Value::Bool(false));
            let base_len = serde_json::to_string(&*object)
                .map_err(|error| error.to_string())?
                .len();
            let (kept, truncated) = fit_failure_candidates(base_len, candidates);
            object.insert("extra_sources".into(), Value::Array(kept));
            object.insert("extra_sources_truncated".into(), Value::Bool(truncated));
        }
    }
    serde_json::to_string(&payload).map_err(|error| error.to_string())
}

// Callers read the default failure payload in full, so its 4 KiB target outranks candidate
// completeness; the journal keeps every candidate.
fn fit_failure_candidates(base_len: usize, candidates: Vec<Value>) -> (Vec<Value>, bool) {
    let total = candidates.len();
    let mut encoded_len = base_len;
    let mut kept = Vec::new();
    for candidate in candidates {
        let separator_len = usize::from(!kept.is_empty());
        let candidate_len = candidate.to_string().len();
        if encoded_len + separator_len + candidate_len > FAILURE_PAYLOAD_TARGET_BYTES {
            break;
        }
        encoded_len += separator_len + candidate_len;
        kept.push(candidate);
    }
    let truncated = kept.len() < total;
    (kept, truncated)
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
    output: Option<OutputTarget>,
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
    format: ContentOutputFormat,
    output: Option<OutputTarget>,
) -> Result<RenderedOutput, String> {
    let (stdout, exit_code, diagnostic) = match result {
        Ok(outcome) => {
            let stdout = match format {
                ContentOutputFormat::Json => {
                    serde_json::to_string(&outcome).map_err(|error| error.to_string())?
                }
                ContentOutputFormat::Markdown => {
                    format!("# Fetched with {}\n\n{}", outcome.provider, outcome.content)
                }
                ContentOutputFormat::Content => outcome.content.clone(),
            };
            (stdout, 0, outcome.diagnostic)
        }
        Err(error) => {
            let stdout = match format {
                ContentOutputFormat::Json => format_failure_json(&error)?,
                ContentOutputFormat::Markdown | ContentOutputFormat::Content => format!(
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
        format == ContentOutputFormat::Json,
        output,
        diagnostic,
    )
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
    output: Option<OutputTarget>,
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
    encode_failure_payload(error, &failure_payload(error)?)
}

/// The fields of the stable failure payload, for commands that add their own fields.
fn failure_payload(error: &ProviderError) -> Result<serde_json::Map<String, Value>, String> {
    let mut payload = serde_json::Map::new();
    payload.insert("error_kind".into(), error.kind.as_str().into());
    payload.insert(
        "message".into(),
        error.message.chars().take(500).collect::<String>().into(),
    );
    payload.insert("attempts".into(), bounded_attempt_summary(&error.attempts));
    payload.insert("journal_ref".into(), Value::Null);
    payload.insert("journal_status".into(), "not_applicable".into());
    if error.verbose {
        payload.insert(
            "provider_attempts".into(),
            serde_json::to_value(&error.attempts)
                .map_err(|serialize_error| serialize_error.to_string())?,
        );
    }
    Ok(payload)
}

/// Encodes a failure payload; without `--verbose` it must stay within 4 KiB.
fn encode_failure_payload(
    error: &ProviderError,
    payload: &serde_json::Map<String, Value>,
) -> Result<String, String> {
    let encoded = serde_json::to_string(payload).map_err(|error| error.to_string())?;
    if !error.verbose && encoded.len() > FAILURE_PAYLOAD_TARGET_BYTES {
        return Err("default failure payload exceeded 4 KiB".into());
    }
    Ok(encoded)
}

fn apply_tee(
    stdout: String,
    exit_code: u8,
    is_json: bool,
    output: Option<OutputTarget>,
    diagnostic: Option<String>,
) -> Result<RenderedOutput, String> {
    let Some(target) = output else {
        return Ok(RenderedOutput {
            stdout,
            stderr: diagnostic,
            exit_code,
        });
    };
    let tee_bytes = format!("{stdout}\n");
    match fs::write(target.path(), tee_bytes.as_bytes()) {
        Ok(()) => Ok(RenderedOutput {
            stdout: match target {
                OutputTarget::Receipt(path) if exit_code == 0 => output_receipt(&path, &tee_bytes),
                OutputTarget::Receipt(_) | OutputTarget::Tee(_) => stdout,
            },
            stderr: diagnostic,
            exit_code,
        }),
        Err(error) => {
            let output_diagnostic = format!(
                "cannot write output to {}: {error}",
                target.path().display()
            );
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

fn output_receipt(path: &Path, written: &str) -> String {
    json!({
        "output_path": path.to_string_lossy(),
        "bytes": written.len(),
        "lines": written.lines().count(),
    })
    .to_string()
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
    match ErrorKind::from(kind).family() {
        ErrorFamily::Transport => 4,
        ErrorFamily::Content => 5,
    }
}

#[cfg(test)]
mod tests {
    use forager::app::ProviderError;
    use forager::types::{AttemptDisposition, AttemptErrorKind, AttemptTarget, ProviderAttempt};
    use serde_json::{Value, json};

    use super::{
        FAILURE_PAYLOAD_TARGET_BYTES, OutputTarget, apply_tee, fit_failure_candidates,
        format_failure_json,
    };

    #[test]
    fn failure_candidates_stop_before_the_payload_target_is_exceeded() {
        let candidate = json!("abcd");
        let candidate_len = candidate.to_string().len();
        let base_len = FAILURE_PAYLOAD_TARGET_BYTES - 2 * candidate_len - 1;

        let (kept, truncated) = fit_failure_candidates(
            base_len,
            vec![candidate.clone(), candidate.clone(), candidate],
        );

        assert_eq!((kept.len(), truncated), (2, true));
    }

    #[test]
    fn failure_candidates_that_fit_are_all_kept() {
        let (kept, truncated) = fit_failure_candidates(100, vec![json!("a"), json!("b")]);

        assert_eq!((kept, truncated), (vec![json!("a"), json!("b")], false));
    }

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
            attempts,
            verbose: false,
            diagnostic: None,
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
                payload["attempts"]["by_kind"]
                    .as_object()
                    .map(serde_json::Map::len),
                payload["attempts"]["by_kind_truncated"].as_bool(),
                payload["attempts"]["providers"].as_array().map(Vec::len),
                payload["attempts"]["providers_truncated"].as_bool(),
            ),
            (Some(500), Some(8), Some(true), Some(8), Some(true))
        );
    }

    #[test]
    fn a_failure_lists_its_provider_attempts_and_may_exceed_four_kib_only_when_verbose() {
        let attempts = (0..10)
            .map(|_| ProviderAttempt {
                message: "rate limit detail ".repeat(100),
                ..attempt("exa", AttemptErrorKind::RateLimited)
            })
            .collect::<Vec<_>>();
        let payloads = [false, true].map(|verbose| {
            let encoded = format_failure_json(&ProviderError {
                kind: AttemptErrorKind::RateLimited,
                message: "rate limit detail".into(),
                attempts: attempts.clone(),
                verbose,
                diagnostic: None,
            })
            .expect("provider failure payload");
            let payload: Value = serde_json::from_str(&encoded).expect("provider failure JSON");
            (
                payload
                    .get("provider_attempts")
                    .and_then(Value::as_array)
                    .map(Vec::len),
                encoded.len() > FAILURE_PAYLOAD_TARGET_BYTES,
            )
        });

        assert_eq!(payloads, [(None, false), (Some(10), true)]);
    }

    #[test]
    fn an_output_file_that_cannot_be_written_marks_the_json_and_exits_three() {
        let directory = tempfile::tempdir().expect("create unwritable output target");

        let rendered = apply_tee(
            json!({"results": []}).to_string(),
            0,
            true,
            Some(OutputTarget::Tee(directory.path().to_path_buf())),
            None,
        )
        .expect("rendered output");
        let payload: Value = serde_json::from_str(&rendered.stdout).expect("annotated JSON");

        assert_eq!(
            (
                rendered.exit_code,
                &payload["output_status"],
                &payload["results"],
                rendered
                    .stderr
                    .is_some_and(|stderr| stderr.contains("cannot write output")),
            ),
            (3, &json!("failed"), &json!([]), true)
        );
    }

    const PROVIDERS: [&str; 9] = [
        "arxiv_api",
        "exa",
        "firecrawl",
        "hn_api",
        "jina",
        "openai",
        "serpapi",
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
            target: AttemptTarget::operation("acceptance"),
            disposition: AttemptDisposition::Failed,
            error_kind: Some(kind),
            http_status: None,
            duration_ms: 0,
            credential_index: 0,
            retry_count: 0,
            rotation_count: 0,
            anonymous: false,
            message: String::new(),
            model: None,
            transport: None,
            endpoint_host: None,
        }
    }
}
