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
    ProviderAttempt, SearchCandidate, SearchOutcome,
};
use serde::Serialize;
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
            verbose,
            attempt_log,
        }) => emit_logged(render_exa(result, format, output, verbose), attempt_log),
        Ok(CommandOutput::Fetch {
            result,
            format,
            output,
            verbose,
            attempt_log,
        }) => emit_logged(render_fetch(result, format, output, verbose), attempt_log),
        Ok(CommandOutput::Map {
            result,
            format,
            output,
            verbose,
            attempt_log,
        }) => emit_logged(render_map(result, format, output, verbose), attempt_log),
        Ok(CommandOutput::PlatformSearch {
            result,
            format,
            output,
            verbose,
            attempt_log,
        }) => emit_logged(
            platform_render::render_search(result, format, output, verbose),
            attempt_log,
        ),
        Ok(CommandOutput::PlatformFetch {
            result,
            format,
            output,
            verbose,
            attempt_log,
        }) => emit_logged(
            platform_render::render_fetch(*result, format, output, verbose),
            attempt_log,
        ),
        Ok(CommandOutput::PlatformComments {
            result,
            format,
            output,
            verbose,
            attempt_log,
        }) => emit_logged(
            platform_render::render_comments(result, format, output, verbose),
            attempt_log,
        ),
        Ok(CommandOutput::PlatformAsk {
            platform,
            result,
            format,
            output,
            verbose,
            attempt_log,
        }) => emit_logged(
            platform_render::render_ask(platform, result, format, output, verbose),
            attempt_log,
        ),
        Ok(CommandOutput::PlatformTranscribe {
            result,
            format,
            output,
            verbose,
            attempt_log,
        }) => emit_logged(
            platform_render::render_transcribe(*result, format, output, verbose),
            attempt_log,
        ),
        Ok(CommandOutput::HnStories {
            result,
            format,
            output,
            verbose,
            attempt_log,
        }) => emit_logged(
            platform_render::render_stories(result, format, output, verbose),
            attempt_log,
        ),
        Ok(CommandOutput::PolymarketHistory {
            result,
            format,
            output,
            verbose,
            attempt_log,
        }) => emit_logged(
            platform_render::render_history(result, format, output, verbose),
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
                ContentOutputFormat::Json => {
                    format_search_failure_json(&failure, journal, verbose)?
                }
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
    emit_rendered(with_attempt_log(rendered, attempt_log))
}

/// Appends the attempt log to the diagnostics of `rendered`, leaving its stdout as it is.
fn with_attempt_log(
    rendered: Result<RenderedOutput, String>,
    attempt_log: Option<String>,
) -> Result<RenderedOutput, String> {
    rendered.map(|mut rendered| {
        rendered.stderr =
            app::combine_diagnostics([rendered.stderr.take(), attempt_log].into_iter().flatten());
        rendered
    })
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
    verbose: bool,
) -> Result<String, String> {
    let mut payload: Value = serde_json::from_str(&format_failure_json(&failure.error, verbose)?)
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
        if verbose {
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
    verbose: bool,
) -> Result<RenderedOutput, String> {
    let (stdout, exit_code, diagnostic) = match result {
        Ok(outcome) => {
            let stdout = match format {
                OutputFormat::Json => result_json(&outcome, &outcome.attempts, verbose)?,
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
                OutputFormat::Json => format_failure_json(&error, verbose)?,
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
    verbose: bool,
) -> Result<RenderedOutput, String> {
    let (stdout, exit_code, diagnostic) = match result {
        Ok(outcome) => {
            let stdout = match format {
                ContentOutputFormat::Json => result_json(&outcome, &outcome.attempts, verbose)?,
                ContentOutputFormat::Markdown => {
                    format!("# Fetched with {}\n\n{}", outcome.provider, outcome.content)
                }
                ContentOutputFormat::Content => outcome.content.clone(),
            };
            (stdout, 0, outcome.diagnostic)
        }
        Err(error) => {
            let stdout = match format {
                ContentOutputFormat::Json => format_failure_json(&error, verbose)?,
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
    verbose: bool,
) -> Result<RenderedOutput, String> {
    let (stdout, exit_code, diagnostic) = match result {
        Ok(outcome) => (
            format_success(&outcome, format, verbose)?,
            0,
            outcome.diagnostic,
        ),
        Err(error) => (
            format_failure(&error, format, verbose)?,
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

fn format_success(
    outcome: &ExaOutcome,
    format: OutputFormat,
    verbose: bool,
) -> Result<String, String> {
    match format {
        OutputFormat::Json => result_json(outcome, &outcome.attempts, verbose),
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

fn format_failure(
    error: &ProviderError,
    format: OutputFormat,
    verbose: bool,
) -> Result<String, String> {
    match format {
        OutputFormat::Json => format_failure_json(error, verbose),
        OutputFormat::Markdown => Ok(format!(
            "# Exa search failed\n\n**{}**: {}",
            error.kind.as_str(),
            error.message
        )),
    }
}

/// The JSON of a successful `result`, followed by its `attempts` as `provider_attempts` when
/// `verbose`.
fn result_json<T: Serialize>(
    result: &T,
    attempts: &[ProviderAttempt],
    verbose: bool,
) -> Result<String, String> {
    #[derive(Serialize)]
    struct WithAttempts<'a, T> {
        #[serde(flatten)]
        result: &'a T,
        provider_attempts: &'a [ProviderAttempt],
    }
    if verbose {
        serde_json::to_string(&WithAttempts {
            result,
            provider_attempts: attempts,
        })
    } else {
        serde_json::to_string(result)
    }
    .map_err(|error| error.to_string())
}

fn format_failure_json(error: &ProviderError, verbose: bool) -> Result<String, String> {
    encode_failure_payload(&failure_payload(error, verbose)?, verbose)
}

/// The fields of the stable failure payload, for commands that add their own fields.
fn failure_payload(
    error: &ProviderError,
    verbose: bool,
) -> Result<serde_json::Map<String, Value>, String> {
    let mut payload = serde_json::Map::new();
    payload.insert("error_kind".into(), error.kind.as_str().into());
    payload.insert(
        "message".into(),
        error.message.chars().take(500).collect::<String>().into(),
    );
    payload.insert("attempts".into(), bounded_attempt_summary(&error.attempts));
    payload.insert("journal_ref".into(), Value::Null);
    payload.insert("journal_status".into(), "not_applicable".into());
    if verbose {
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
    payload: &serde_json::Map<String, Value>,
    verbose: bool,
) -> Result<String, String> {
    let encoded = serde_json::to_string(payload).map_err(|error| error.to_string())?;
    if !verbose && encoded.len() > FAILURE_PAYLOAD_TARGET_BYTES {
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
    use forager::types::{
        AttemptDisposition, AttemptErrorKind, AttemptTarget, Capability, CapabilityGap,
        ProviderAttempt, SearchCandidate, SearchOutcome, Source,
    };
    use serde_json::{Value, json};

    use super::{
        ContentOutputFormat, FAILURE_PAYLOAD_TARGET_BYTES, FetchOutcome, JournalOutcome,
        OutputTarget, RenderedOutput, SearchFailure, apply_tee, fit_failure_candidates,
        format_failure_json, format_search_failure_json, postflight_exit_code, render_fetch,
        render_search, with_attempt_log,
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
            diagnostic: None,
        };
        let encoded =
            format_failure_json(&provider_error, false).expect("bounded provider failure payload");
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
            let encoded = format_failure_json(
                &ProviderError {
                    kind: AttemptErrorKind::RateLimited,
                    message: "rate limit detail".into(),
                    attempts: attempts.clone(),
                    diagnostic: None,
                },
                verbose,
            )
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

    #[test]
    fn a_fetched_body_keeps_its_diagnostic_for_standard_error() {
        let rendered = render_fetch(
            Ok(FetchOutcome {
                provider: "jina",
                url: "https://example.test/large".into(),
                content: "body".into(),
                attempts: Vec::new(),
                diagnostic: Some("content truncated at 4 MiB".into()),
            }),
            ContentOutputFormat::Json,
            None,
            false,
        )
        .expect("rendered output");
        let payload: Value = serde_json::from_str(&rendered.stdout).expect("fetch JSON");

        assert_eq!(
            (
                rendered.exit_code,
                &payload["content"],
                rendered.stderr.as_deref()
            ),
            (0, &json!("body"), Some("content truncated at 4 MiB"))
        );
    }

    #[test]
    fn a_successful_result_lists_its_attempts_only_when_verbose() {
        let listed = [false, true].map(|verbose| {
            let rendered = render_fetch(
                Ok(FetchOutcome {
                    provider: "jina",
                    url: "https://example.test/page".into(),
                    content: "body".into(),
                    attempts: vec![attempt("jina", AttemptErrorKind::Network)],
                    diagnostic: None,
                }),
                ContentOutputFormat::Json,
                None,
                verbose,
            )
            .expect("rendered output");
            let payload: Value = serde_json::from_str(&rendered.stdout).expect("fetch JSON");
            payload
                .get("provider_attempts")
                .and_then(Value::as_array)
                .map(Vec::len)
        });

        assert_eq!(listed, [None, Some(1)]);
    }

    /// The JSON payload of a search whose main search failed for a bad key, with `gaps` and the
    /// Search Candidates `extra_sources`, in full when `verbose`.
    fn search_failure_payload(
        gaps: Vec<CapabilityGap>,
        extra_sources: Vec<SearchCandidate>,
        verbose: bool,
    ) -> Value {
        let encoded = format_search_failure_json(
            &SearchFailure {
                error: ProviderError {
                    kind: AttemptErrorKind::Auth,
                    message: "bad key".into(),
                    attempts: Vec::new(),
                    diagnostic: None,
                },
                extra_sources,
                capability_gaps: gaps,
            },
            &JournalOutcome {
                status: "written",
                reference: Some("/journal/record.json".into()),
                warning: None,
            },
            verbose,
        )
        .expect("search failure payload");
        serde_json::from_str(&encoded).expect("search failure JSON")
    }

    /// The candidate Tavily's web search found at `url` titled `title`, with `summary`.
    fn web_search_candidate(title: &str, url: &str, summary: Option<&str>) -> SearchCandidate {
        SearchCandidate::from_source(
            Source {
                text: summary.map(Into::into),
                ..source(title, url)
            },
            "tavily",
            Capability::WebSearch,
        )
        .expect("an HTTP candidate")
    }

    #[test]
    fn a_failed_search_payload_names_its_capability_gaps() {
        let payload = search_failure_payload(
            vec![CapabilityGap {
                capability: Capability::DocsSearch,
                reason: "no_configured_provider",
                providers_skipped: vec!["exa".into()],
            }],
            Vec::new(),
            false,
        );

        assert_eq!(
            (&payload["error_kind"], &payload["capability_gaps"]),
            (
                &json!("auth"),
                &json!([{
                    "capability": "docs_search",
                    "reason": "no_configured_provider",
                    "providers_skipped": ["exa"]
                }]),
            )
        );
    }

    #[test]
    fn a_failed_search_payload_drops_candidate_summaries_unless_verbose() {
        let candidate = json!({
            "provider": "tavily",
            "capability": "web_search",
            "title": "Candidate",
            "url": "https://example.test/candidate",
            "summary": "Candidate summary",
            "provider_data": {}
        });
        let mut without_summary = candidate.clone();
        without_summary
            .as_object_mut()
            .expect("candidate object")
            .remove("summary");

        let payloads = [false, true].map(|verbose| {
            let payload = search_failure_payload(
                Vec::new(),
                vec![web_search_candidate(
                    "Candidate",
                    "https://example.test/candidate",
                    Some("Candidate summary"),
                )],
                verbose,
            );
            (
                payload["extra_sources"].clone(),
                payload.get("extra_sources_truncated").cloned(),
            )
        });

        assert_eq!(
            payloads,
            [
                (json!([without_summary]), Some(json!(false))),
                (json!([candidate]), None),
            ]
        );
    }

    #[test]
    fn a_failed_search_payload_cuts_its_candidates_to_the_payload_target() {
        let candidates = (0..20)
            .map(|index| {
                web_search_candidate(
                    &format!("{index} {}", "long title ".repeat(30)),
                    &format!("https://example.test/candidate/{index}"),
                    None,
                )
            })
            .collect::<Vec<_>>();

        let payload = search_failure_payload(Vec::new(), candidates, false);
        let kept = payload["extra_sources"].as_array().map_or(0, Vec::len);

        assert_eq!(
            (
                &payload["extra_sources_truncated"],
                payload.to_string().len() <= FAILURE_PAYLOAD_TARGET_BYTES,
                (1..20).contains(&kept),
            ),
            (&json!(true), true, true),
            "kept {kept}"
        );
    }

    #[test]
    fn a_search_keeps_its_capability_gap_diagnostic_for_standard_error() {
        let rendered = render_search(
            Ok(SearchOutcome {
                provider: "xai",
                answer: "answer".into(),
                sources: Vec::new(),
                extra_sources: Vec::new(),
                capabilities: vec![Capability::WebSearch],
                capability_gaps: Vec::new(),
                attempts: Vec::new(),
                diagnostic: Some("capability gap: web_search has no configured provider".into()),
            }),
            &JournalOutcome {
                status: "disabled",
                reference: None,
                warning: None,
            },
            ContentOutputFormat::Json,
            None,
            false,
        )
        .expect("rendered output");

        assert_eq!(
            (rendered.exit_code, rendered.stderr.as_deref()),
            (
                0,
                Some("capability gap: web_search has no configured provider")
            )
        );
    }

    fn source(title: &str, url: &str) -> Source {
        Source {
            title: title.into(),
            url: url.into(),
            published_date: None,
            author: None,
            text: None,
            highlights: Vec::new(),
            id: None,
            image: None,
            favicon: None,
        }
    }

    /// xAI's answer `answer` citing one primary source, with no extra sources.
    fn answered(answer: &str) -> SearchOutcome {
        SearchOutcome {
            provider: "xai",
            answer: answer.into(),
            sources: vec![source(
                "Primary",
                "https://example.test/source?token=********",
            )],
            extra_sources: Vec::new(),
            capabilities: Vec::new(),
            capability_gaps: Vec::new(),
            attempts: Vec::new(),
            diagnostic: None,
        }
    }

    fn written_journal() -> JournalOutcome {
        JournalOutcome {
            status: "written",
            reference: Some("/journal/search_result_1_2_3.json".into()),
            warning: None,
        }
    }

    #[test]
    fn a_search_payload_leaves_the_invocation_echoes_to_the_journal() {
        let rendered = render_search(
            Ok(SearchOutcome {
                capabilities: vec![Capability::DocsSearch, Capability::WebSearch],
                attempts: vec![attempt("xai", AttemptErrorKind::Network)],
                ..answered("Focused answer")
            }),
            &written_journal(),
            ContentOutputFormat::Json,
            None,
            false,
        )
        .expect("rendered output");
        let payload: Value = serde_json::from_str(&rendered.stdout).expect("search JSON");

        assert_eq!(
            (rendered.exit_code, payload),
            (
                0,
                json!({
                    "answer": "Focused answer",
                    "sources": [{
                        "title": "Primary",
                        "url": "https://example.test/source?token=********"
                    }],
                    "capability_gaps": [],
                    "journal_ref": "/journal/search_result_1_2_3.json",
                    "journal_status": "written"
                })
            )
        );
    }

    #[test]
    fn a_search_in_markdown_lists_extra_sources_after_the_primary_ones_titled_by_url_if_untitled() {
        let untitled = SearchCandidate::from_source(
            Source {
                text: Some("Firecrawl summary".into()),
                ..source("", "https://example.test/untitled")
            },
            "firecrawl",
            Capability::WebSearch,
        )
        .expect("an HTTP candidate");
        let preview = SearchCandidate::from_web_fetch(
            "jina",
            "https://example.test/article".into(),
            "Fetched evidence line one.".into(),
        )
        .expect("an HTTP candidate");

        let rendered = render_search(
            Ok(SearchOutcome {
                extra_sources: vec![untitled, preview],
                ..answered("answer")
            }),
            &written_journal(),
            ContentOutputFormat::Markdown,
            None,
            false,
        )
        .expect("rendered output");

        assert_eq!(
            (rendered.exit_code, rendered.stdout.as_str()),
            (
                0,
                "# Search result\n\nanswer\n\n## Primary Sources\n\n- [Primary](https://example.test/source?token=********)\n\n## Extra Sources\n\n- [https://example.test/untitled](https://example.test/untitled) — firecrawl\n\n  Firecrawl summary\n- [https://example.test/article](https://example.test/article) — jina\n\n  Fetched evidence line one."
            )
        );
    }

    #[test]
    fn a_journal_that_failed_leaves_the_search_successful_and_warns_once() {
        let rendered = render_search(
            Ok(answered("answer")),
            &JournalOutcome {
                status: "failed",
                reference: None,
                warning: Some("File exists (os error 17)".into()),
            },
            ContentOutputFormat::Json,
            None,
            false,
        )
        .expect("rendered output");
        let payload: Value = serde_json::from_str(&rendered.stdout).expect("search JSON");

        assert_eq!(
            (
                rendered.exit_code,
                &payload["answer"],
                &payload["journal_status"],
                &payload["journal_ref"],
                rendered.stderr.as_deref(),
            ),
            (
                0,
                &json!("answer"),
                &json!("failed"),
                &Value::Null,
                Some("Search Result Journal warning: File exists (os error 17)"),
            )
        );
    }

    #[test]
    fn a_search_that_ran_out_of_time_prints_its_failure_payload_and_exits_four() {
        let rendered = render_search(
            Err(SearchFailure {
                error: ProviderError {
                    kind: AttemptErrorKind::Timeout,
                    message: "deadline exceeded".into(),
                    attempts: vec![attempt("xai", AttemptErrorKind::Timeout)],
                    diagnostic: None,
                },
                extra_sources: Vec::new(),
                capability_gaps: Vec::new(),
            }),
            &written_journal(),
            ContentOutputFormat::Json,
            None,
            false,
        )
        .expect("rendered output");
        let payload: Value = serde_json::from_str(&rendered.stdout).expect("failure JSON");

        assert_eq!(
            (rendered.exit_code, &payload["error_kind"]),
            (4, &json!("timeout"))
        );
    }

    #[test]
    fn the_attempt_log_joins_the_diagnostics_without_changing_stdout() {
        let rendered = || {
            Ok(RenderedOutput {
                stdout: r#"{"answer":"answer"}"#.into(),
                stderr: Some("diagnostic".into()),
                exit_code: 0,
            })
        };

        let outputs = [None, Some("forager attempts: {}".to_owned())].map(|attempt_log| {
            let output = with_attempt_log(rendered(), attempt_log).expect("rendered output");
            (output.stdout, output.stderr, output.exit_code)
        });

        assert_eq!(
            outputs,
            [
                (
                    r#"{"answer":"answer"}"#.to_owned(),
                    Some("diagnostic".to_owned()),
                    0
                ),
                (
                    r#"{"answer":"answer"}"#.to_owned(),
                    Some("diagnostic\nforager attempts: {}".to_owned()),
                    0
                ),
            ]
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

    #[test]
    fn a_content_failure_exits_five_and_every_other_attempt_failure_exits_four() {
        let codes = [
            (AttemptErrorKind::Auth, 4),
            (AttemptErrorKind::RateLimited, 4),
            (AttemptErrorKind::QuotaExhausted, 4),
            (AttemptErrorKind::Parameter, 4),
            (AttemptErrorKind::Timeout, 4),
            (AttemptErrorKind::Network, 4),
            (AttemptErrorKind::Runtime, 4),
            (AttemptErrorKind::Quality, 5),
            (AttemptErrorKind::Evidence, 5),
        ];

        assert_eq!(
            codes.map(|(kind, _)| (kind, postflight_exit_code(kind))),
            codes
        );
    }

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
