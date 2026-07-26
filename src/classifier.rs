use std::collections::HashSet;
use std::time::{Duration, Instant};

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::config::{self, ClassifierRuntimeConfig};
use crate::credentials::CredentialPool;
use crate::net::{RetryPolicy, error_kind_for_status, truncate_message};
use crate::providers::execution::{AttemptFailure, ExecutionSettings, execute};
use crate::types::{
    AttemptErrorKind, Capability, CapabilitySet, Deadline, MIN_USEFUL_SLICE_SECONDS,
    ProviderAttempt,
};

const VOCABULARY: &str = include_str!("../assets/capability-vocabulary.json");

#[derive(Debug)]
pub(crate) struct ClassifierSuccess {
    pub(crate) capabilities: CapabilitySet,
    pub(crate) attempts: Vec<ProviderAttempt>,
    pub(crate) duration: Duration,
}

#[derive(Debug)]
pub(crate) struct ClassifierFailure {
    pub(crate) attempts: Vec<ProviderAttempt>,
    pub(crate) duration: Duration,
    pub(crate) message: String,
}

pub(crate) struct Classifier {
    config: ClassifierRuntimeConfig,
    client: Client,
    credentials: CredentialPool,
    retry_policy: RetryPolicy,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CapabilityDecision {
    required_capabilities: Vec<Capability>,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Deserialize)]
struct Message {
    content: String,
}

impl Classifier {
    pub(crate) fn new(
        mut config: ClassifierRuntimeConfig,
        client: Client,
        retry_policy: RetryPolicy,
    ) -> Self {
        let credentials = CredentialPool::new("classifier", std::mem::take(&mut config.keys));
        Self {
            config,
            client,
            credentials,
            retry_policy,
        }
    }

    pub(crate) async fn classify(
        &self,
        query: &str,
        command_deadline: Deadline,
    ) -> Result<ClassifierSuccess, ClassifierFailure> {
        let started = Instant::now();
        let Some(command_remaining) = command_deadline.remaining() else {
            return Err(ClassifierFailure {
                attempts: Vec::new(),
                duration: started.elapsed(),
                message: "classifier skipped because the command deadline elapsed".into(),
            });
        };
        let stage_deadline =
            Deadline::new(command_remaining.min(Duration::from_secs(self.config.timeout_seconds)));
        let models = self.model_candidates();
        let mut attempts = Vec::new();

        for (index, model) in models.iter().enumerate() {
            let Some(remaining) = stage_deadline.remaining() else {
                break;
            };
            let slots = models.len() - index;
            let Some(model_budget) = model_budget(remaining, slots) else {
                attempts.push(self.skipped_attempt(
                    model,
                    "skipped to preserve classifier model fallback deadline budget",
                ));
                continue;
            };
            match self
                .execute_model(query, model, Deadline::new(model_budget))
                .await
            {
                Ok((decision, mut model_attempts)) => {
                    attempts.append(&mut model_attempts);
                    return Ok(ClassifierSuccess {
                        capabilities: CapabilitySet::from_capabilities(
                            decision.required_capabilities,
                        ),
                        attempts,
                        duration: started.elapsed(),
                    });
                }
                Err(mut model_attempts) => attempts.append(&mut model_attempts),
            }
        }

        let message = attempts.last().map_or_else(
            || "classifier model chain exhausted".into(),
            |attempt| attempt.message.clone(),
        );
        Err(ClassifierFailure {
            attempts,
            duration: started.elapsed(),
            message,
        })
    }

    async fn execute_model(
        &self,
        query: &str,
        model: &str,
        deadline: Deadline,
    ) -> Result<(CapabilityDecision, Vec<ProviderAttempt>), Vec<ProviderAttempt>> {
        let endpoint_host = reqwest::Url::parse(&self.config.url)
            .ok()
            .and_then(|url| url.host_str().map(ToOwned::to_owned));
        execute(
            &self.credentials,
            ExecutionSettings {
                provider: "classifier",
                seam: "classifier",
                retry_policy: self.retry_policy,
                deadline,
                attempt_timeout: deadline.remaining().unwrap_or_default(),
                verbose: false,
                timeout_message: "classifier request timed out",
                model: Some(model.into()),
                transport: Some("http"),
                endpoint_host,
                breaker_event: None,
            },
            |credential| async move { self.send_once(query, model, &credential).await },
        )
        .await
        .map(|outcome| (outcome.value, outcome.attempts))
        .map_err(|error| error.attempts)
    }

    async fn send_once(
        &self,
        query: &str,
        model: &str,
        credential: &str,
    ) -> Result<(u16, CapabilityDecision), AttemptFailure> {
        let endpoint = format!("{}/chat/completions", self.config.url.trim_end_matches('/'));
        let response = self
            .client
            .post(endpoint)
            .bearer_auth(credential)
            .header("accept", "application/json")
            .json(&serde_json::json!({
                "model": model,
                "messages": [
                    {
                        "role": "system",
                        "content": classifier_instruction()
                    },
                    {
                        "role": "user",
                        "content": query
                    }
                ],
                "response_format": {
                    "type": "json_schema",
                    "json_schema": {
                        "name": "classifier_capability_decision",
                        "strict": true,
                        "schema": {
                            "type": "object",
                            "additionalProperties": false,
                            "required": ["required_capabilities"],
                            "properties": {
                                "required_capabilities": {
                                    "type": "array",
                                    "items": {
                                        "type": "string",
                                        "enum": [
                                            "docs_search",
                                            "web_search",
                                            "web_fetch",
                                            "vertical_search"
                                        ]
                                    },
                                    "uniqueItems": true
                                }
                            }
                        }
                    }
                }
            }))
            .send()
            .await
            .map_err(|error| AttemptFailure {
                kind: if error.is_timeout() {
                    AttemptErrorKind::Timeout
                } else {
                    AttemptErrorKind::Network
                },
                status: error.status().map(|status| status.as_u16()),
                message: self.redact(&error.to_string()),
            })?;
        let status = response.status();
        let body = response.text().await.map_err(|error| AttemptFailure {
            kind: AttemptErrorKind::Network,
            status: Some(status.as_u16()),
            message: self.redact(&error.to_string()),
        })?;
        if !status.is_success() {
            return Err(AttemptFailure {
                kind: error_kind_for_status(status, &body),
                status: Some(status.as_u16()),
                message: self.redact(if body.trim().is_empty() {
                    "classifier request failed"
                } else {
                    &body
                }),
            });
        }
        let response: ChatResponse = serde_json::from_str(&body).map_err(|_| AttemptFailure {
            kind: AttemptErrorKind::Runtime,
            status: Some(status.as_u16()),
            message: "classifier returned invalid response JSON".into(),
        })?;
        let content = response
            .choices
            .first()
            .map(|choice| choice.message.content.trim())
            .filter(|content| !content.is_empty())
            .ok_or_else(|| AttemptFailure {
                kind: AttemptErrorKind::Runtime,
                status: Some(status.as_u16()),
                message: "classifier response omitted JSON content".into(),
            })?;
        let decision: CapabilityDecision =
            serde_json::from_str(content).map_err(|_| AttemptFailure {
                kind: AttemptErrorKind::Runtime,
                status: Some(status.as_u16()),
                message: "classifier returned invalid capability schema".into(),
            })?;
        let unique = decision
            .required_capabilities
            .iter()
            .copied()
            .collect::<HashSet<_>>();
        if unique.len() != decision.required_capabilities.len() {
            return Err(AttemptFailure {
                kind: AttemptErrorKind::Runtime,
                status: Some(status.as_u16()),
                message: "classifier returned duplicate capabilities".into(),
            });
        }
        Ok((status.as_u16(), decision))
    }

    fn model_candidates(&self) -> Vec<String> {
        let mut models = Vec::new();
        for model in std::iter::once(&self.config.model).chain(&self.config.fallback_models) {
            if !model.is_empty() && !models.contains(model) {
                models.push(model.clone());
            }
        }
        models
    }

    fn skipped_attempt(&self, model: &str, message: &str) -> ProviderAttempt {
        ProviderAttempt {
            provider: "classifier",
            seam: "classifier",
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
            breaker_event: None,
        }
    }

    fn redact(&self, message: &str) -> String {
        truncate_message(&self.credentials.redact(&config::redact_urls(message)))
    }
}

fn classifier_instruction() -> String {
    format!(
        "Select the complete capability set required by the user request. Multiple capabilities or an empty set are valid. Never select providers. Return JSON only, matching the required schema. Capability vocabulary, authoritative order, selection semantics, and examples:\n{VOCABULARY}"
    )
}

fn model_budget(remaining: Duration, remaining_slots: usize) -> Option<Duration> {
    if remaining_slots == 1 {
        return Some(remaining);
    }
    let slice = remaining / u32::try_from(remaining_slots).unwrap_or(u32::MAX);
    (slice >= Duration::from_secs(MIN_USEFUL_SLICE_SECONDS)).then_some(slice)
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::VOCABULARY;
    use crate::types::Capability;

    #[test]
    fn vocabulary_asset_matches_compiled_capability_identity_and_order() {
        let vocabulary: Value = serde_json::from_str(VOCABULARY).expect("valid vocabulary asset");
        let identities = vocabulary["capabilities"]
            .as_array()
            .expect("capability entries")
            .iter()
            .map(|capability| capability["id"].as_str().expect("capability id"))
            .collect::<Vec<_>>();

        assert_eq!(
            identities,
            [
                Capability::DocsSearch.as_str(),
                Capability::WebSearch.as_str(),
                Capability::WebFetch.as_str(),
                Capability::VerticalSearch.as_str(),
            ]
        );
        assert!(
            vocabulary["capabilities"]
                .as_array()
                .expect("capability entries")
                .iter()
                .all(|capability| capability["examples"]
                    .as_array()
                    .is_some_and(|examples| !examples.is_empty()))
        );
    }
}
