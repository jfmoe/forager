use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use reqwest::{Client, Response};
use serde::Serialize;
use serde_json::Value;

use crate::config::OpenAiCompatibleRuntimeConfig;
use crate::credentials::CredentialPool;
use crate::net::{RetryPolicy, error_kind_for_status, slice_budget};
use crate::providers::execution::{AttemptFailure, ExecutionSettings, execute};
use crate::providers::shared::redacted_urls_message;
use crate::providers::xai::SearchRequest;
use crate::providers::{MainSearchRequestKind, ProviderError};
use crate::redact::{Secret, redact_url, redact_urls};
use crate::types::{AttemptErrorKind, Deadline, SearchOutcome, Source};

pub(crate) struct OpenAiCompatible {
    config: OpenAiCompatibleRuntimeConfig,
    client: Client,
    credentials: CredentialPool,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    breakers: Arc<ModelBreakers>,
}

const BREAKER_FAILURE_THRESHOLD: u8 = 2;
const BREAKER_COOLDOWN: Duration = Duration::from_mins(10);

#[derive(Clone, Copy)]
struct TransportAttempt {
    stream: bool,
    fallback_from: Option<&'static str>,
}

#[derive(Default)]
pub(crate) struct ModelBreakers {
    states: Mutex<HashMap<(String, String), BreakerState>>,
}

#[derive(Clone, Copy)]
struct BreakerState {
    consecutive_failures: u8,
    opened_until: Option<Instant>,
}

impl ModelBreakers {
    fn is_open(&self, url: &str, model: &str) -> bool {
        let mut states = self
            .states
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let key = (url.trim_end_matches('/').to_owned(), model.to_owned());
        let Some(state) = states.get(&key).copied() else {
            return false;
        };
        if state
            .opened_until
            .is_some_and(|until| until > Instant::now())
        {
            return true;
        }
        if state.opened_until.is_some() {
            states.remove(&key);
        }
        false
    }

    fn record_success(&self, url: &str, model: &str) {
        self.states
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&(url.trim_end_matches('/').to_owned(), model.to_owned()));
    }

    fn record_failure(&self, url: &str, model: &str) -> bool {
        let mut states = self
            .states
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let state = states
            .entry((url.trim_end_matches('/').to_owned(), model.to_owned()))
            .or_insert(BreakerState {
                consecutive_failures: 0,
                opened_until: None,
            });
        state.consecutive_failures = state.consecutive_failures.saturating_add(1);
        if state.consecutive_failures >= BREAKER_FAILURE_THRESHOLD {
            state.opened_until = Some(Instant::now() + BREAKER_COOLDOWN);
            true
        } else {
            false
        }
    }
}

impl OpenAiCompatible {
    pub(crate) fn new(
        config: OpenAiCompatibleRuntimeConfig,
        client: Client,
        credentials: CredentialPool,
        retry_policy: RetryPolicy,
        deadline: Deadline,
        breakers: Arc<ModelBreakers>,
    ) -> Self {
        Self {
            config,
            client,
            credentials,
            retry_policy,
            deadline,
            breakers,
        }
    }

    pub(crate) async fn search(
        &self,
        request: SearchRequest,
    ) -> Result<SearchOutcome, ProviderError> {
        self.execute(request, MainSearchRequestKind::Search).await
    }

    pub(crate) async fn probe(
        &self,
        request: SearchRequest,
    ) -> Result<SearchOutcome, ProviderError> {
        self.execute(request, MainSearchRequestKind::ModelProbe)
            .await
    }

    async fn execute(
        &self,
        request: SearchRequest,
        request_kind: MainSearchRequestKind,
    ) -> Result<SearchOutcome, ProviderError> {
        let query = request.query;
        let models = self.model_candidates(request.model.as_deref(), request.allow_model_fallback);
        let mut attempts = Vec::new();
        let mut diagnostic = None;
        let mut executable = Vec::new();
        for model in models {
            if self.breakers.is_open(&self.config.url, &model) {
                attempts.push(self.skipped_model_attempt(
                    &model,
                    "skipped because the model breaker is open",
                    Some("open"),
                ));
            } else {
                executable.push(model);
            }
        }
        for (index, model) in executable.iter().enumerate() {
            let Some(remaining) = self.deadline.remaining() else {
                break;
            };
            let slots = executable.len() - index;
            let Some(model_budget) = slice_budget(remaining, slots) else {
                attempts.push(self.skipped_model_attempt(
                    model,
                    "skipped to preserve model fallback deadline budget",
                    None,
                ));
                continue;
            };
            match self
                .execute_model(
                    &query,
                    model,
                    request.verbose,
                    Deadline::new(model_budget),
                    request_kind,
                )
                .await
            {
                Ok(mut execution) => {
                    self.breakers.record_success(&self.config.url, model);
                    attempts.append(&mut execution.attempts);
                    diagnostic = execution.diagnostic.or(diagnostic);
                    let (answer, sources) = execution.value;
                    return Ok(SearchOutcome {
                        provider: "openai_compatible",
                        query,
                        model: model.clone(),
                        answer,
                        sources,
                        extra_sources: Vec::new(),
                        validation_results: Vec::new(),
                        vertical_results: Vec::new(),
                        capabilities: Vec::new(),
                        capability_gaps: Vec::new(),
                        attempts,
                        diagnostic,
                    });
                }
                Err(mut error) => {
                    let opened = self.breakers.record_failure(&self.config.url, model);
                    if opened && let Some(attempt) = error.attempts.last_mut() {
                        attempt.breaker_event = Some("opened");
                    }
                    diagnostic = error.diagnostic.or(diagnostic);
                    attempts.append(&mut error.attempts);
                }
            }
        }
        let kind = attempts
            .last()
            .and_then(|attempt| attempt.error_kind)
            .unwrap_or(AttemptErrorKind::Timeout);
        let message = attempts.last().map_or_else(
            || "OpenAI-compatible model chain exhausted".into(),
            |attempt| attempt.message.clone(),
        );
        Err(ProviderError {
            kind,
            message,
            attempts,
            verbose: request.verbose,
            diagnostic,
            redirected_library_id: None,
        })
    }

    async fn execute_model(
        &self,
        query: &str,
        model: &str,
        verbose: bool,
        deadline: Deadline,
        request_kind: MainSearchRequestKind,
    ) -> Result<crate::providers::execution::ExecutionOutcome<(String, Vec<Source>)>, ProviderError>
    {
        if !self.config.stream {
            return self
                .execute_transport(
                    query,
                    model,
                    verbose,
                    deadline,
                    TransportAttempt {
                        stream: false,
                        fallback_from: None,
                    },
                    request_kind,
                )
                .await;
        }
        let stream_budget = deadline
            .remaining()
            .map(|remaining| remaining / 2)
            .unwrap_or_default();
        let stream = self
            .execute_transport(
                query,
                model,
                verbose,
                Deadline::new(stream_budget),
                TransportAttempt {
                    stream: true,
                    fallback_from: None,
                },
                request_kind,
            )
            .await;
        match stream {
            Ok(outcome) => Ok(outcome),
            Err(mut stream_error)
                if matches!(
                    stream_error.kind,
                    AttemptErrorKind::Timeout
                        | AttemptErrorKind::Network
                        | AttemptErrorKind::Runtime
                ) =>
            {
                let Some(remaining) = deadline.remaining() else {
                    return Err(stream_error);
                };
                let non_stream = self
                    .execute_transport(
                        query,
                        model,
                        verbose,
                        Deadline::new(remaining),
                        TransportAttempt {
                            stream: false,
                            fallback_from: Some("sse"),
                        },
                        request_kind,
                    )
                    .await;
                match non_stream {
                    Ok(mut outcome) => {
                        stream_error.attempts.append(&mut outcome.attempts);
                        outcome.attempts = stream_error.attempts;
                        outcome.diagnostic = outcome.diagnostic.or(stream_error.diagnostic.take());
                        Ok(outcome)
                    }
                    Err(mut error) => {
                        stream_error.attempts.append(&mut error.attempts);
                        error.attempts = stream_error.attempts;
                        error.diagnostic = error.diagnostic.or(stream_error.diagnostic);
                        Err(error)
                    }
                }
            }
            Err(error) => Err(error),
        }
    }

    async fn execute_transport(
        &self,
        query: &str,
        model: &str,
        verbose: bool,
        deadline: Deadline,
        transport: TransportAttempt,
        request_kind: MainSearchRequestKind,
    ) -> Result<crate::providers::execution::ExecutionOutcome<(String, Vec<Source>)>, ProviderError>
    {
        execute(
            &self.credentials,
            ExecutionSettings {
                provider: "openai_compatible",
                seam: "main_search",
                retry_policy: self.retry_policy,
                deadline,
                attempt_timeout: deadline.remaining().unwrap_or_default(),
                verbose,
                timeout_message: "OpenAI-compatible request timed out",
                model: Some(model.to_owned()),
                transport: Some(if transport.stream { "sse" } else { "http" }),
                endpoint_host: reqwest::Url::parse(&self.config.url)
                    .ok()
                    .and_then(|url| url.host_str().map(ToOwned::to_owned)),
                breaker_event: transport.fallback_from.map(|_| "transport_fallback"),
            },
            |credential| async move {
                self.send_once(query, model, &credential, transport.stream, request_kind)
                    .await
            },
        )
        .await
    }

    fn model_candidates(&self, override_model: Option<&str>, fallback: bool) -> Vec<String> {
        let mut models = Vec::new();
        let primary = override_model.unwrap_or(&self.config.model);
        push_model(&mut models, apply_online_suffix(&self.config.url, primary));
        if override_model.is_none() && fallback {
            for model in &self.config.fallback_models {
                push_model(&mut models, apply_online_suffix(&self.config.url, model));
            }
        }
        models
    }

    fn skipped_model_attempt(
        &self,
        model: &str,
        message: &str,
        breaker_event: Option<&'static str>,
    ) -> crate::types::ProviderAttempt {
        crate::types::ProviderAttempt {
            provider: "openai_compatible",
            seam: "main_search",
            error_kind: Some(AttemptErrorKind::Timeout),
            http_status: None,
            duration_ms: 0,
            credential_index: 0,
            retry_count: 0,
            rotation_count: 0,
            message: message.into(),
            model: Some(model.into()),
            transport: None,
            endpoint_host: reqwest::Url::parse(&self.config.url)
                .ok()
                .and_then(|url| url.host_str().map(ToOwned::to_owned)),
            breaker_event,
        }
    }

    async fn send_once(
        &self,
        query: &str,
        model: &str,
        credential: &Secret,
        stream: bool,
        request_kind: MainSearchRequestKind,
    ) -> Result<(u16, (String, Vec<Source>)), AttemptFailure> {
        let endpoint = format!("{}/chat/completions", self.config.url.trim_end_matches('/'));
        let input = request_kind.input(query);
        let mut messages = Vec::with_capacity(2);
        if let Some(instruction) = request_kind.instruction() {
            messages.push(Message {
                role: "system",
                content: instruction,
            });
        }
        messages.push(Message {
            role: "user",
            content: &input,
        });
        let response = self
            .client
            .post(endpoint)
            .bearer_auth(credential.expose())
            .header("accept", "application/json, text/event-stream")
            .json(&ChatRequest {
                model,
                messages,
                stream,
            })
            .send()
            .await
            .map_err(|error| self.request_failure(&error))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(AttemptFailure {
                kind: error_kind_for_status(status, &body),
                status: Some(status.as_u16()),
                message: redacted_urls_message(
                    if body.trim().is_empty() {
                        "OpenAI-compatible request failed"
                    } else {
                        &body
                    },
                    &self.credentials,
                ),
            });
        }
        let value = if stream {
            self.stream_response(response).await?
        } else {
            self.http_response(response).await?
        };
        Ok((status.as_u16(), value))
    }

    async fn http_response(
        &self,
        response: Response,
    ) -> Result<(String, Vec<Source>), AttemptFailure> {
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let body = response
            .text()
            .await
            .map_err(|error| self.request_failure(&error))?;
        if content_type.contains("text/event-stream") || body.trim_start().starts_with("data:") {
            return self.parse_sse_text(&body);
        }
        let value: Value = serde_json::from_str(&body).map_err(|error| AttemptFailure {
            kind: AttemptErrorKind::Runtime,
            status: Some(200),
            message: redacted_urls_message(
                &format!("OpenAI-compatible returned invalid JSON: {error}"),
                &self.credentials,
            ),
        })?;
        self.normalize(&value, false)
    }

    async fn stream_response(
        &self,
        response: Response,
    ) -> Result<(String, Vec<Source>), AttemptFailure> {
        let mut events = response.bytes_stream().eventsource();
        let mut answer = String::new();
        let mut sources = Vec::new();
        let mut completed = false;
        while let Some(event) = events.next().await {
            let event = event.map_err(|error| AttemptFailure {
                kind: AttemptErrorKind::Network,
                status: Some(200),
                message: redacted_urls_message(
                    &format!("OpenAI-compatible returned invalid SSE: {error}"),
                    &self.credentials,
                ),
            })?;
            completed |= self.consume_sse_data(&event.data, &mut answer, &mut sources)?;
        }
        finish(answer, sources, completed)
    }

    fn parse_sse_text(&self, body: &str) -> Result<(String, Vec<Source>), AttemptFailure> {
        let mut answer = String::new();
        let mut sources = Vec::new();
        let mut completed = false;
        for line in body.lines().map(str::trim) {
            let Some(data) = line.strip_prefix("data:").map(str::trim) else {
                continue;
            };
            completed |= self.consume_sse_data(data, &mut answer, &mut sources)?;
        }
        finish(answer, sources, completed)
    }

    fn consume_sse_data(
        &self,
        data: &str,
        answer: &mut String,
        sources: &mut Vec<Source>,
    ) -> Result<bool, AttemptFailure> {
        let data = data.trim();
        if data.is_empty() {
            return Ok(false);
        }
        if data == "[DONE]" {
            return Ok(true);
        }
        let value: Value = serde_json::from_str(data).map_err(|error| AttemptFailure {
            kind: AttemptErrorKind::Runtime,
            status: Some(200),
            message: redacted_urls_message(
                &format!("OpenAI-compatible returned invalid event JSON: {error}"),
                &self.credentials,
            ),
        })?;
        let completed = has_finish_reason(&value);
        let (delta, event_sources) = self.normalize(&value, true)?;
        answer.push_str(&delta);
        merge_sources(sources, event_sources);
        Ok(completed)
    }

    fn normalize(
        &self,
        value: &Value,
        streaming: bool,
    ) -> Result<(String, Vec<Source>), AttemptFailure> {
        let choice = value
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first());
        let content = choice
            .and_then(|choice| choice.get(if streaming { "delta" } else { "message" }))
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let mut sources = normalize_citations(value.get("citations"));
        if let Some(message) =
            choice.and_then(|choice| choice.get(if streaming { "delta" } else { "message" }))
        {
            merge_sources(&mut sources, normalize_citations(message.get("citations")));
        }
        if !streaming && content.trim().is_empty() {
            return Err(AttemptFailure {
                kind: AttemptErrorKind::Runtime,
                status: Some(200),
                message: "OpenAI-compatible response contained no answer".into(),
            });
        }
        Ok((self.redacted_text(content), self.redact_sources(sources)))
    }

    fn request_failure(&self, error: &reqwest::Error) -> AttemptFailure {
        AttemptFailure {
            kind: if error.is_timeout() {
                AttemptErrorKind::Timeout
            } else {
                AttemptErrorKind::Network
            },
            status: error.status().map(|status| status.as_u16()),
            message: redacted_urls_message(&error.to_string(), &self.credentials),
        }
    }

    fn redact_sources(&self, sources: Vec<Source>) -> Vec<Source> {
        let mut redacted = Vec::new();
        for mut source in sources {
            source.url = self.credentials.redact(&redact_url(&source.url));
            source.title = self.redacted_text(&source.title);
            if !redacted
                .iter()
                .any(|existing: &Source| existing.url == source.url)
            {
                redacted.push(source);
            }
        }
        redacted
    }

    fn redacted_text(&self, value: &str) -> String {
        self.credentials.redact(&redact_urls(value))
    }
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<Message<'a>>,
    stream: bool,
}

#[derive(Serialize)]
struct Message<'a> {
    role: &'static str,
    content: &'a str,
}

fn normalize_citations(value: Option<&Value>) -> Vec<Source> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|citation| {
            let (url, title) = if let Some(url) = citation.as_str() {
                (url, None)
            } else {
                (
                    citation
                        .get("url")
                        .or_else(|| citation.get("href"))
                        .or_else(|| citation.get("link"))
                        .and_then(Value::as_str)?,
                    citation
                        .get("title")
                        .or_else(|| citation.get("name"))
                        .or_else(|| citation.get("label"))
                        .and_then(Value::as_str),
                )
            };
            (url.starts_with("http://") || url.starts_with("https://")).then(|| Source {
                title: title.unwrap_or(url).trim().to_owned(),
                url: url.to_owned(),
                published_date: None,
                author: None,
                text: None,
                highlights: Vec::new(),
            })
        })
        .collect()
}

fn merge_sources(target: &mut Vec<Source>, sources: Vec<Source>) {
    for source in sources {
        if !target.iter().any(|existing| existing.url == source.url) {
            target.push(source);
        }
    }
}

fn has_finish_reason(value: &Value) -> bool {
    value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("finish_reason"))
        .is_some_and(|reason| !reason.is_null())
}

fn finish(
    answer: String,
    sources: Vec<Source>,
    completed: bool,
) -> Result<(String, Vec<Source>), AttemptFailure> {
    if !completed {
        return Err(AttemptFailure {
            kind: AttemptErrorKind::Runtime,
            status: Some(200),
            message: "OpenAI-compatible stream ended before a completion marker".into(),
        });
    }
    if answer.trim().is_empty() {
        return Err(AttemptFailure {
            kind: AttemptErrorKind::Runtime,
            status: Some(200),
            message: "OpenAI-compatible stream contained no answer".into(),
        });
    }
    Ok((answer, sources))
}

fn apply_online_suffix(url: &str, model: &str) -> String {
    if url.to_ascii_lowercase().contains("openrouter") && !model.ends_with(":online") {
        format!("{model}:online")
    } else {
        model.to_owned()
    }
}

fn push_model(models: &mut Vec<String>, model: String) {
    if !model.trim().is_empty() && !models.contains(&model) {
        models.push(model);
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    use crate::config::OpenAiCompatibleRuntimeConfig;
    use crate::credentials::CredentialPool;
    use crate::net::RetryPolicy;
    use crate::providers::xai::SearchRequest;
    use crate::types::{AttemptErrorKind, Deadline};

    use super::{
        BREAKER_FAILURE_THRESHOLD, BreakerState, ModelBreakers, OpenAiCompatible,
        apply_online_suffix, push_model,
    };

    #[test]
    fn openrouter_models_receive_one_online_suffix() {
        assert_eq!(
            (
                apply_online_suffix("https://openrouter.ai/api/v1", "model"),
                apply_online_suffix("https://openrouter.ai/api/v1", "model:online"),
                apply_online_suffix("https://relay.example/v1", "model"),
            ),
            (
                "model:online".to_owned(),
                "model:online".to_owned(),
                "model".to_owned(),
            )
        );
    }

    #[test]
    fn duplicate_model_candidates_are_kept_once_in_order() {
        let mut models = Vec::new();
        for model in ["primary", "fallback", "primary", "fallback"] {
            push_model(&mut models, model.into());
        }
        assert_eq!(models, ["primary", "fallback"]);
    }

    #[test]
    fn model_breaker_opens_at_two_failures_and_success_closes_it() {
        let breakers = ModelBreakers::default();
        for failure in 1..=BREAKER_FAILURE_THRESHOLD {
            assert_eq!(
                breakers.record_failure("https://relay.example/v1", "model"),
                failure == BREAKER_FAILURE_THRESHOLD
            );
        }
        assert!(breakers.is_open("https://relay.example/v1", "model"));
        breakers.record_success("https://relay.example/v1", "model");
        assert!(!breakers.is_open("https://relay.example/v1", "model"));
    }

    #[test]
    fn model_breaker_closes_after_the_six_hundred_second_cooldown() {
        let breakers = ModelBreakers::default();
        breakers.states.lock().expect("breaker state").insert(
            ("https://relay.example/v1".into(), "model".into()),
            BreakerState {
                consecutive_failures: BREAKER_FAILURE_THRESHOLD,
                opened_until: Some(Instant::now().checked_sub(Duration::from_secs(1)).unwrap()),
            },
        );
        assert!(!breakers.is_open("https://relay.example/v1", "model"));
    }

    #[test]
    fn model_breaker_opens_after_two_real_provider_failures_and_skips_the_next_request() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind provider fixture");
        let address = listener.local_addr().expect("fixture address");
        let server = thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().expect("accept provider request");
                let mut request = [0_u8; 4096];
                let _ = stream.read(&mut request).expect("read provider request");
                write!(
                    stream,
                    "HTTP/1.1 503 Error\r\nContent-Type: application/json\r\nContent-Length: 16\r\nConnection: close\r\n\r\n{{\"error\":\"down\"}}"
                )
                .expect("write provider response");
            }
        });
        let breakers = Arc::new(ModelBreakers::default());
        let provider = OpenAiCompatible::new(
            OpenAiCompatibleRuntimeConfig {
                url: format!("http://{address}/v1"),
                keys: vec!["key".into()],
                model: "model".into(),
                fallback_models: Vec::new(),
                stream: false,
            },
            reqwest::Client::new(),
            CredentialPool::new("openai_compatible", vec!["key".into()]),
            RetryPolicy::new(1, 1.0, Duration::ZERO),
            Deadline::new(Duration::from_secs(10)),
            breakers,
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        let request = || SearchRequest {
            query: "query".into(),
            model: None,
            allow_model_fallback: false,
            verbose: true,
        };

        runtime.block_on(async {
            assert_eq!(
                provider
                    .search(request())
                    .await
                    .expect_err("first failure")
                    .kind,
                AttemptErrorKind::Network
            );
            assert_eq!(
                provider
                    .search(request())
                    .await
                    .expect_err("second failure")
                    .attempts
                    .last()
                    .and_then(|attempt| attempt.breaker_event),
                Some("opened")
            );
            let skipped = provider.search(request()).await.expect_err("open breaker");
            assert_eq!(
                (
                    skipped.attempts.len(),
                    skipped.attempts[0].breaker_event,
                    skipped.attempts[0].http_status,
                ),
                (1, Some("open"), None)
            );
        });
        server.join().expect("provider fixture");
    }
}
