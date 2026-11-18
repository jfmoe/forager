//! A minimal Chrome DevTools Protocol client over the browser's debugging pipe
//! (`--remote-debugging-pipe`): forager writes commands to one pipe and reads answers and events
//! from the other, each message one JSON object followed by a NUL byte. It opens no port.
//!
//! The client pairs each answer with its command by `id`, hands every event to one receiver,
//! and bounds each message. It knows no CDP domain; callers name the methods they use.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::types::Deadline;

/// The largest message either side may send. A response body travels base64-encoded inside one
/// message, so this leaves room for a body at the transport's body limit.
pub(crate) const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

const READ_CHUNK_BYTES: usize = 64 * 1024;

/// Why a CDP command did not return a result.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub(crate) enum CdpError {
    /// The browser answered the command with an error.
    #[error("CDP `{method}` failed: {message}")]
    Command { method: String, message: String },
    /// No answer arrived before the command's deadline.
    #[error("CDP `{method}` got no answer before its deadline")]
    Timeout { method: String },
    /// The connection ended before the answer arrived.
    #[error("{0}")]
    Closed(Closure),
}

/// Why the connection ended. The first cause is kept and reported to every later command.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub(crate) enum Closure {
    #[error("the browser closed its debugging pipe")]
    Eof,
    #[error("a CDP message exceeded the {MAX_MESSAGE_BYTES}-byte limit")]
    TooLarge,
    #[error("the browser sent a message that is not CDP JSON: {0}")]
    Malformed(String),
    #[error("the CDP pipe failed: {0}")]
    Io(String),
}

/// An event the browser sent, with the session it belongs to; browser-level events have none.
#[derive(Debug)]
pub(crate) struct CdpEvent {
    pub(crate) session: Option<String>,
    pub(crate) method: String,
    pub(crate) params: Value,
}

type Writer = Box<dyn AsyncWrite + Send + Unpin>;
type Answer = Result<Value, CdpError>;

#[derive(Default)]
struct Shared {
    pending: HashMap<u64, (String, oneshot::Sender<Answer>)>,
    closed: Option<Closure>,
}

/// One CDP connection. Clones share it; the reader stops when the last clone is dropped.
#[derive(Clone)]
pub(crate) struct Cdp {
    shared: Arc<Mutex<Shared>>,
    writer: Arc<tokio::sync::Mutex<Writer>>,
    next_id: Arc<AtomicU64>,
    _reader: Arc<AbortOnDrop>,
}

struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[derive(Deserialize)]
struct Incoming {
    id: Option<u64>,
    method: Option<String>,
    #[serde(rename = "sessionId")]
    session: Option<String>,
    #[serde(default)]
    params: Value,
    result: Option<Value>,
    error: Option<ProtocolError>,
}

#[derive(Deserialize)]
struct ProtocolError {
    message: String,
}

impl Cdp {
    /// Starts a connection that reads answers and events from `reader` and writes commands to
    /// `writer`. Returns it with the receiver of every event the browser sends.
    ///
    /// The event channel is unbounded on purpose: the reader must never wait for the event
    /// consumer, because that consumer itself waits for answers that only the reader delivers.
    /// Every event is bounded by [`MAX_MESSAGE_BYTES`] and lives at most as long as one
    /// browser command.
    pub(crate) fn start(
        reader: impl AsyncRead + Send + Unpin + 'static,
        writer: impl AsyncWrite + Send + Unpin + 'static,
    ) -> (Self, mpsc::UnboundedReceiver<CdpEvent>) {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let (events, received) = mpsc::unbounded_channel();
        let task = tokio::spawn(read_messages(reader, Arc::clone(&shared), events));
        let cdp = Self {
            shared,
            writer: Arc::new(tokio::sync::Mutex::new(Box::new(writer))),
            next_id: Arc::new(AtomicU64::new(1)),
            _reader: Arc::new(AbortOnDrop(task)),
        };
        (cdp, received)
    }

    /// Sends `method` with `params`, to `session` when given, and waits for its answer until
    /// `deadline`.
    ///
    /// # Errors
    ///
    /// Returns the browser's error answer, a timeout at `deadline`, or the reason the
    /// connection ended.
    pub(crate) async fn call(
        &self,
        session: Option<&str>,
        method: &str,
        params: Value,
        deadline: Deadline,
    ) -> Result<Value, CdpError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut message = json!({"id": id, "method": method, "params": params});
        if let Some(session) = session {
            message["sessionId"] = Value::String(session.to_owned());
        }
        let mut bytes = serde_json::to_vec(&message).map_err(|error| CdpError::Command {
            method: method.to_owned(),
            message: format!("cannot encode the command: {error}"),
        })?;
        if bytes.len() > MAX_MESSAGE_BYTES {
            return Err(CdpError::Closed(Closure::TooLarge));
        }
        bytes.push(0);
        let (answer, answered) = oneshot::channel();
        {
            let mut shared = lock(&self.shared);
            if let Some(closure) = &shared.closed {
                return Err(CdpError::Closed(closure.clone()));
            }
            shared.pending.insert(id, (method.to_owned(), answer));
        }
        if let Err(error) = self.write(&bytes).await {
            lock(&self.shared).pending.remove(&id);
            return Err(CdpError::Closed(Closure::Io(error.to_string())));
        }
        let remaining = deadline.remaining().unwrap_or_default();
        match tokio::time::timeout(remaining, answered).await {
            Ok(Ok(answer)) => answer,
            Ok(Err(_)) => Err(CdpError::Closed(self.closure())),
            Err(_) => {
                lock(&self.shared).pending.remove(&id);
                Err(CdpError::Timeout {
                    method: method.to_owned(),
                })
            }
        }
    }

    /// Like [`Cdp::call`], decoding the result as `T`.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`Cdp::call`], or a command error for a result of another shape.
    pub(crate) async fn call_as<T: DeserializeOwned>(
        &self,
        session: Option<&str>,
        method: &str,
        params: Value,
        deadline: Deadline,
    ) -> Result<T, CdpError> {
        let result = self.call(session, method, params, deadline).await?;
        serde_json::from_value(result).map_err(|error| CdpError::Command {
            method: method.to_owned(),
            message: format!("unexpected result: {error}"),
        })
    }

    /// Whether the connection has ended, so no command can succeed any more.
    pub(crate) fn is_closed(&self) -> bool {
        lock(&self.shared).closed.is_some()
    }

    async fn write(&self, bytes: &[u8]) -> std::io::Result<()> {
        let mut writer = self.writer.lock().await;
        writer.write_all(bytes).await?;
        writer.flush().await
    }

    fn closure(&self) -> Closure {
        lock(&self.shared).closed.clone().unwrap_or(Closure::Eof)
    }
}

fn lock(shared: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

async fn read_messages(
    mut reader: impl AsyncRead + Unpin,
    shared: Arc<Mutex<Shared>>,
    events: mpsc::UnboundedSender<CdpEvent>,
) {
    let closure = read_until_closed(&mut reader, &shared, &events).await;
    let mut shared = lock(&shared);
    shared.closed.get_or_insert(closure);
    let closure = shared.closed.clone().unwrap_or(Closure::Eof);
    for (_, (_, answer)) in shared.pending.drain() {
        let _ = answer.send(Err(CdpError::Closed(closure.clone())));
    }
}

async fn read_until_closed(
    reader: &mut (impl AsyncRead + Unpin),
    shared: &Mutex<Shared>,
    events: &mpsc::UnboundedSender<CdpEvent>,
) -> Closure {
    let mut buffer = Vec::new();
    // Bytes before `scanned` hold no NUL, so each byte is searched once.
    let mut scanned = 0;
    let mut chunk = vec![0; READ_CHUNK_BYTES];
    loop {
        while let Some(offset) = buffer[scanned..].iter().position(|byte| *byte == 0) {
            let end = scanned + offset;
            let message = buffer.drain(..=end).collect::<Vec<_>>();
            scanned = 0;
            if let Err(closure) = dispatch(&message[..end], shared, events) {
                return closure;
            }
        }
        scanned = buffer.len();
        if buffer.len() > MAX_MESSAGE_BYTES {
            return Closure::TooLarge;
        }
        match reader.read(&mut chunk).await {
            Ok(0) => return Closure::Eof,
            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
            Err(error) => return Closure::Io(error.to_string()),
        }
    }
}

fn dispatch(
    message: &[u8],
    shared: &Mutex<Shared>,
    events: &mpsc::UnboundedSender<CdpEvent>,
) -> Result<(), Closure> {
    if message.len() > MAX_MESSAGE_BYTES {
        return Err(Closure::TooLarge);
    }
    let incoming = serde_json::from_slice::<Incoming>(message)
        .map_err(|error| Closure::Malformed(error.to_string()))?;
    if let Some(id) = incoming.id {
        let Some((method, answer)) = lock(shared).pending.remove(&id) else {
            return Ok(());
        };
        let result = match (incoming.error, incoming.result) {
            (Some(error), _) => Err(CdpError::Command {
                method,
                message: error.message,
            }),
            (None, result) => Ok(result.unwrap_or(Value::Null)),
        };
        let _ = answer.send(result);
    } else if let Some(method) = incoming.method {
        let _ = events.send(CdpEvent {
            session: incoming.session,
            method,
            params: incoming.params,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::{Value, json};
    use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream, duplex};

    use super::{Cdp, CdpError, Closure, MAX_MESSAGE_BYTES};
    use crate::types::Deadline;

    /// The browser's side of the two pipes: what forager wrote, and where to answer.
    struct Browser {
        commands: DuplexStream,
        answers: DuplexStream,
    }

    impl Browser {
        async fn next_command(&mut self) -> Value {
            let mut message = Vec::new();
            loop {
                let byte = self.commands.read_u8().await.expect("read command byte");
                if byte == 0 {
                    return serde_json::from_slice(&message).expect("command is JSON");
                }
                message.push(byte);
            }
        }

        async fn send(&mut self, message: &Value) {
            let mut bytes = serde_json::to_vec(message).expect("encode message");
            bytes.push(0);
            self.answers.write_all(&bytes).await.expect("send message");
        }
    }

    fn connect() -> (
        Cdp,
        tokio::sync::mpsc::UnboundedReceiver<super::CdpEvent>,
        Browser,
    ) {
        let (writer, commands) = duplex(MAX_MESSAGE_BYTES * 2);
        let (answers, reader) = duplex(MAX_MESSAGE_BYTES * 2);
        let (cdp, events) = Cdp::start(reader, writer);
        (cdp, events, Browser { commands, answers })
    }

    fn run(test: impl std::future::Future<Output = ()>) {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build test runtime")
            .block_on(test);
    }

    fn soon() -> Deadline {
        Deadline::new(Duration::from_secs(5))
    }

    #[test]
    fn a_command_is_one_nul_terminated_json_message_with_its_session() {
        run(async {
            let (cdp, _events, mut browser) = connect();

            let call = tokio::spawn(async move {
                cdp.call(
                    Some("session-1"),
                    "Runtime.evaluate",
                    json!({"expression": "1"}),
                    soon(),
                )
                .await
            });
            let command = browser.next_command().await;
            browser
                .send(&json!({"id": command["id"], "result": {"ok": true}}))
                .await;

            assert_eq!(
                (command, call.await.expect("join call")),
                (
                    json!({"id": 1, "method": "Runtime.evaluate", "params": {"expression": "1"}, "sessionId": "session-1"}),
                    Ok(json!({"ok": true}))
                )
            );
        });
    }

    #[test]
    fn answers_pair_with_their_commands_in_any_order() {
        run(async {
            let (cdp, _events, mut browser) = connect();
            let first = tokio::spawn({
                let cdp = cdp.clone();
                async move { cdp.call(None, "First.method", json!({}), soon()).await }
            });
            let first_id = browser.next_command().await["id"].clone();
            let second = tokio::spawn({
                let cdp = cdp.clone();
                async move { cdp.call(None, "Second.method", json!({}), soon()).await }
            });
            let second_id = browser.next_command().await["id"].clone();

            browser
                .send(&json!({"id": second_id, "result": {"answer": "second"}}))
                .await;
            browser
                .send(&json!({"id": first_id, "result": {"answer": "first"}}))
                .await;

            assert_eq!(
                (
                    first.await.expect("join first"),
                    second.await.expect("join second")
                ),
                (
                    Ok(json!({"answer": "first"})),
                    Ok(json!({"answer": "second"}))
                )
            );
        });
    }

    #[test]
    fn an_error_answer_fails_the_command_with_the_browser_message() {
        run(async {
            let (cdp, _events, mut browser) = connect();
            let call = tokio::spawn(async move {
                cdp.call(None, "Page.navigate", json!({"url": "x"}), soon())
                    .await
            });
            let id = browser.next_command().await["id"].clone();

            browser
                .send(&json!({"id": id, "error": {"code": -32000, "message": "Cannot navigate to invalid URL"}}))
                .await;

            assert_eq!(
                call.await.expect("join call"),
                Err(CdpError::Command {
                    method: "Page.navigate".into(),
                    message: "Cannot navigate to invalid URL".into()
                })
            );
        });
    }

    #[test]
    fn events_reach_the_receiver_with_their_session() {
        run(async {
            let (_cdp, mut events, mut browser) = connect();

            browser
                .send(&json!({"method": "Network.loadingFinished", "sessionId": "page", "params": {"requestId": "7"}}))
                .await;
            let event = events.recv().await.expect("event");

            assert_eq!(
                (
                    event.session.as_deref(),
                    event.method.as_str(),
                    event.params
                ),
                (
                    Some("page"),
                    "Network.loadingFinished",
                    json!({"requestId": "7"})
                )
            );
        });
    }

    #[test]
    fn a_command_without_an_answer_fails_at_its_deadline() {
        run(async {
            let (cdp, _events, _browser) = connect();

            let result = cdp
                .call(
                    None,
                    "Browser.getVersion",
                    json!({}),
                    Deadline::new(Duration::from_millis(50)),
                )
                .await;

            assert_eq!(
                result,
                Err(CdpError::Timeout {
                    method: "Browser.getVersion".into()
                })
            );
        });
    }

    #[test]
    fn a_message_over_the_limit_ends_the_connection_for_every_command() {
        run(async {
            let (cdp, _events, mut browser) = connect();
            let pending = tokio::spawn({
                let cdp = cdp.clone();
                async move {
                    cdp.call(None, "Network.getResponseBody", json!({}), soon())
                        .await
                }
            });
            browser.next_command().await;

            browser
                .answers
                .write_all(&vec![b'x'; MAX_MESSAGE_BYTES + 1])
                .await
                .expect("send oversized message");
            let pending = pending.await.expect("join pending call");
            let later = cdp
                .call(None, "Browser.getVersion", json!({}), soon())
                .await;

            assert_eq!(
                (pending, later),
                (
                    Err(CdpError::Closed(Closure::TooLarge)),
                    Err(CdpError::Closed(Closure::TooLarge))
                )
            );
        });
    }

    #[test]
    fn closing_the_pipe_fails_the_pending_command() {
        run(async {
            let (cdp, _events, mut browser) = connect();
            let pending = tokio::spawn({
                let cdp = cdp.clone();
                async move {
                    cdp.call(None, "Browser.getVersion", json!({}), soon())
                        .await
                }
            });
            browser.next_command().await;

            drop(browser);
            let pending = pending.await.expect("join pending call");

            assert_eq!(
                (pending, cdp.is_closed()),
                (Err(CdpError::Closed(Closure::Eof)), true)
            );
        });
    }

    #[test]
    fn output_that_is_not_json_ends_the_connection_as_malformed() {
        run(async {
            let (cdp, _events, mut browser) = connect();
            let pending = tokio::spawn(async move {
                cdp.call(None, "Browser.getVersion", json!({}), soon())
                    .await
            });
            browser.next_command().await;

            browser
                .answers
                .write_all(b"not json\0")
                .await
                .expect("send garbage");

            assert!(
                matches!(
                    pending.await.expect("join pending call"),
                    Err(CdpError::Closed(Closure::Malformed(_)))
                ),
                "garbage on the pipe did not end the connection as malformed"
            );
        });
    }
}
