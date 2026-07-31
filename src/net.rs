use std::time::Duration;

use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use reqwest::{Client, Response, StatusCode};
use serde_json::{Value, json};

use crate::types::{AttemptErrorKind, Deadline, MIN_USEFUL_SLICE_SECONDS};

#[derive(Clone, Copy, Debug)]
pub(crate) struct RetryPolicy {
    max_attempts: usize,
    multiplier: f64,
    max_wait: Duration,
}

impl RetryPolicy {
    pub(crate) fn new(max_attempts: usize, multiplier: f64, max_wait: Duration) -> Self {
        Self {
            max_attempts,
            multiplier,
            max_wait,
        }
    }

    pub(crate) fn max_attempts(self) -> usize {
        self.max_attempts
    }

    pub(crate) fn wait(self, retry_count: usize) -> Duration {
        let seconds = self.multiplier * retry_count as f64;
        Duration::try_from_secs_f64(seconds)
            .unwrap_or(self.max_wait)
            .min(self.max_wait)
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;

    use super::{RetryPolicy, build_client, combine_diagnostics, duration_millis};

    #[test]
    fn combine_diagnostics_joins_present_values_in_order() {
        assert_eq!(
            combine_diagnostics(
                [Some("first".to_owned()), None, Some("second".to_owned())]
                    .into_iter()
                    .flatten(),
            ),
            Some("first\nsecond".to_owned())
        );
    }

    #[test]
    fn duration_millis_saturates_values_larger_than_u64() {
        assert_eq!(duration_millis(Duration::MAX), u64::MAX);
    }

    #[test]
    fn retry_wait_clamps_overflowing_seconds_to_max_wait() {
        let policy = RetryPolicy::new(3, f64::MAX, Duration::from_secs(10));

        assert_eq!(policy.wait(2), Duration::from_secs(10));
    }

    #[test]
    fn build_client_sends_forager_user_agent() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut request = [0_u8; 4096];
            let read = stream.read(&mut request).expect("read request");
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .expect("write response");
            String::from_utf8(request[..read].to_vec()).expect("UTF-8 request")
        });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build runtime");
        let client = build_client(true).expect("build client");

        runtime
            .block_on(client.get(format!("http://{address}")).send())
            .expect("send request");

        let request = server.join().expect("join test server");
        assert!(
            request
                .lines()
                .any(|line| line == concat!("user-agent: forager/", env!("CARGO_PKG_VERSION"))),
            "request did not contain the forager user agent: {request}"
        );
    }
}

pub(crate) fn build_client(ssl_verify: bool) -> Result<Client, reqwest::Error> {
    Client::builder()
        .danger_accept_invalid_certs(!ssl_verify)
        .connect_timeout(Duration::from_secs(5))
        .user_agent(concat!("forager/", env!("CARGO_PKG_VERSION")))
        .pool_idle_timeout(Duration::from_secs(90))
        .build()
}

pub(crate) fn error_kind_for_status(status: StatusCode, body: &str) -> AttemptErrorKind {
    match status.as_u16() {
        400 | 404 | 405 | 409 | 422 => AttemptErrorKind::Parameter,
        401 | 403 => AttemptErrorKind::Auth,
        402 => AttemptErrorKind::QuotaExhausted,
        429 if body
            .as_bytes()
            .windows(b"quota".len())
            .any(|window| window.eq_ignore_ascii_case(b"quota")) =>
        {
            AttemptErrorKind::QuotaExhausted
        }
        429 => AttemptErrorKind::RateLimited,
        408 | 504 => AttemptErrorKind::Timeout,
        500..=599 => AttemptErrorKind::Network,
        _ => AttemptErrorKind::Runtime,
    }
}

pub(crate) fn truncate_message(message: &str) -> String {
    message.chars().take(500).collect()
}

pub fn combine_diagnostics(diagnostics: impl IntoIterator<Item = String>) -> Option<String> {
    let diagnostics = diagnostics.into_iter().collect::<Vec<_>>();
    (!diagnostics.is_empty()).then(|| diagnostics.join("\n"))
}

pub(crate) fn slice_budget(remaining: Duration, remaining_slots: usize) -> Option<Duration> {
    if remaining_slots == 1 {
        return Some(remaining);
    }
    let slice = remaining / u32::try_from(remaining_slots).unwrap_or(u32::MAX);
    (slice >= Duration::from_secs(MIN_USEFUL_SLICE_SECONDS)).then_some(slice)
}

pub(crate) fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

pub(crate) struct McpClient<'a> {
    client: &'a Client,
    url: &'a str,
    deadline: Deadline,
}

pub(crate) struct McpToolResult {
    pub(crate) structured_content: Value,
    pub(crate) text: String,
}

#[derive(Debug)]
pub(crate) struct McpError {
    pub(crate) kind: AttemptErrorKind,
    pub(crate) status: Option<u16>,
    pub(crate) message: String,
    session_expired: bool,
}

impl<'a> McpClient<'a> {
    pub(crate) fn new(client: &'a Client, url: &'a str, deadline: Deadline) -> Self {
        Self {
            client,
            url,
            deadline,
        }
    }

    pub(crate) async fn call_tool(
        &self,
        credential: &str,
        tool: &str,
        arguments: Value,
    ) -> Result<McpToolResult, McpError> {
        for handshake in 0..2 {
            let session = self.initialize(credential).await?;
            if let Some(session) = session.as_deref() {
                match self.notify_initialized(credential, session).await {
                    Err(error) if error.session_expired && handshake == 0 => continue,
                    Err(error) => return Err(error),
                    Ok(()) => {}
                }
            }
            match self
                .tool_call(credential, session.as_deref(), tool, arguments.clone())
                .await
            {
                Err(error) if error.session_expired && handshake == 0 => continue,
                result => return result,
            }
        }
        Err(McpError::runtime("MCP session could not be renewed"))
    }

    async fn initialize(&self, credential: &str) -> Result<Option<String>, McpError> {
        let response = self
            .post(
                credential,
                None,
                &json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": MCP_PROTOCOL_VERSION,
                        "capabilities": {},
                        "clientInfo": {"name": "forager", "version": env!("CARGO_PKG_VERSION")}
                    }
                }),
            )
            .await?;
        let session = response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        rpc_result(response, 1, self.deadline).await?;
        Ok(session)
    }

    async fn notify_initialized(&self, credential: &str, session: &str) -> Result<(), McpError> {
        self.post(
            credential,
            Some(session),
            &json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized"
            }),
        )
        .await?;
        Ok(())
    }

    async fn tool_call(
        &self,
        credential: &str,
        session: Option<&str>,
        tool: &str,
        arguments: Value,
    ) -> Result<McpToolResult, McpError> {
        let response = self
            .post(
                credential,
                session,
                &json!({
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "tools/call",
                    "params": {"name": tool, "arguments": arguments}
                }),
            )
            .await?;
        let result = rpc_result(response, 2, self.deadline).await?;
        let text = content_text(&result);
        if result
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Err(McpError::semantic(&text, None));
        }
        Ok(McpToolResult {
            structured_content: result
                .get("structuredContent")
                .cloned()
                .filter(Value::is_object)
                .unwrap_or_else(|| json!({})),
            text,
        })
    }

    async fn post(
        &self,
        credential: &str,
        session: Option<&str>,
        payload: &Value,
    ) -> Result<Response, McpError> {
        let remaining = self.deadline.remaining().ok_or_else(|| {
            McpError::new(AttemptErrorKind::Timeout, None, "MCP deadline elapsed")
        })?;
        let mut request = self
            .client
            .post(self.url)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .bearer_auth(credential)
            .json(payload);
        if let Some(session) = session {
            request = request.header("mcp-session-id", session);
        }
        let response = tokio::time::timeout(remaining, request.send())
            .await
            .map_err(|_| McpError::new(AttemptErrorKind::Timeout, None, "MCP request timed out"))?
            .map_err(|error| {
                McpError::new(
                    if error.is_timeout() {
                        AttemptErrorKind::Timeout
                    } else {
                        AttemptErrorKind::Network
                    },
                    error.status().map(|status| status.as_u16()),
                    &format!("MCP network request failed: {error}"),
                )
            })?;
        if response.status().is_success() {
            return Ok(response);
        }
        let status = response.status();
        let remaining = self.deadline.remaining().ok_or_else(|| {
            McpError::new(AttemptErrorKind::Timeout, None, "MCP deadline elapsed")
        })?;
        let body = tokio::time::timeout(remaining, response.text())
            .await
            .map_err(|_| {
                McpError::new(
                    AttemptErrorKind::Timeout,
                    Some(status.as_u16()),
                    "MCP response body timed out",
                )
            })?
            .unwrap_or_default();
        let mut error = McpError::new(
            error_kind_for_status(status, &body),
            Some(status.as_u16()),
            &if body.trim().is_empty() {
                format!("MCP server returned HTTP {}", status.as_u16())
            } else {
                body
            },
        );
        error.session_expired = session.is_some() && session_expired(status, &error.message);
        Err(error)
    }
}

async fn rpc_result(
    response: Response,
    request_id: u64,
    deadline: Deadline,
) -> Result<Value, McpError> {
    let messages = response_messages(response, deadline).await?;
    for message in messages {
        if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Err(McpError::runtime(
                "MCP returned an invalid JSON-RPC message",
            ));
        }
        if message.get("id").and_then(Value::as_u64) != Some(request_id) {
            continue;
        }
        if let Some(error) = message.get("error") {
            let text = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("MCP returned a JSON-RPC error");
            let mut classified = McpError::semantic(text, None);
            if classified.kind == AttemptErrorKind::Runtime
                && error.get("code").and_then(Value::as_i64) == Some(-32602)
            {
                classified.kind = AttemptErrorKind::Parameter;
            }
            classified.session_expired =
                session_expired(StatusCode::BAD_REQUEST, &classified.message);
            return Err(classified);
        }
        return message
            .get("result")
            .cloned()
            .filter(Value::is_object)
            .ok_or_else(|| McpError::runtime("MCP JSON-RPC response omitted its result"));
    }
    Err(McpError::runtime(
        "MCP response did not match the JSON-RPC request",
    ))
}

async fn response_messages(response: Response, deadline: Deadline) -> Result<Vec<Value>, McpError> {
    let is_sse = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("text/event-stream"));
    if is_sse {
        let mut events = response.bytes_stream().eventsource();
        let mut messages = Vec::new();
        loop {
            let remaining = deadline.remaining().ok_or_else(|| {
                McpError::new(AttemptErrorKind::Timeout, None, "MCP deadline elapsed")
            })?;
            let event = tokio::time::timeout(remaining, events.next())
                .await
                .map_err(|_| {
                    McpError::new(
                        AttemptErrorKind::Timeout,
                        None,
                        "MCP SSE response timed out",
                    )
                })?;
            let Some(event) = event else {
                break;
            };
            let event = event.map_err(|error| {
                McpError::runtime(&format!("MCP returned invalid SSE: {error}"))
            })?;
            let message = serde_json::from_str(&event.data).map_err(|error| {
                McpError::runtime(&format!("MCP returned invalid SSE JSON: {error}"))
            })?;
            messages.push(message);
        }
        if messages.is_empty() {
            return Err(McpError::runtime("MCP returned an empty SSE response"));
        }
        return Ok(messages);
    }

    let remaining = deadline
        .remaining()
        .ok_or_else(|| McpError::new(AttemptErrorKind::Timeout, None, "MCP deadline elapsed"))?;
    let payload: Value = tokio::time::timeout(remaining, response.json())
        .await
        .map_err(|_| {
            McpError::new(
                AttemptErrorKind::Timeout,
                None,
                "MCP JSON response timed out",
            )
        })?
        .map_err(|error| McpError::runtime(&format!("MCP returned invalid JSON: {error}")))?;
    Ok(match payload {
        Value::Array(messages) => messages,
        message => vec![message],
    })
}

fn content_text(result: &Value) -> String {
    result
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

fn session_expired(status: StatusCode, message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    matches!(
        status,
        StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND | StatusCode::CONFLICT
    ) && lower.contains("session")
        && ["expired", "not found", "invalid"]
            .iter()
            .any(|marker| lower.contains(marker))
}

impl McpError {
    fn new(kind: AttemptErrorKind, status: Option<u16>, message: &str) -> Self {
        Self {
            kind,
            status,
            message: truncate_message(message),
            session_expired: false,
        }
    }

    fn runtime(message: &str) -> Self {
        Self::new(AttemptErrorKind::Runtime, None, message)
    }

    fn semantic(message: &str, status: Option<u16>) -> Self {
        let lower = message.to_ascii_lowercase();
        let kind = if lower.contains("quota") || lower.contains("credits exhausted") {
            AttemptErrorKind::QuotaExhausted
        } else if lower.contains("rate limit") || lower.contains("too many requests") {
            AttemptErrorKind::RateLimited
        } else if lower.contains("unauthorized") || lower.contains("authentication") {
            AttemptErrorKind::Auth
        } else if lower.contains("invalid parameter") || lower.contains("invalid argument") {
            AttemptErrorKind::Parameter
        } else {
            AttemptErrorKind::Runtime
        };
        Self::new(kind, status, message)
    }
}
