use std::time::{Duration, Instant};

use reqwest::Client;
use serde_json::{Map, Value, json};

use crate::config::Context7RuntimeConfig;
use crate::credentials::CredentialPool;
use crate::net::{
    CONTENT_TRUNCATED_DIAGNOSTIC, McpClient, McpError, McpToolResult, RetryPolicy,
    combine_diagnostics, duration_millis,
};
use crate::providers::ProviderError;
use crate::providers::shared::redacted_urls_message;
use crate::types::{
    AttemptErrorKind, Context7DocsOutcome, Context7LibraryOutcome, Context7Outcome, Deadline,
    LibraryCandidate, ProviderAttempt,
};

#[derive(Clone, Debug)]
pub(crate) struct Context7LibraryRequest {
    pub(crate) name: String,
    pub(crate) query: String,
    pub(crate) verbose: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Context7DocsRequest {
    pub(crate) library_id: String,
    pub(crate) query: String,
    pub(crate) verbose: bool,
}

pub(crate) struct Context7 {
    config: Context7RuntimeConfig,
    client: Client,
    credentials: CredentialPool,
    retry_policy: RetryPolicy,
    deadline: Deadline,
}

impl Context7 {
    pub(crate) fn new(
        config: Context7RuntimeConfig,
        client: Client,
        credentials: CredentialPool,
        retry_policy: RetryPolicy,
        deadline: Deadline,
    ) -> Self {
        Self {
            config,
            client,
            credentials,
            retry_policy,
            deadline,
        }
    }

    pub(crate) async fn library(
        &self,
        request: Context7LibraryRequest,
    ) -> Result<Context7Outcome, ProviderError> {
        self.execute(Context7Operation::Library(request)).await
    }

    pub(crate) async fn docs(
        &self,
        request: Context7DocsRequest,
    ) -> Result<Context7Outcome, ProviderError> {
        self.execute(Context7Operation::Docs(request)).await
    }

    // The retry loop stays contiguous so redirects, rotation, and diagnostics share one lifecycle.
    #[expect(clippy::too_many_lines)]
    async fn execute(
        &self,
        operation: Context7Operation,
    ) -> Result<Context7Outcome, ProviderError> {
        let selection = self.credentials.claim();
        let mut attempts = Vec::new();
        let mut credential_index = selection.index;
        let mut retry_count = 0;
        let mut rotation_count = 0;

        loop {
            let Some(remaining) = self.deadline.remaining() else {
                return Err(Self::terminal_error(
                    AttemptErrorKind::Timeout,
                    attempts,
                    operation.verbose(),
                    selection.diagnostic.clone(),
                    None,
                ));
            };
            let attempt_limit = remaining.min(Duration::from_secs(self.config.timeout_seconds));
            let attempt_deadline = Deadline::new(attempt_limit);
            let started = Instant::now();
            let client = McpClient::new(&self.client, &self.config.url, attempt_deadline);
            let result = client
                .call_tool(
                    self.credentials.key(credential_index),
                    operation.tool(),
                    operation.arguments(),
                )
                .await
                .map_err(Context7Failure::from)
                .and_then(|result| {
                    let truncated = result.truncated;
                    operation.decode(result).map(|outcome| (outcome, truncated))
                });

            match result {
                Ok((outcome, truncated)) => {
                    attempts.push(ProviderAttempt {
                        provider: "context7",
                        seam: "docs_search",
                        error_kind: None,
                        http_status: Some(200),
                        duration_ms: duration_millis(started.elapsed()),
                        credential_index,
                        retry_count,
                        rotation_count,
                        message: String::new(),
                        model: None,
                        transport: Some("mcp"),
                        endpoint_host: None,
                        breaker_event: None,
                    });
                    let visible_attempts = if operation.verbose() {
                        attempts
                    } else {
                        Vec::new()
                    };
                    let diagnostic = combine_diagnostics(
                        [
                            selection.diagnostic,
                            truncated.then(|| CONTENT_TRUNCATED_DIAGNOSTIC.to_owned()),
                        ]
                        .into_iter()
                        .flatten(),
                    );
                    let outcome = operation.outcome(outcome, visible_attempts, diagnostic);
                    return Ok(self.redact_outcome(outcome));
                }
                Err(failure) => {
                    let kind = failure.kind;
                    let redirect = failure
                        .redirected_library_id
                        .as_deref()
                        .map(|target| redacted_urls_message(target, &self.credentials));
                    attempts.push(ProviderAttempt {
                        provider: "context7",
                        seam: "docs_search",
                        error_kind: Some(kind),
                        http_status: failure.status,
                        duration_ms: duration_millis(started.elapsed()),
                        credential_index,
                        retry_count,
                        rotation_count,
                        message: redacted_urls_message(&failure.message, &self.credentials),
                        model: None,
                        transport: Some("mcp"),
                        endpoint_host: None,
                        breaker_event: None,
                    });

                    if kind.rotates_credential() && rotation_count + 1 < self.credentials.len() {
                        rotation_count += 1;
                        credential_index = self
                            .credentials
                            .rotated_index(selection.index, rotation_count);
                        continue;
                    }
                    if kind.is_retryable()
                        && retry_count.saturating_add(1) < self.retry_policy.max_attempts()
                    {
                        retry_count += 1;
                        let wait = self.retry_policy.wait(retry_count);
                        let Some(remaining) = self.deadline.remaining() else {
                            return Err(Self::terminal_error(
                                AttemptErrorKind::Timeout,
                                attempts,
                                operation.verbose(),
                                selection.diagnostic.clone(),
                                redirect,
                            ));
                        };
                        if wait >= remaining {
                            return Err(Self::terminal_error(
                                AttemptErrorKind::Timeout,
                                attempts,
                                operation.verbose(),
                                selection.diagnostic.clone(),
                                redirect,
                            ));
                        }
                        tokio::time::sleep(wait).await;
                        continue;
                    }
                    return Err(Self::terminal_error(
                        kind,
                        attempts,
                        operation.verbose(),
                        selection.diagnostic,
                        redirect,
                    ));
                }
            }
        }
    }

    fn terminal_error(
        kind: AttemptErrorKind,
        attempts: Vec<ProviderAttempt>,
        verbose: bool,
        diagnostic: Option<String>,
        redirected_library_id: Option<String>,
    ) -> ProviderError {
        let message = attempts.last().map_or_else(
            || "Context7 request failed".to_owned(),
            |attempt| attempt.message.clone(),
        );
        ProviderError {
            kind,
            message,
            attempts,
            verbose,
            diagnostic,
            redirected_library_id,
        }
    }

    fn redact_outcome(&self, mut outcome: Context7Outcome) -> Context7Outcome {
        match &mut outcome {
            Context7Outcome::Library(outcome) => {
                outcome.query = self.credentials.redact(&outcome.query);
                for library in &mut outcome.results {
                    library.id = self.credentials.redact(&library.id);
                    library.title = self.credentials.redact(&library.title);
                    library.description = self.credentials.redact(&library.description);
                }
            }
            Context7Outcome::Docs(outcome) => {
                outcome.library_id = self.credentials.redact(&outcome.library_id);
                outcome.query = self.credentials.redact(&outcome.query);
                outcome.content = self.credentials.redact(&outcome.content);
                for value in outcome
                    .code_snippets
                    .iter_mut()
                    .chain(&mut outcome.info_snippets)
                    .chain(&mut outcome.results)
                {
                    redact_json(value, &self.credentials);
                }
            }
        }
        outcome
    }
}

enum Context7Operation {
    Library(Context7LibraryRequest),
    Docs(Context7DocsRequest),
}

impl Context7Operation {
    fn tool(&self) -> &'static str {
        match self {
            Self::Library(_) => "resolve-library-id",
            Self::Docs(_) => "query-docs",
        }
    }

    fn arguments(&self) -> Value {
        match self {
            Self::Library(request) => {
                json!({"libraryName": request.name, "query": request.query})
            }
            Self::Docs(request) => {
                json!({"libraryId": request.library_id, "query": request.query})
            }
        }
    }

    fn verbose(&self) -> bool {
        match self {
            Self::Library(request) => request.verbose,
            Self::Docs(request) => request.verbose,
        }
    }

    fn decode(&self, result: McpToolResult) -> Result<DecodedOutcome, Context7Failure> {
        let data = merged_data(result.structured_content, &result.text);
        if let Some(target) = redirect_target(&data, &result.text) {
            return Err(Context7Failure {
                kind: AttemptErrorKind::Runtime,
                status: None,
                message: format!("Context7 library ID was redirected to {target}"),
                redirected_library_id: Some(target),
            });
        }
        Ok(match self {
            Self::Library(_) => {
                let mut results = library_results(&data);
                if results.is_empty() {
                    results = library_results_from_text(&result.text);
                }
                DecodedOutcome::Libraries(results)
            }
            Self::Docs(_) => {
                let code_snippets = array_field(&data, &["codeSnippets", "code_snippets"]);
                let info_snippets = array_field(&data, &["infoSnippets", "info_snippets"]);
                let results = array_field(&data, &["results", "items"]);
                let content = data
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or(&result.text)
                    .to_owned();
                DecodedOutcome::Docs {
                    content,
                    code_snippets,
                    info_snippets,
                    results,
                }
            }
        })
    }

    fn outcome(
        &self,
        decoded: DecodedOutcome,
        attempts: Vec<ProviderAttempt>,
        diagnostic: Option<String>,
    ) -> Context7Outcome {
        match (self, decoded) {
            (Self::Library(request), DecodedOutcome::Libraries(results)) => {
                let total = results.len();
                Context7Outcome::Library(Context7LibraryOutcome {
                    provider: "context7",
                    query: format!("{} {}", request.name, request.query)
                        .trim()
                        .to_owned(),
                    results,
                    total,
                    attempts,
                    diagnostic,
                })
            }
            (
                Self::Docs(request),
                DecodedOutcome::Docs {
                    content,
                    code_snippets,
                    info_snippets,
                    mut results,
                },
            ) => {
                if results.is_empty() {
                    results.extend(code_snippets.iter().chain(&info_snippets).cloned());
                }
                let total = results.len();
                Context7Outcome::Docs(Context7DocsOutcome {
                    provider: "context7",
                    library_id: request.library_id.clone(),
                    query: request.query.clone(),
                    content,
                    code_snippets,
                    info_snippets,
                    results,
                    total,
                    attempts,
                    diagnostic,
                })
            }
            _ => unreachable!("operation and decoded outcome always have matching variants"),
        }
    }
}

impl From<McpError> for Context7Failure {
    fn from(error: McpError) -> Self {
        Self {
            kind: error.kind,
            status: error.status,
            message: error.message,
            redirected_library_id: None,
        }
    }
}

enum DecodedOutcome {
    Libraries(Vec<LibraryCandidate>),
    Docs {
        content: String,
        code_snippets: Vec<Value>,
        info_snippets: Vec<Value>,
        results: Vec<Value>,
    },
}

struct Context7Failure {
    kind: AttemptErrorKind,
    status: Option<u16>,
    message: String,
    redirected_library_id: Option<String>,
}

fn merged_data(mut data: Value, text: &str) -> Value {
    let parsed = serde_json::from_str::<Value>(text)
        .ok()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    let Some(data_object) = data.as_object_mut() else {
        return parsed;
    };
    if let Some(parsed_object) = parsed.as_object() {
        let mut merged = parsed_object.clone();
        merged.extend(std::mem::take(data_object));
        Value::Object(merged)
    } else {
        data
    }
}

fn library_results(data: &Value) -> Vec<LibraryCandidate> {
    ["results", "libraries", "items"]
        .iter()
        .find_map(|key| data.get(key).and_then(Value::as_array))
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .map(normalize_library)
        .collect()
}

fn library_results_from_text(text: &str) -> Vec<LibraryCandidate> {
    text.split("\n----")
        .filter_map(|block| {
            let mut fields = Map::new();
            for line in block.lines() {
                let Some((label, value)) = line
                    .strip_prefix("- ")
                    .and_then(|line| line.split_once(':'))
                else {
                    continue;
                };
                let key = match label.trim() {
                    "Title" => "title",
                    "Context7-compatible library ID" => "id",
                    "Description" => "description",
                    "Code Snippets" => "totalSnippets",
                    "Trust Score" => "trustScore",
                    "Benchmark Score" => "benchmarkScore",
                    _ => continue,
                };
                let value = value.trim();
                let parsed = value
                    .replace(',', "")
                    .parse::<u64>()
                    .map(Value::from)
                    .or_else(|_| value.parse::<f64>().map(Value::from))
                    .unwrap_or_else(|_| Value::String(value.to_owned()));
                fields.insert(key.to_owned(), parsed);
            }
            fields
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| id.starts_with('/'))
                .then(|| normalize_library(&fields))
        })
        .collect()
}

fn normalize_library(item: &Map<String, Value>) -> LibraryCandidate {
    LibraryCandidate {
        id: string_field(item, &["id", "libraryId"]),
        title: string_field(item, &["title", "name"]),
        description: string_field(item, &["description"]),
        trust_score: number_field(item, &["trustScore"]),
        benchmark_score: number_field(item, &["benchmarkScore"]),
        total_snippets: integer_field(item, &["totalSnippets"]),
        stars: integer_field(item, &["stars"]),
        provider: "context7",
    }
}

fn string_field(item: &Map<String, Value>, names: &[&str]) -> String {
    names
        .iter()
        .find_map(|name| item.get(*name).and_then(Value::as_str))
        .unwrap_or_default()
        .to_owned()
}

fn number_field(item: &Map<String, Value>, names: &[&str]) -> Option<f64> {
    names
        .iter()
        .find_map(|name| item.get(*name).and_then(Value::as_f64))
}

fn integer_field(item: &Map<String, Value>, names: &[&str]) -> Option<u64> {
    names
        .iter()
        .find_map(|name| item.get(*name).and_then(Value::as_u64))
}

fn array_field(data: &Value, names: &[&str]) -> Vec<Value> {
    names
        .iter()
        .find_map(|name| data.get(*name).and_then(Value::as_array))
        .cloned()
        .unwrap_or_default()
}

fn redirect_target(data: &Value, text: &str) -> Option<String> {
    redirect_target_in_data(data).or_else(|| {
        let lower = text.to_ascii_lowercase();
        if !lower.contains("redirect") {
            return None;
        }
        text.split_whitespace()
            .map(|part| part.trim_matches(['`', '.', ',', ':']))
            .find(|part| part.starts_with('/') && part.len() > 1)
            .map(ToOwned::to_owned)
    })
}

fn redirect_target_in_data(data: &Value) -> Option<String> {
    match data {
        Value::Object(fields) => {
            for name in [
                "redirectedLibraryId",
                "redirected_library_id",
                "redirectTarget",
                "redirect_target",
            ] {
                if let Some(target) = fields
                    .get(name)
                    .and_then(Value::as_str)
                    .filter(|target| target.starts_with('/'))
                {
                    return Some(target.to_owned());
                }
            }
            fields.values().find_map(redirect_target_in_data)
        }
        Value::Array(values) => values.iter().find_map(redirect_target_in_data),
        _ => None,
    }
}

fn redact_json(value: &mut Value, credentials: &CredentialPool) {
    match value {
        Value::String(text) => *text = credentials.redact(text),
        Value::Array(values) => {
            for value in values {
                redact_json(value, credentials);
            }
        }
        Value::Object(fields) => {
            for value in fields.values_mut() {
                redact_json(value, credentials);
            }
        }
        _ => {}
    }
}
