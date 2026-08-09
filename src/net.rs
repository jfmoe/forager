use std::time::Duration;

use eventsource_stream::{EventStreamError, Eventsource};
use futures_util::{Stream, StreamExt, future};
use reqwest::header::HeaderMap;
use reqwest::{Client, Response, StatusCode, redirect};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::redact::Secret;
use crate::types::{AttemptErrorKind, Deadline, MIN_USEFUL_SLICE_SECONDS};

const HTTP_READ_TIMEOUT: Duration = Duration::from_mins(1);
pub(crate) const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;
pub(crate) const CONTENT_TRUNCATED_DIAGNOSTIC: &str = "content truncated at 4 MiB";

pub(crate) struct CappedResponseBody {
    pub(crate) text: String,
    pub(crate) truncated: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResponseBodyPolicy {
    TruncatableContent,
    CompleteProtocol,
    Error,
}

impl ResponseBodyPolicy {
    pub(crate) fn for_status(status: StatusCode, success: Self) -> Self {
        if status.is_success() {
            success
        } else {
            Self::Error
        }
    }

    const fn limit(self) -> usize {
        match self {
            Self::TruncatableContent | Self::CompleteProtocol => MAX_RESPONSE_BYTES,
            Self::Error => MAX_ERROR_BODY_BYTES,
        }
    }
}

#[derive(Debug)]
pub(crate) enum CappedStreamError<E> {
    Transport(E),
    LimitExceeded,
}

impl<E: std::fmt::Display> std::fmt::Display for CappedStreamError<E> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(error) => error.fmt(formatter),
            Self::LimitExceeded => formatter.write_str("response exceeded 4 MiB"),
        }
    }
}

impl<E> CappedStreamError<E> {
    pub(crate) const fn attempt_error_kind(&self) -> AttemptErrorKind {
        match self {
            Self::Transport(_) => AttemptErrorKind::Network,
            Self::LimitExceeded => AttemptErrorKind::Runtime,
        }
    }
}

impl<E> std::error::Error for CappedStreamError<E> where E: std::error::Error + Send + Sync + 'static
{}

pub(crate) async fn read_response_body(
    response: Response,
    policy: ResponseBodyPolicy,
) -> Result<CappedResponseBody, CappedStreamError<reqwest::Error>> {
    let limit = policy.limit();
    let capacity = response
        .content_length()
        .and_then(|length| usize::try_from(length).ok())
        .map_or(0, |length| length.min(limit));
    let mut bytes = Vec::with_capacity(capacity);
    let mut stream = response.bytes_stream();
    let mut truncated = false;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(CappedStreamError::Transport)?;
        let remaining = limit.saturating_sub(bytes.len());
        if chunk.len() > remaining {
            if policy == ResponseBodyPolicy::CompleteProtocol {
                return Err(CappedStreamError::LimitExceeded);
            }
            bytes.extend_from_slice(&chunk[..remaining]);
            truncated = true;
            break;
        }
        bytes.extend_from_slice(&chunk);
    }
    if truncated {
        truncate_incomplete_utf8_suffix(&mut bytes);
    }
    Ok(CappedResponseBody {
        text: String::from_utf8_lossy(&bytes).into_owned(),
        truncated,
    })
}

pub(crate) fn json_string_prefix(body: &str, key: &str) -> Option<String> {
    let marker = format!("\"{key}\"");
    body.match_indices(&marker).find_map(|(index, _)| {
        let value = body
            .get(index + marker.len()..)?
            .trim_start()
            .strip_prefix(':')?
            .trim_start();
        decode_json_string_prefix(value)
    })
}

fn decode_json_string_prefix(value: &str) -> Option<String> {
    if !value.starts_with('"') {
        return None;
    }
    if let Ok(parsed) = String::deserialize(&mut serde_json::Deserializer::from_str(value)) {
        return Some(parsed);
    }
    let mut candidate = value.to_owned();
    for _ in 0..=6 {
        candidate.push('"');
        if let Ok(parsed) = String::deserialize(&mut serde_json::Deserializer::from_str(&candidate))
        {
            return Some(parsed);
        }
        candidate.pop();
        candidate.pop()?;
    }
    None
}

pub(crate) fn capped_stream<S, B, E>(
    stream: S,
    limit: usize,
) -> impl Stream<Item = Result<B, CappedStreamError<E>>>
where
    S: Stream<Item = Result<B, E>>,
    B: AsRef<[u8]>,
{
    stream.scan(Some(limit), |remaining, item| {
        let output = match (*remaining, item) {
            (None, _) => None,
            (Some(_), Err(error)) => {
                *remaining = None;
                Some(Err(CappedStreamError::Transport(error)))
            }
            (Some(available), Ok(chunk)) if chunk.as_ref().len() <= available => {
                *remaining = Some(available - chunk.as_ref().len());
                Some(Ok(chunk))
            }
            (Some(_), Ok(_)) => {
                *remaining = None;
                Some(Err(CappedStreamError::LimitExceeded))
            }
        };
        future::ready(output)
    })
}

fn truncate_incomplete_utf8_suffix(bytes: &mut Vec<u8>) {
    if let Err(error) = std::str::from_utf8(bytes)
        && error.error_len().is_none()
    {
        bytes.truncate(error.valid_up_to());
    }
}

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
        let retry_count = u32::try_from(retry_count).unwrap_or(u32::MAX);
        let seconds = self.multiplier * f64::from(retry_count);
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

    use reqwest::StatusCode;

    use super::{
        MAX_ERROR_BODY_BYTES, MAX_RESPONSE_BYTES, ResponseBodyPolicy, RetryPolicy, build_client,
        build_client_with_read_timeout, combine_diagnostics, duration_millis,
        error_kind_for_status,
    };
    use crate::types::AttemptErrorKind;

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
    fn response_body_policy_selects_error_heads_only_for_non_success_statuses() {
        assert_eq!(
            (
                ResponseBodyPolicy::for_status(
                    StatusCode::OK,
                    ResponseBodyPolicy::TruncatableContent,
                ),
                ResponseBodyPolicy::for_status(
                    StatusCode::OK,
                    ResponseBodyPolicy::CompleteProtocol,
                ),
                ResponseBodyPolicy::for_status(
                    StatusCode::BAD_REQUEST,
                    ResponseBodyPolicy::CompleteProtocol,
                ),
            ),
            (
                ResponseBodyPolicy::TruncatableContent,
                ResponseBodyPolicy::CompleteProtocol,
                ResponseBodyPolicy::Error,
            )
        );
    }

    #[test]
    fn response_body_policy_keeps_error_heads_smaller_than_success_bodies() {
        assert_eq!(
            (
                ResponseBodyPolicy::TruncatableContent.limit(),
                ResponseBodyPolicy::CompleteProtocol.limit(),
                ResponseBodyPolicy::Error.limit(),
            ),
            (MAX_RESPONSE_BYTES, MAX_RESPONSE_BYTES, MAX_ERROR_BODY_BYTES,)
        );
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

        runtime.block_on(async {
            let client = build_client(true).expect("build client");
            client
                .get(format!("http://{address}"))
                .send()
                .await
                .expect("send request");
        });

        let request = server.join().expect("join test server");
        assert!(
            request
                .lines()
                .any(|line| line == concat!("user-agent: forager/", env!("CARGO_PKG_VERSION"))),
            "request did not contain the forager user agent: {request}"
        );
    }

    #[test]
    fn build_client_returns_redirect_without_requesting_its_location() {
        let target_listener = TcpListener::bind("127.0.0.1:0").expect("bind target canary");
        target_listener
            .set_nonblocking(true)
            .expect("set target canary nonblocking");
        let target_address = target_listener.local_addr().expect("target canary address");
        let (stop_target, target_stopped) = std::sync::mpsc::channel();
        let target_server = thread::spawn(move || {
            loop {
                if target_stopped.try_recv().is_ok() {
                    return 0;
                }
                match target_listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_nonblocking(false)
                            .expect("set target stream blocking");
                        let mut request = [0_u8; 4096];
                        let read = stream.read(&mut request).expect("read target request");
                        assert!(read > 0, "target canary received an empty request");
                        stream
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .expect("write target response");
                        return 1;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("accept target canary request: {error}"),
                }
            }
        });

        let redirect_listener = TcpListener::bind("127.0.0.1:0").expect("bind redirect server");
        let redirect_address = redirect_listener
            .local_addr()
            .expect("redirect server address");
        let redirect_server = thread::spawn(move || {
            let (mut stream, _) = redirect_listener.accept().expect("accept redirect request");
            let mut request = [0_u8; 4096];
            let read = stream.read(&mut request).expect("read redirect request");
            assert!(read > 0, "redirect server received an empty request");
            write!(
                stream,
                "HTTP/1.1 302 Found\r\nLocation: http://{target_address}/canary\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .expect("write redirect response");
        });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build runtime");

        let status = runtime.block_on(async {
            build_client(true)
                .expect("build client")
                .get(format!("http://{redirect_address}"))
                .send()
                .await
                .expect("send request")
                .status()
        });
        let _ = stop_target.send(());
        redirect_server.join().expect("join redirect server");
        let target_requests = target_server.join().expect("join target canary");

        assert_eq!((status, target_requests), (StatusCode::FOUND, 0));
    }

    #[test]
    fn redirect_status_is_runtime_without_retry_or_rotation() {
        let kind = error_kind_for_status(StatusCode::FOUND, "");

        assert_eq!(
            (kind, kind.is_retryable(), kind.rotates_credential()),
            (AttemptErrorKind::Runtime, false, false)
        );
    }

    #[test]
    fn build_client_times_out_when_the_response_body_stalls() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        let (release, released) = std::sync::mpsc::channel();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut request = [0_u8; 4096];
            let read = stream.read(&mut request).expect("read request");
            assert!(read > 0, "fixture received an empty request");
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nx")
                .expect("write partial response");
            stream.flush().expect("flush partial response");
            released
                .recv_timeout(Duration::from_secs(1))
                .expect("wait for client timeout");
        });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build runtime");

        let result = runtime.block_on(async {
            let client = build_client_with_read_timeout(true, Duration::from_millis(200))
                .expect("build client");
            let response = client
                .get(format!("http://{address}"))
                .send()
                .await
                .expect("receive response headers");
            response.bytes().await
        });
        release.send(()).expect("release test server");
        server.join().expect("join test server");

        assert!(
            matches!(result, Err(ref error) if error.is_timeout()),
            "shared client did not enforce its read timeout: {result:?}"
        );
    }
}

pub(crate) fn build_client(ssl_verify: bool) -> Result<Client, reqwest::Error> {
    build_client_with_read_timeout(ssl_verify, HTTP_READ_TIMEOUT)
}

fn build_client_with_read_timeout(
    ssl_verify: bool,
    read_timeout: Duration,
) -> Result<Client, reqwest::Error> {
    Client::builder()
        .danger_accept_invalid_certs(!ssl_verify)
        .connect_timeout(Duration::from_secs(5))
        .read_timeout(read_timeout)
        .redirect(redirect::Policy::none())
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
    headers: &'a HeaderMap,
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
    pub(crate) fn new(
        client: &'a Client,
        url: &'a str,
        headers: &'a HeaderMap,
        deadline: Deadline,
    ) -> Self {
        Self {
            client,
            url,
            headers,
            deadline,
        }
    }

    pub(crate) async fn call_tool(
        &self,
        credential: &Secret,
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
                Err(error) if error.session_expired && handshake == 0 => {}
                Ok(result) => return Ok(result),
                Err(error) => return Err(error),
            }
        }
        Err(McpError::runtime("MCP session could not be renewed"))
    }

    async fn initialize(&self, credential: &Secret) -> Result<Option<String>, McpError> {
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

    async fn notify_initialized(&self, credential: &Secret, session: &str) -> Result<(), McpError> {
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
        credential: &Secret,
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
        let text = content_text(&result)?;
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
        credential: &Secret,
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
            .bearer_auth(credential.expose())
            .headers(self.headers.clone())
            .json(payload);
        if let Some(session) = session {
            request = request.header("mcp-session-id", session);
        }
        let response = tokio::time::timeout(remaining, request.send())
            .await
            .map_err(|_| McpError::new(AttemptErrorKind::Timeout, None, "MCP request timed out"))?
            .map_err(|error| {
                McpError::new(
                    AttemptErrorKind::Network,
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
        let body = tokio::time::timeout(
            remaining,
            read_response_body(response, ResponseBodyPolicy::Error),
        )
        .await
        .map_err(|_| {
            McpError::new(
                AttemptErrorKind::Timeout,
                Some(status.as_u16()),
                "MCP response body timed out",
            )
        })?
        .map_err(|error| {
            McpError::new(
                AttemptErrorKind::Network,
                Some(status.as_u16()),
                &format!("MCP response body failed: {error}"),
            )
        })?;
        let mut error = McpError::new(
            error_kind_for_status(status, &body.text),
            Some(status.as_u16()),
            &if body.text.trim().is_empty() {
                format!("MCP server returned HTTP {}", status.as_u16())
            } else {
                body.text
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
        let result = message
            .get("result")
            .cloned()
            .filter(Value::is_object)
            .ok_or_else(|| McpError::runtime("MCP JSON-RPC response omitted its result"))?;
        return Ok(result);
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
        let mut events = capped_stream(response.bytes_stream(), MAX_RESPONSE_BYTES).eventsource();
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
            let event = match event {
                Err(EventStreamError::Transport(CappedStreamError::LimitExceeded)) => {
                    return Err(McpError::runtime("response exceeded 4 MiB"));
                }
                Err(error) => {
                    return Err(McpError::new(
                        AttemptErrorKind::Network,
                        None,
                        &format!("MCP SSE response failed: {error}"),
                    ));
                }
                Ok(event) => event,
            };
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
    let body = tokio::time::timeout(
        remaining,
        read_response_body(response, ResponseBodyPolicy::CompleteProtocol),
    )
    .await
    .map_err(|_| {
        McpError::new(
            AttemptErrorKind::Timeout,
            None,
            "MCP JSON response timed out",
        )
    })?
    .map_err(|error| match error {
        CappedStreamError::Transport(error) => McpError::new(
            AttemptErrorKind::Network,
            error.status().map(|status| status.as_u16()),
            &format!("MCP response body failed: {error}"),
        ),
        CappedStreamError::LimitExceeded => McpError::runtime("response exceeded 4 MiB"),
    })?;
    let payload: Value = serde_json::from_str(&body.text)
        .map_err(|error| McpError::runtime(&format!("MCP returned invalid JSON: {error}")))?;
    let messages = match payload {
        Value::Array(messages) => messages,
        message => vec![message],
    };
    Ok(messages)
}

fn content_text(result: &Value) -> Result<String, McpError> {
    let Some(content) = result.get("content") else {
        return Ok(String::new());
    };
    let content = content
        .as_array()
        .ok_or_else(|| McpError::runtime("MCP result content must be an array"))?;
    Ok(content
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned())
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
