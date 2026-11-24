use std::pin::Pin;
use std::time::Duration;

use eventsource_stream::{EventStreamError, Eventsource};
use futures_util::{Stream, StreamExt, future};
use reqwest::{Client, RequestBuilder, Response, StatusCode, redirect};
use serde::Deserialize;

use crate::credentials::CredentialPool;
use crate::redact::redact_urls;
use crate::types::{AttemptErrorKind, MIN_USEFUL_SLICE_SECONDS};

#[cfg(test)]
#[path = "loopback_guard.rs"]
mod loopback_guard;

const HTTP_READ_TIMEOUT: Duration = Duration::from_mins(1);
pub(crate) const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;
pub(crate) const CONTENT_TRUNCATED_DIAGNOSTIC: &str = "content truncated at 4 MiB";

const RESPONSE_LIMIT_MESSAGE: &str = "response exceeded 4 MiB";

#[derive(Debug)]
pub(crate) struct AttemptFailure {
    pub(crate) kind: AttemptErrorKind,
    pub(crate) status: Option<u16>,
    pub(crate) message: String,
}

#[derive(Debug)]
pub(crate) struct ProviderResponseBody {
    pub(crate) status: u16,
    pub(crate) text: String,
    pub(crate) truncated: bool,
}

pub(crate) type ProviderEventStream =
    Pin<Box<dyn Stream<Item = Result<eventsource_stream::Event, AttemptFailure>> + Send>>;

pub(crate) async fn send_provider_request(
    request: RequestBuilder,
    credentials: &CredentialPool,
) -> Result<Response, AttemptFailure> {
    request.send().await.map_err(|error| AttemptFailure {
        kind: AttemptErrorKind::Network,
        status: error.status().map(|status| status.as_u16()),
        message: redact_provider_message(&error.to_string(), credentials),
    })
}

pub(crate) async fn read_complete_protocol(
    response: Response,
    credentials: &CredentialPool,
    error_message: fn(&str, u16) -> String,
) -> Result<ProviderResponseBody, AttemptFailure> {
    read_complete_protocol_with_reasons(response, credentials, error_message, |_| None).await
}

/// Reads a complete protocol body; for an error status, a reason `reason_kind` recognizes in the
/// provider's error body decides the error kind in place of the shared status mapping.
pub(crate) async fn read_complete_protocol_with_reasons(
    response: Response,
    credentials: &CredentialPool,
    error_message: fn(&str, u16) -> String,
    reason_kind: fn(&str) -> Option<AttemptErrorKind>,
) -> Result<ProviderResponseBody, AttemptFailure> {
    let response = ensure_success(response, credentials, error_message, reason_kind).await?;
    read_success_body(response, ResponseBodyPolicy::CompleteProtocol, credentials).await
}

pub(crate) async fn read_truncatable_content(
    response: Response,
    credentials: &CredentialPool,
    error_message: fn(&str, u16) -> String,
) -> Result<ProviderResponseBody, AttemptFailure> {
    let response = ensure_success(response, credentials, error_message, |_| None).await?;
    read_success_body(
        response,
        ResponseBodyPolicy::TruncatableContent,
        credentials,
    )
    .await
}

pub(crate) async fn read_capped_sse(
    response: Response,
    credentials: &CredentialPool,
    error_message: impl Fn(&str, u16) -> String,
    stream_error_prefix: String,
) -> Result<(u16, ProviderEventStream), AttemptFailure> {
    let response = ensure_success(response, credentials, error_message, |_| None).await?;
    let status = response.status().as_u16();
    let credentials = credentials.clone();
    let events = capped_stream(response.bytes_stream(), MAX_RESPONSE_BYTES)
        .eventsource()
        .map(move |event| {
            event.map_err(|error| match error {
                EventStreamError::Transport(CappedStreamError::LimitExceeded) => AttemptFailure {
                    kind: AttemptErrorKind::Runtime,
                    status: Some(status),
                    message: RESPONSE_LIMIT_MESSAGE.into(),
                },
                error => AttemptFailure {
                    kind: AttemptErrorKind::Network,
                    status: Some(status),
                    message: redact_provider_message(
                        &format!("{stream_error_prefix}: {error}"),
                        &credentials,
                    ),
                },
            })
        });
    Ok((status, Box::pin(events)))
}

async fn read_success_body(
    response: Response,
    success_policy: ResponseBodyPolicy,
    credentials: &CredentialPool,
) -> Result<ProviderResponseBody, AttemptFailure> {
    let status = response.status().as_u16();
    let body = read_response_body(response, success_policy)
        .await
        .map_err(|error| response_body_failure(&error, status, credentials))?;
    Ok(ProviderResponseBody {
        status,
        text: body.text,
        truncated: body.truncated,
    })
}

async fn ensure_success(
    response: Response,
    credentials: &CredentialPool,
    error_message: impl Fn(&str, u16) -> String,
    reason_kind: fn(&str) -> Option<AttemptErrorKind>,
) -> Result<Response, AttemptFailure> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = read_response_body(response, ResponseBodyPolicy::Error)
        .await
        .map_err(|error| response_body_failure(&error, status.as_u16(), credentials))?;
    Err(AttemptFailure {
        kind: reason_kind(&body.text).unwrap_or_else(|| error_kind_for_status(status, &body.text)),
        status: Some(status.as_u16()),
        message: redact_provider_message(&error_message(&body.text, status.as_u16()), credentials),
    })
}

fn response_body_failure(
    error: &CappedStreamError<reqwest::Error>,
    status: u16,
    credentials: &CredentialPool,
) -> AttemptFailure {
    AttemptFailure {
        kind: error.attempt_error_kind(),
        status: Some(status),
        message: redact_provider_message(&error.to_string(), credentials),
    }
}

fn redact_provider_message(message: &str, credentials: &CredentialPool) -> String {
    truncate_message(&credentials.redact(&redact_urls(message)))
}

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
            Self::LimitExceeded => formatter.write_str(RESPONSE_LIMIT_MESSAGE),
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

pub(crate) fn build_client(ssl_verify: bool) -> Result<Client, reqwest::Error> {
    build_client_with_read_timeout(ssl_verify, HTTP_READ_TIMEOUT)
}

fn build_client_with_read_timeout(
    ssl_verify: bool,
    read_timeout: Duration,
) -> Result<Client, reqwest::Error> {
    let builder = Client::builder()
        .danger_accept_invalid_certs(!ssl_verify)
        .connect_timeout(Duration::from_secs(5))
        .read_timeout(read_timeout)
        .redirect(redirect::Policy::none())
        .user_agent(concat!("forager/", env!("CARGO_PKG_VERSION")))
        .pool_idle_timeout(Duration::from_secs(90));
    #[cfg(test)]
    let builder = loopback_guard::apply(builder);
    builder.build()
}

pub(crate) fn error_kind_for_status(status: StatusCode, body: &str) -> AttemptErrorKind {
    match status.as_u16() {
        400 | 404 | 405 | 409 | 422 => AttemptErrorKind::Parameter,
        // Firecrawl refuses unsupported target sites with 403; its invalid keys get 401.
        403 if mentions(body, "support this site") => AttemptErrorKind::Parameter,
        // Jina refuses anonymous access to a domain it is shielding from abuse; a key may lift it.
        403 if mentions(body, "AbuseAlleviationError") => AttemptErrorKind::Parameter,
        401 | 403 => AttemptErrorKind::Auth,
        402 => AttemptErrorKind::QuotaExhausted,
        // SerpApi reports a used-up monthly plan as "run out of searches".
        429 if mentions(body, "quota") || mentions(body, "run out of searches") => {
            AttemptErrorKind::QuotaExhausted
        }
        429 => AttemptErrorKind::RateLimited,
        408 | 504 => AttemptErrorKind::Timeout,
        500..=599 => AttemptErrorKind::Network,
        _ => AttemptErrorKind::Runtime,
    }
}

fn mentions(body: &str, phrase: &str) -> bool {
    body.as_bytes()
        .windows(phrase.len())
        .any(|window| window.eq_ignore_ascii_case(phrase.as_bytes()))
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

#[cfg(test)]
mod tests {
    use std::io::{self, Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use reqwest::{Body, Response, StatusCode};

    use super::{
        MAX_ERROR_BODY_BYTES, MAX_RESPONSE_BYTES, RESPONSE_LIMIT_MESSAGE, ResponseBodyPolicy,
        RetryPolicy, build_client, build_client_with_read_timeout, combine_diagnostics,
        duration_millis, error_kind_for_status, read_complete_protocol, read_truncatable_content,
        slice_budget,
    };
    use crate::credentials::CredentialPool;
    use crate::fixture_server::block_on;
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
    fn complete_protocol_accepts_body_exactly_at_cap() {
        let body = vec![b'x'; MAX_RESPONSE_BYTES];
        let (url, server) = serve_response("200 OK", body);
        let result = block_on(async {
            let response = build_client(true)
                .expect("build client")
                .get(url)
                .send()
                .await
                .expect("send request");
            read_complete_protocol(response, &credentials(), raw_error_body).await
        });
        server.join().expect("join response server");

        assert_eq!(
            result.ok().map(|body| body.text.len()),
            Some(MAX_RESPONSE_BYTES)
        );
    }

    #[test]
    fn complete_protocol_rejects_body_one_byte_over_cap_as_runtime() {
        let body = vec![b'x'; MAX_RESPONSE_BYTES + 1];
        let (url, server) = serve_response("200 OK", body);
        let failure = block_on(async {
            let response = build_client(true)
                .expect("build client")
                .get(url)
                .send()
                .await
                .expect("send request");
            read_complete_protocol(response, &credentials(), raw_error_body)
                .await
                .expect_err("over-cap protocol must fail")
        });
        server.join().expect("join response server");

        assert_eq!(
            (failure.kind, failure.message.as_str()),
            (AttemptErrorKind::Runtime, RESPONSE_LIMIT_MESSAGE)
        );
    }

    #[test]
    fn truncatable_content_removes_incomplete_utf8_tail() {
        let mut body = vec![b'x'; MAX_RESPONSE_BYTES - 1];
        body.extend_from_slice("界".as_bytes());
        let (url, server) = serve_response("200 OK", body);
        let result = block_on(async {
            let response = build_client(true)
                .expect("build client")
                .get(url)
                .send()
                .await
                .expect("send request");
            read_truncatable_content(response, &credentials(), raw_error_body)
                .await
                .expect("truncatable body")
        });
        server.join().expect("join response server");

        assert_eq!(
            (result.text.len(), result.truncated),
            (MAX_RESPONSE_BYTES - 1, true)
        );
    }

    #[test]
    fn non_success_response_reads_only_the_bounded_error_head() {
        let body = vec![b'x'; MAX_ERROR_BODY_BYTES + 1];
        let (url, server) = serve_response("429 Too Many Requests", body);
        let failure = block_on(async {
            let response = build_client(true)
                .expect("build client")
                .get(url)
                .send()
                .await
                .expect("send request");
            read_complete_protocol(response, &credentials(), body_length_message)
                .await
                .expect_err("non-success response must fail")
        });
        server.join().expect("join response server");

        assert_eq!(
            (failure.kind, failure.message),
            (
                AttemptErrorKind::RateLimited,
                MAX_ERROR_BODY_BYTES.to_string()
            )
        );
    }

    #[test]
    fn complete_protocol_maps_a_stalled_body_to_network() {
        let stalled_body = futures_util::stream::iter([
            Ok(&b"x"[..]),
            Err(io::Error::from(io::ErrorKind::TimedOut)),
        ]);
        let response = Response::from(http::Response::new(Body::wrap_stream(stalled_body)));

        let failure = block_on(read_complete_protocol(
            response,
            &credentials(),
            raw_error_body,
        ))
        .expect_err("stalled body must fail");

        assert_eq!(failure.kind, AttemptErrorKind::Network);
    }

    fn credentials() -> CredentialPool {
        CredentialPool::new("test", vec!["secret".into()])
    }

    fn raw_error_body(body: &str, _status: u16) -> String {
        body.to_owned()
    }

    fn body_length_message(body: &str, _status: u16) -> String {
        body.len().to_string()
    }

    fn serve_response(status: &'static str, body: Vec<u8>) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind response server");
        let address = listener.local_addr().expect("response server address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut request = [0_u8; 4096];
            let read = stream.read(&mut request).expect("read request");
            assert!(read > 0, "fixture received an empty request");
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .expect("write response headers");
            stream.write_all(&body).expect("write response body");
        });
        (format!("http://{address}"), server)
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
        block_on(async {
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
        let status = block_on(async {
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
    fn http_statuses_map_to_their_attempt_error_equivalence_classes() {
        for (case, status, body, expected) in [
            ("bad_request", 400, "", AttemptErrorKind::Parameter),
            ("not_found", 404, "", AttemptErrorKind::Parameter),
            ("method_not_allowed", 405, "", AttemptErrorKind::Parameter),
            ("conflict", 409, "", AttemptErrorKind::Parameter),
            ("unprocessable", 422, "", AttemptErrorKind::Parameter),
            ("unauthorized", 401, "", AttemptErrorKind::Auth),
            ("forbidden", 403, "", AttemptErrorKind::Auth),
            (
                "unsupported_target_site",
                403,
                "We do NOT support this site.",
                AttemptErrorKind::Parameter,
            ),
            (
                "anonymous_access_blocked",
                403,
                r#"{"code":403,"name":"AbuseAlleviationError","status":40305}"#,
                AttemptErrorKind::Parameter,
            ),
            (
                "payment_required",
                402,
                "",
                AttemptErrorKind::QuotaExhausted,
            ),
            (
                "rate_limited",
                429,
                "slow down",
                AttemptErrorKind::RateLimited,
            ),
            (
                "quota_lowercase",
                429,
                "quota exceeded",
                AttemptErrorKind::QuotaExhausted,
            ),
            (
                "quota_mixed_case",
                429,
                "QuOtA exceeded",
                AttemptErrorKind::QuotaExhausted,
            ),
            (
                "serpapi_searches_exhausted",
                429,
                r#"{"error":"Your account has run out of searches."}"#,
                AttemptErrorKind::QuotaExhausted,
            ),
            (
                "searches_exhausted_mixed_case",
                429,
                "Run Out Of Searches",
                AttemptErrorKind::QuotaExhausted,
            ),
            (
                "serpapi_hourly_throughput",
                429,
                r#"{"error":"Your account has exceeded the hourly searches limit."}"#,
                AttemptErrorKind::RateLimited,
            ),
            ("request_timeout", 408, "", AttemptErrorKind::Timeout),
            ("gateway_timeout", 504, "", AttemptErrorKind::Timeout),
            ("server_error_lower", 500, "", AttemptErrorKind::Network),
            ("server_error_upper", 599, "", AttemptErrorKind::Network),
            ("redirect", 302, "", AttemptErrorKind::Runtime),
            (
                "unclassified_client_error",
                418,
                "",
                AttemptErrorKind::Runtime,
            ),
        ] {
            let status = StatusCode::from_u16(status).expect("valid fixture status");

            assert_eq!(error_kind_for_status(status, body), expected, "case={case}");
        }
    }

    #[test]
    fn slice_budget_preserves_reachable_fallback_slots_at_every_boundary() {
        for (case, remaining, slots, expected) in [
            ("last_slot_zero", 0, 1, Some(Duration::ZERO)),
            (
                "last_slot_below_minimum",
                4,
                1,
                Some(Duration::from_secs(4)),
            ),
            ("two_slots_below_minimum", 9, 2, None),
            ("two_slots_at_minimum", 10, 2, Some(Duration::from_secs(5))),
            (
                "two_slots_fractional",
                11,
                2,
                Some(Duration::from_millis(5_500)),
            ),
            ("three_slots_below_minimum", 14, 3, None),
            (
                "three_slots_at_minimum",
                15,
                3,
                Some(Duration::from_secs(5)),
            ),
            (
                "three_slots_fractional",
                16,
                3,
                Some(Duration::from_nanos(5_333_333_333)),
            ),
        ] {
            assert_eq!(
                slice_budget(Duration::from_secs(remaining), slots),
                expected,
                "case={case}"
            );
        }
    }

    #[test]
    fn build_client_times_out_a_stalled_response() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        let (release, released) = mpsc::channel::<()>();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            // The read timeout also bounds the wait for headers, so on a loaded host the
            // client may give up before this fixture runs; the connection stalls either
            // way, which makes failed writes here expected rather than fatal.
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request);
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nx");
            // The bound expires only when the client has no read timeout; closing then
            // ends the stall with a non-timeout error instead of hanging the test.
            let _ = released.recv_timeout(Duration::from_secs(10));
        });

        let result = block_on(async {
            let client = build_client_with_read_timeout(true, Duration::from_millis(200))
                .expect("build client");
            client
                .get(format!("http://{address}"))
                .send()
                .await?
                .bytes()
                .await
        });
        drop(release);
        server.join().expect("join test server");

        assert!(
            matches!(result, Err(ref error) if error.is_timeout()),
            "shared client did not enforce its read timeout: {result:?}"
        );
    }
}
