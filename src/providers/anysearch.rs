use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Duration;

use reqwest::Client;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::config::AnysearchRuntimeConfig;
use crate::credentials::CredentialPool;
use crate::net::{McpClient, McpToolResult, RetryPolicy, truncate_message};
use crate::providers::ProviderError;
use crate::providers::execution::{AttemptFailure, ExecutionSettings, execute_v2};
use crate::providers::shared::redact_urls;
use crate::types::{
    AnysearchDomain, AnysearchDomainsOutcome, AnysearchOutcome, AnysearchResult,
    AnysearchSearchOutcome, AttemptErrorKind, Deadline, ProviderAttempt, SchemaValidation,
};

static MCP_HEADERS: LazyLock<HeaderMap> = LazyLock::new(|| {
    let mut headers = HeaderMap::new();
    headers.insert("x-anysearch-client", HeaderValue::from_static("mcp/1.0.0"));
    headers
});

#[derive(Clone, Debug)]
pub(crate) struct AnysearchDomainsRequest {
    pub(crate) domain: String,
    pub(crate) verbose: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct AnysearchSearchRequest {
    pub(crate) query: String,
    pub(crate) domain: Option<String>,
    pub(crate) sub_domain: Option<String>,
    pub(crate) sub_domain_params: Map<String, Value>,
    pub(crate) max_results: u16,
    pub(crate) verbose: bool,
}

pub(crate) struct Anysearch {
    config: AnysearchRuntimeConfig,
    client: Client,
    credentials: CredentialPool,
    retry_policy: RetryPolicy,
    deadline: Deadline,
}

impl Anysearch {
    pub(crate) fn new(
        config: AnysearchRuntimeConfig,
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

    pub(crate) async fn domains(
        &self,
        request: AnysearchDomainsRequest,
    ) -> Result<AnysearchOutcome, ProviderError> {
        let arguments = json!({"domain": request.domain});
        let execution = self
            .execute_tool("get_sub_domains", arguments, request.verbose)
            .await?;
        let mut results = decode_domains(&execution.result);
        for result in &mut results {
            if result.domain.is_empty() {
                result.domain.clone_from(&request.domain);
            }
        }
        Ok(AnysearchOutcome::Domains(AnysearchDomainsOutcome {
            provider: "anysearch",
            operation: "domain_discovery",
            experimental: true,
            domain: request.domain,
            total: results.len(),
            results,
            attempts: execution.attempts,
            diagnostic: execution.diagnostic,
        }))
    }

    pub(crate) async fn search(
        &self,
        request: AnysearchSearchRequest,
    ) -> Result<AnysearchOutcome, ProviderError> {
        let explicit = request.domain.is_some();
        let domain_status = request
            .domain
            .as_deref()
            .zip(request.sub_domain.as_deref())
            .map(|(domain, sub_domain)| domain_status(domain, sub_domain))
            .transpose()
            .map_err(|message| ProviderError {
                kind: AttemptErrorKind::Runtime,
                message,
                attempts: Vec::new(),
                verbose: request.verbose,
                diagnostic: None,
                redirected_library_id: None,
            })?;
        let mut arguments = json!({
            "query": request.query,
            "max_results": request.max_results,
        });
        if let Some(fields) = arguments.as_object_mut().filter(|_| explicit) {
            fields.insert(
                "domain".into(),
                Value::String(request.domain.clone().expect("validated domain pair")),
            );
            fields.insert(
                "sub_domain".into(),
                Value::String(
                    request
                        .sub_domain
                        .clone()
                        .expect("validated sub-domain pair"),
                ),
            );
            fields.insert(
                "sub_domain_params".into(),
                Value::Object(request.sub_domain_params.clone()),
            );
        }
        let execution = self
            .execute_tool("search", arguments, request.verbose)
            .await?;
        let results = decode_search(&execution.result);
        let mut sub_domain_param_keys = request
            .sub_domain_params
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        sub_domain_param_keys.sort();
        Ok(AnysearchOutcome::Search(AnysearchSearchOutcome {
            provider: "anysearch",
            operation: if explicit {
                "vertical_search"
            } else {
                "vertical_discovery"
            },
            experimental: true,
            query: request.query,
            max_results: request.max_results,
            domain: request.domain,
            sub_domain: request.sub_domain,
            domain_status,
            sub_domain_param_keys,
            schema_validation: if explicit {
                SchemaValidation {
                    status: "unavailable",
                    errors: Vec::new(),
                    message: Some(
                        "No Verified Domain Contract is available; parameters were passed to AnySearch unchanged.",
                    ),
                    schema_fingerprint: None,
                }
            } else {
                SchemaValidation {
                    status: "not_applicable",
                    errors: Vec::new(),
                    message: None,
                    schema_fingerprint: None,
                }
            },
            total: results.len(),
            results,
            attempts: execution.attempts,
            diagnostic: execution.diagnostic,
        }))
    }

    async fn execute_tool(
        &self,
        tool: &'static str,
        arguments: Value,
        verbose: bool,
    ) -> Result<AnysearchExecution, ProviderError> {
        let arguments_ref = &arguments;
        let execution = execute_v2(
            &self.credentials,
            ExecutionSettings {
                provider: "anysearch",
                seam: "vertical_search",
                retry_policy: self.retry_policy,
                deadline: self.deadline,
                attempt_timeout: Duration::from_secs(self.config.timeout_seconds),
                verbose,
                timeout_message: "AnySearch request timed out",
                model: None,
                transport: Some("mcp"),
                endpoint_host: None,
                breaker_event: None,
            },
            |credential, attempt_deadline| {
                let attempt_arguments = arguments_ref.clone();
                async move {
                    McpClient::new(
                        &self.client,
                        &self.config.url,
                        &MCP_HEADERS,
                        attempt_deadline,
                    )
                    .call_tool(&credential, tool, attempt_arguments)
                    .await
                    .map(|result| (200, result))
                    .map_err(|error| {
                        let mut message = redact_urls(&error.message, &self.credentials);
                        redact_argument_values(&mut message, arguments_ref);
                        AttemptFailure {
                            kind: error.kind,
                            status: error.status,
                            message: truncate_message(&message),
                            redirected_library_id: None,
                        }
                    })
                }
            },
        )
        .await?;
        Ok(AnysearchExecution {
            result: execution.value,
            attempts: if verbose {
                execution.attempts
            } else {
                Vec::new()
            },
            diagnostic: execution.diagnostic,
        })
    }
}

struct AnysearchExecution {
    result: McpToolResult,
    attempts: Vec<ProviderAttempt>,
    diagnostic: Option<String>,
}

#[derive(Deserialize)]
struct Manifest {
    verified_domains: Vec<ManifestEntry>,
    candidate_assessments: Vec<ManifestEntry>,
}

#[derive(Deserialize)]
struct ManifestEntry {
    domain: String,
    sub_domain: String,
    status: String,
}

type DomainStatusIndex = HashMap<String, HashMap<String, &'static str>>;

static DOMAIN_STATUSES: LazyLock<Result<DomainStatusIndex, String>> = LazyLock::new(|| {
    let manifest: Manifest = serde_json::from_str(include_str!(
        "../../assets/anysearch/verified-domain-manifest.json"
    ))
    .map_err(|error| format!("invalid versioned AnySearch manifest: {error}"))?;
    let mut statuses = DomainStatusIndex::new();
    for entry in manifest.candidate_assessments {
        statuses
            .entry(entry.domain)
            .or_default()
            .insert(entry.sub_domain, "discovered_unverified");
    }
    for entry in manifest
        .verified_domains
        .into_iter()
        .filter(|entry| entry.status == "verified")
    {
        statuses
            .entry(entry.domain)
            .or_default()
            .insert(entry.sub_domain, "verified");
    }
    Ok(statuses)
});

fn decode_domains(result: &McpToolResult) -> Vec<AnysearchDomain> {
    let domains = discovery_items(&result.structured_content)
        .into_iter()
        .filter_map(normalize_domain)
        .collect::<Vec<_>>();
    if domains.is_empty() {
        decode_markdown_domains(&result.text)
    } else {
        domains
    }
}

fn decode_search(result: &McpToolResult) -> Vec<AnysearchResult> {
    let mut results = Vec::new();
    let mut current: Option<AnysearchResult> = None;
    for line in result.text.lines() {
        if let Some(title) = numbered_heading(line) {
            if let Some(result) = current.take() {
                results.push(result);
            }
            current = Some(AnysearchResult {
                title: title.to_owned(),
                url: String::new(),
                description: String::new(),
                evidence_type: None,
            });
        } else if let Some(url) = line.strip_prefix("- **URL**: ") {
            if let Some(result) = &mut current {
                url.trim().clone_into(&mut result.url);
            }
        } else if let Some(result) = &mut current {
            let line = line.trim();
            if !line.is_empty() {
                if !result.description.is_empty() {
                    result.description.push(' ');
                }
                result.description.push_str(line);
                result.description = result.description.chars().take(300).collect();
            }
        }
    }
    if let Some(result) = current {
        results.push(result);
    }
    if results.is_empty()
        && (!result.text.is_empty()
            || result
                .structured_content
                .as_object()
                .is_some_and(|value| !value.is_empty()))
    {
        results.push(AnysearchResult {
            title: "vertical search structured result".into(),
            url: String::new(),
            description: result.text.chars().take(300).collect(),
            evidence_type: Some("structured"),
        });
    }
    results
}

fn numbered_heading(line: &str) -> Option<&str> {
    let line = line.strip_prefix("### ")?;
    let (_, title) = line.split_once(". ")?;
    (!title.trim().is_empty()).then_some(title.trim())
}

fn redact_argument_values(message: &mut String, value: &Value) {
    match value {
        Value::String(secret) if !secret.is_empty() => {
            *message = message.replace(secret, "[redacted]");
        }
        Value::Number(secret) => {
            *message = message.replace(&secret.to_string(), "[redacted]");
        }
        Value::Array(values) => {
            for value in values {
                redact_argument_values(message, value);
            }
        }
        Value::Object(fields) => {
            for value in fields.values() {
                redact_argument_values(message, value);
            }
        }
        _ => {}
    }
}

fn domain_status(domain: &str, sub_domain: &str) -> Result<String, String> {
    let statuses = DOMAIN_STATUSES.as_ref().map_err(Clone::clone)?;
    Ok(statuses
        .get(domain)
        .and_then(|statuses| statuses.get(sub_domain))
        .copied()
        .unwrap_or("unverified")
        .to_owned())
}

fn discovery_items(value: &Value) -> Vec<&Map<String, Value>> {
    let Some(object) = value.as_object() else {
        return Vec::new();
    };
    for key in ["sub_domains", "subDomains", "domains", "results", "data"] {
        match object.get(key) {
            Some(Value::Array(items)) => {
                return items.iter().filter_map(Value::as_object).collect();
            }
            Some(nested @ Value::Object(_)) => {
                let items = discovery_items(nested);
                if !items.is_empty() {
                    return items;
                }
            }
            _ => {}
        }
    }
    Vec::new()
}

fn normalize_domain(item: &Map<String, Value>) -> Option<AnysearchDomain> {
    let sub_domain = ["sub_domain", "subDomain", "name", "id"]
        .iter()
        .find_map(|name| item.get(*name).and_then(Value::as_str))?
        .to_owned();
    let domain = item
        .get("domain")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let parameter_schema = [
        "parameter_schema",
        "parameterSchema",
        "parameters",
        "inputSchema",
    ]
    .iter()
    .find_map(|name| item.get(*name).filter(|value| value.is_object()))
    .cloned()
    .unwrap_or_else(|| json!({}));
    Some(AnysearchDomain {
        domain,
        sub_domain,
        description: item
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .chars()
            .take(300)
            .collect(),
        parameter_schema,
    })
}

struct MarkdownDomain {
    domain: String,
    sub_domain: String,
    description: String,
    properties: Map<String, Value>,
    current_parameter: Option<String>,
}

fn decode_markdown_domains(text: &str) -> Vec<AnysearchDomain> {
    let mut domains = Vec::new();
    let mut current = None;
    for line in text.lines() {
        if let Some((domain, sub_domain)) = markdown_domain_heading(line) {
            if let Some(domain) = current.take() {
                domains.push(finish_markdown_domain(domain));
            }
            current = Some(MarkdownDomain {
                domain: domain.to_owned(),
                sub_domain: sub_domain.to_owned(),
                description: String::new(),
                properties: Map::new(),
                current_parameter: None,
            });
            continue;
        }
        let Some(domain) = &mut current else {
            continue;
        };
        let line = line.trim();
        if line.is_empty() || line == "**Parameters:**" {
            continue;
        }
        if let Some((name, description)) = markdown_parameter(line) {
            domain
                .properties
                .insert(name.to_owned(), json!({"description": description.trim()}));
            domain.current_parameter = Some(name.to_owned());
        } else if let Some(name) = &domain.current_parameter {
            let property = &mut domain.properties[name]["description"];
            let description = property.as_str().unwrap_or_default();
            *property = Value::String(format!("{description}\n{line}"));
        } else if domain.description.is_empty() {
            domain.description = line.chars().take(300).collect();
        }
    }
    if let Some(domain) = current {
        domains.push(finish_markdown_domain(domain));
    }
    domains
}

fn markdown_domain_heading(line: &str) -> Option<(&str, &str)> {
    let identifier = line.trim().strip_prefix("### ")?.trim();
    let (domain, sub_domain) = identifier.split_once('.')?;
    (!domain.is_empty() && !sub_domain.is_empty()).then_some((domain, sub_domain))
}

fn markdown_parameter(line: &str) -> Option<(&str, &str)> {
    let parameter = line.strip_prefix("- `")?;
    let (name, suffix) = parameter.split_once('`')?;
    let (_, description) = suffix.split_once(':')?;
    Some((name, description))
}

fn finish_markdown_domain(domain: MarkdownDomain) -> AnysearchDomain {
    AnysearchDomain {
        domain: domain.domain,
        sub_domain: domain.sub_domain,
        description: domain.description,
        parameter_schema: json!({
            "type": "object",
            "properties": domain.properties,
        }),
    }
}
