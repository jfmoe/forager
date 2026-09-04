use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::{Client, Response};
use serde::Serialize;
use serde_json::Value;
use tokio::time::Instant;

use crate::chain::{
    BudgetPolicy, ChainSettings, ChainStep, DiagnosticMerge, StepIdentity, StepSuccess,
    StepVerdict, TerminalPolicy, always_continue, run_chain,
};
use crate::config::OpenAiCompatibleRuntimeConfig;
use crate::credentials::CredentialPool;
use crate::net::{
    AttemptFailure, RetryPolicy, read_capped_sse, read_complete_protocol, send_provider_request,
};
use crate::providers::execution::{ExecutionSettings, execute_v2};
use crate::providers::shared::{
    normalize_main_search, redact_and_deduplicate_sources, redacted_urls_message,
};
use crate::providers::{MainSearchRequest, MainSearchRequestKind, ProviderError};
use crate::redact::Secret;
use crate::types::{
    AttemptDisposition, AttemptErrorKind, AttemptTarget, Deadline, SearchOutcome, Source,
};

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
        request: MainSearchRequest,
    ) -> Result<SearchOutcome, ProviderError> {
        self.execute(request, MainSearchRequestKind::Search).await
    }

    pub(crate) async fn probe(
        &self,
        request: MainSearchRequest,
    ) -> Result<SearchOutcome, ProviderError> {
        self.execute(request, MainSearchRequestKind::ModelProbe)
            .await
    }

    async fn execute(
        &self,
        request: MainSearchRequest,
        request_kind: MainSearchRequestKind,
    ) -> Result<SearchOutcome, ProviderError> {
        let query = request.query;
        let verbose = request.verbose;
        let models = self.model_candidates(request.model.as_deref(), request.allow_model_fallback);
        let steps = models
            .into_iter()
            .map(|model| ChainStep {
                gate_attempt: self.breakers.is_open(&self.config.url, &model).then(|| {
                    self.skipped_model_attempt(
                        &model,
                        "skipped because the model breaker is open",
                        Some("open"),
                    )
                }),
                context: model,
                configured: true,
            })
            .collect::<Vec<_>>();
        let endpoint_host = reqwest::Url::parse(&self.config.url)
            .ok()
            .and_then(|url| url.host_str().map(ToOwned::to_owned));
        let identity = |model: &String| StepIdentity {
            provider: "openai_compatible",
            model: Some(model.clone()),
            endpoint_host: endpoint_host.clone(),
        };
        let outcome = run_chain(
            steps,
            ChainSettings {
                seam: "main_search",
                budget_policy: BudgetPolicy::PrimaryFirst,
                fallback_off: false,
                diagnostic_merge: DiagnosticMerge::LatestWins,
                terminal: TerminalPolicy::Tail {
                    verbose,
                    default_kind: AttemptErrorKind::Runtime,
                    exhausted_message: "OpenAI-compatible model chain has no executable models",
                },
                identity: &identity,
                continue_on_failure: &always_continue,
            },
            self.deadline,
            |model, model_deadline| {
                let query = &query;
                async move {
                    match self
                        .execute_model(query, &model, verbose, model_deadline, request_kind)
                        .await
                    {
                        Ok(execution) => {
                            self.breakers.record_success(&self.config.url, &model);
                            StepVerdict::Accepted(StepSuccess {
                                value: (model, execution.value),
                                attempts: execution.attempts,
                                diagnostic: execution.diagnostic,
                            })
                        }
                        Err(mut error) => {
                            let opened = self.breakers.record_failure(&self.config.url, &model);
                            if opened && let Some(attempt) = error.attempts.last_mut() {
                                attempt.breaker_event = Some("opened");
                            }
                            StepVerdict::Failed(error)
                        }
                    }
                }
            },
        )
        .await?;
        let (model, (answer, sources)) = outcome.value;
        Ok(SearchOutcome {
            provider: "openai_compatible",
            query,
            model,
            answer,
            sources,
            extra_sources: Vec::new(),
            capabilities: Vec::new(),
            capability_gaps: Vec::new(),
            attempts: outcome.attempts,
            diagnostic: outcome.diagnostic,
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
        let steps = if self.config.stream {
            vec![
                TransportAttempt {
                    stream: true,
                    fallback_from: None,
                },
                TransportAttempt {
                    stream: false,
                    fallback_from: Some("sse"),
                },
            ]
        } else {
            vec![TransportAttempt {
                stream: false,
                fallback_from: None,
            }]
        }
        .into_iter()
        .map(|transport| ChainStep {
            context: transport,
            configured: true,
            gate_attempt: None,
        })
        .collect::<Vec<_>>();
        let transport_continuable = |error: &ProviderError| {
            matches!(
                error.kind,
                AttemptErrorKind::Timeout | AttemptErrorKind::Network | AttemptErrorKind::Runtime
            )
        };
        let identity = |_: &TransportAttempt| StepIdentity {
            provider: "openai_compatible",
            model: Some(model.to_owned()),
            endpoint_host: reqwest::Url::parse(&self.config.url)
                .ok()
                .and_then(|url| url.host_str().map(ToOwned::to_owned)),
        };
        let outcome = run_chain(
            steps,
            ChainSettings {
                seam: "main_search",
                budget_policy: BudgetPolicy::PrimaryFirst,
                fallback_off: false,
                diagnostic_merge: DiagnosticMerge::LatestWins,
                terminal: TerminalPolicy::LastError {
                    verbose,
                    fallback_kind: AttemptErrorKind::Timeout,
                    fallback_message: "openai_compatible request failed",
                },
                identity: &identity,
                continue_on_failure: &transport_continuable,
            },
            deadline,
            |transport, transport_deadline| async move {
                match self
                    .execute_transport(
                        query,
                        model,
                        verbose,
                        transport_deadline,
                        transport,
                        request_kind,
                    )
                    .await
                {
                    Ok(execution) => StepVerdict::Accepted(StepSuccess {
                        value: execution.value,
                        attempts: execution.attempts,
                        diagnostic: execution.diagnostic,
                    }),
                    Err(error) => StepVerdict::Failed(error),
                }
            },
        )
        .await?;
        Ok(crate::providers::execution::ExecutionOutcome {
            value: outcome.value,
            attempts: outcome.attempts,
            diagnostic: outcome.diagnostic,
        })
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
        execute_v2(
            &self.credentials,
            ExecutionSettings {
                provider: "openai_compatible",
                target: AttemptTarget::seam("main_search"),
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
            |credential, _| async move {
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
            target: AttemptTarget::seam("main_search"),
            disposition: AttemptDisposition::Skipped,
            error_kind: None,
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
        let request = self
            .client
            .post(endpoint)
            .bearer_auth(credential.expose())
            .header("accept", "application/json, text/event-stream")
            .json(&ChatRequest {
                model,
                messages,
                stream,
            });
        let response = send_provider_request(request, &self.credentials).await?;
        let status = response.status().as_u16();
        let (answer, sources) = if stream {
            self.stream_response(response).await?
        } else {
            self.http_response(response).await?
        };
        let value = if matches!(request_kind, MainSearchRequestKind::Search) {
            normalize_main_search(&answer, sources, &self.credentials)?
        } else {
            (
                answer,
                redact_and_deduplicate_sources(sources, &self.credentials),
            )
        };
        Ok((status, value))
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
        let body =
            read_complete_protocol(response, &self.credentials, openai_compatible_error).await?;
        if content_type.contains("text/event-stream") || body.text.trim_start().starts_with("data:")
        {
            return self.parse_sse_text(&body.text);
        }
        let value: Value = serde_json::from_str(&body.text).map_err(|error| AttemptFailure {
            kind: AttemptErrorKind::Runtime,
            status: Some(200),
            message: redacted_urls_message(
                &format!("OpenAI-compatible returned invalid JSON: {error}"),
                &self.credentials,
            ),
        })?;
        Self::normalize(&value, false)
    }

    async fn stream_response(
        &self,
        response: Response,
    ) -> Result<(String, Vec<Source>), AttemptFailure> {
        let (_, mut events) = read_capped_sse(
            response,
            &self.credentials,
            openai_compatible_error,
            "OpenAI-compatible returned invalid SSE",
        )
        .await?;
        let mut answer = String::new();
        let mut sources = Vec::new();
        let mut completed = false;
        while let Some(event) = events.next().await {
            let event = event?;
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
        let (delta, event_sources) = Self::normalize(&value, true)?;
        answer.push_str(&delta);
        merge_sources(sources, event_sources);
        Ok(completed)
    }

    fn normalize(value: &Value, streaming: bool) -> Result<(String, Vec<Source>), AttemptFailure> {
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
        Ok((content.to_owned(), sources))
    }
}

fn openai_compatible_error(body: &str, _status: u16) -> String {
    if body.trim().is_empty() {
        "OpenAI-compatible request failed".into()
    } else {
        body.to_owned()
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
                title: title.unwrap_or_default().trim().to_owned(),
                url: url.to_owned(),
                published_date: None,
                author: None,
                text: None,
                highlights: Vec::new(),
                id: None,
                image: None,
                favicon: None,
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
    if answer.trim().is_empty() {
        return Err(AttemptFailure {
            kind: AttemptErrorKind::Runtime,
            status: Some(200),
            message: if completed {
                "OpenAI-compatible stream contained no answer"
            } else {
                "OpenAI-compatible stream ended with an empty answer"
            }
            .into(),
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
    use std::time::Duration;

    use crate::config::OpenAiCompatibleRuntimeConfig;
    use crate::credentials::CredentialPool;
    use crate::net::RetryPolicy;
    use crate::providers::MainSearchRequest;
    use crate::types::{AttemptDisposition, AttemptErrorKind, Deadline};

    use super::{
        BREAKER_COOLDOWN, BREAKER_FAILURE_THRESHOLD, ModelBreakers, OpenAiCompatible,
        apply_online_suffix, finish, push_model,
    };

    fn provider() -> OpenAiCompatible {
        OpenAiCompatible::new(
            OpenAiCompatibleRuntimeConfig {
                url: "https://relay.example/v1".into(),
                keys: vec!["key".into()],
                model: "model".into(),
                fallback_models: Vec::new(),
                stream: true,
            },
            reqwest::Client::new(),
            CredentialPool::new("openai_compatible", vec!["key".into()]),
            RetryPolicy::new(1, 1.0, Duration::ZERO),
            Deadline::new(Duration::from_secs(10)),
            Arc::new(ModelBreakers::default()),
        )
    }

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
    fn sse_parser_accepts_controls_and_clean_eof_but_rejects_empty_or_malformed_data() {
        let provider = provider();
        let mut answer = String::new();
        let mut sources = Vec::new();

        assert_eq!(
            provider
                .consume_sse_data("", &mut answer, &mut sources)
                .ok(),
            Some(false)
        );
        assert_eq!(
            provider
                .consume_sse_data("[DONE]", &mut answer, &mut sources)
                .ok(),
            Some(true)
        );
        assert_eq!(
            provider
                .consume_sse_data(
                    r#"{"choices":[{"delta":{"content":"answer"}}]}"#,
                    &mut answer,
                    &mut sources,
                )
                .ok(),
            Some(false)
        );
        assert_eq!(
            finish(answer, sources, false).ok().map(|value| value.0),
            Some("answer".into())
        );
        assert_eq!(
            finish(String::new(), Vec::new(), false)
                .err()
                .map(|error| error.kind),
            Some(AttemptErrorKind::Runtime)
        );
        assert_eq!(
            provider
                .consume_sse_data("{not-json}", &mut String::new(), &mut Vec::new())
                .err()
                .map(|error| error.kind),
            Some(AttemptErrorKind::Runtime)
        );
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
    fn model_breaker_opens_after_real_failures_and_recovers_after_its_cooldown() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind provider fixture");
        let address = listener.local_addr().expect("fixture address");
        let server = thread::spawn(move || {
            for status in ["503 Error", "503 Error", "200 OK"] {
                let (mut stream, _) = listener.accept().expect("accept provider request");
                let mut request = [0_u8; 4096];
                let _ = stream.read(&mut request).expect("read provider request");
                let body = if status.starts_with("200") {
                    r#"{"choices":[{"message":{"content":"recovered"}}]}"#
                } else {
                    r#"{"error":"down"}"#
                };
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len(),
                )
                .expect("write provider response");
            }
        });
        let breakers = Arc::new(ModelBreakers::default());
        let config = OpenAiCompatibleRuntimeConfig {
            url: format!("http://{address}/v1"),
            keys: vec!["key".into()],
            model: "model".into(),
            fallback_models: Vec::new(),
            stream: false,
        };
        let provider = OpenAiCompatible::new(
            config.clone(),
            reqwest::Client::new(),
            CredentialPool::new("openai_compatible", vec!["key".into()]),
            RetryPolicy::new(1, 1.0, Duration::ZERO),
            Deadline::new(Duration::from_secs(10)),
            Arc::clone(&breakers),
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        let request = || MainSearchRequest {
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
                    skipped.kind,
                    skipped.attempts.len(),
                    skipped.attempts[0].disposition,
                    skipped.attempts[0].breaker_event,
                    skipped.attempts[0].http_status,
                ),
                (
                    AttemptErrorKind::Runtime,
                    1,
                    AttemptDisposition::Skipped,
                    Some("open"),
                    None,
                )
            );
            tokio::time::pause();
            tokio::time::advance(BREAKER_COOLDOWN).await;
            tokio::time::resume();
            let recovered = OpenAiCompatible::new(
                config,
                reqwest::Client::new(),
                CredentialPool::new("openai_compatible", vec!["key".into()]),
                RetryPolicy::new(1, 1.0, Duration::ZERO),
                Deadline::new(Duration::from_secs(10)),
                breakers,
            )
            .search(request())
            .await
            .expect("breaker permits a request after cooldown");
            assert_eq!(recovered.answer, "recovered");
        });
        server.join().expect("provider fixture");
    }
}
