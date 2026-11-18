//! Passive capture: what the page itself requested and received, read from the browser's
//! network events. Capture never sends a request of its own and never rewrites `fetch`, XHR,
//! or WebSocket in the page.
//!
//! Only GET and POST requests are kept, and only those the caller's filter names. A POST body
//! comes from the request event; a response body is read once the request has finished loading.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use reqwest::Url;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task::JoinHandle;

use super::runtime;
use crate::net::{AttemptFailure, MAX_RESPONSE_BYTES};
use crate::providers::cdp::{Cdp, CdpEvent};
use crate::types::Deadline;

// The browser keeps a body only up to its resource buffer, so a body just over the limit can
// still be measured and refused; larger ones are not kept at all.
const RESOURCE_BUFFER_BYTES: u32 = 8 * 1024 * 1024;
const TOTAL_BUFFER_BYTES: u32 = 64 * 1024 * 1024;
const MAX_POST_BYTES: u32 = 1024 * 1024;

/// How much of a request passive capture keeps.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CaptureLevel {
    Ignore,
    /// The request and its status, without the response body.
    Status,
    /// The request, its status, and its response body.
    Body,
}

/// Decides, from a request URL, what capture keeps of it.
pub(crate) type CaptureFilter = fn(&Url) -> CaptureLevel;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RequestMethod {
    Get,
    Post,
}

/// What became of a response body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ResponseBody {
    /// The body, as text.
    Received(String),
    /// The request finished loading, but the browser could not return its body.
    Missing,
    /// The request failed before it finished, with the browser's error text.
    Failed(String),
    /// The filter kept only the status.
    NotKept,
}

/// One request the page made and what came back.
#[derive(Clone, Debug)]
pub(crate) struct Exchange {
    pub(crate) method: RequestMethod,
    pub(crate) url: Url,
    pub(crate) status: Option<u16>,
    /// The POST body, as text.
    pub(crate) request_body: Option<String>,
    pub(crate) body: ResponseBody,
}

pub(super) fn network_enable_params() -> Value {
    json!({
        "maxResourceBufferSize": RESOURCE_BUFFER_BYTES,
        "maxTotalBufferSize": TOTAL_BUFFER_BYTES,
        "maxPostDataSize": MAX_POST_BYTES,
    })
}

#[derive(Default)]
struct State {
    open: HashMap<String, Open>,
    ended: Vec<Exchange>,
    oversized: Option<String>,
}

struct Open {
    level: CaptureLevel,
    exchange: Exchange,
    has_post_data: bool,
}

/// The capture of one page, fed by a task that reads the page's events.
pub(super) struct Capture {
    state: Arc<Mutex<State>>,
    task: JoinHandle<()>,
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Capture {
    pub(super) fn start(
        cdp: Cdp,
        session: String,
        events: UnboundedReceiver<CdpEvent>,
        filter: CaptureFilter,
        work: Deadline,
    ) -> Self {
        let state = Arc::new(Mutex::new(State::default()));
        let task = tokio::spawn(record(
            Recorder {
                cdp,
                session,
                filter,
                work,
                state: Arc::clone(&state),
            },
            events,
        ));
        Self { state, task }
    }

    pub(super) fn take(&self) -> Result<Vec<Exchange>, AttemptFailure> {
        let mut state = lock(&self.state);
        if let Some(url) = &state.oversized {
            return Err(runtime(format!(
                "a response body from {url} exceeded the {} MiB limit",
                MAX_RESPONSE_BYTES / (1024 * 1024)
            )));
        }
        Ok(std::mem::take(&mut state.ended))
    }
}

fn lock(state: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Recorder {
    cdp: Cdp,
    session: String,
    filter: CaptureFilter,
    work: Deadline,
    state: Arc<Mutex<State>>,
}

#[derive(Deserialize)]
struct RequestWillBeSent {
    #[serde(rename = "requestId")]
    id: String,
    request: Request,
    #[serde(rename = "redirectResponse")]
    redirect: Option<Response>,
}

#[derive(Deserialize)]
struct Request {
    url: String,
    method: String,
    #[serde(rename = "postData")]
    post_data: Option<String>,
    #[serde(rename = "postDataEntries", default)]
    post_data_entries: Vec<PostDataEntry>,
    #[serde(rename = "hasPostData", default)]
    has_post_data: bool,
}

#[derive(Deserialize)]
struct PostDataEntry {
    bytes: Option<String>,
}

#[derive(Deserialize)]
struct ResponseReceived {
    #[serde(rename = "requestId")]
    id: String,
    response: Response,
}

#[derive(Deserialize)]
struct Response {
    status: u16,
}

#[derive(Deserialize)]
struct LoadingFinished {
    #[serde(rename = "requestId")]
    id: String,
    #[serde(rename = "encodedDataLength", default)]
    encoded_length: f64,
}

#[derive(Deserialize)]
struct LoadingFailed {
    #[serde(rename = "requestId")]
    id: String,
    #[serde(rename = "errorText", default)]
    error: String,
}

#[derive(Deserialize)]
struct ResponseBodyResult {
    body: String,
    #[serde(rename = "base64Encoded", default)]
    base64: bool,
}

#[derive(Deserialize)]
struct PostDataResult {
    #[serde(rename = "postData")]
    post_data: String,
}

async fn record(recorder: Recorder, mut events: UnboundedReceiver<CdpEvent>) {
    while let Some(event) = events.recv().await {
        if event.session.as_deref() != Some(recorder.session.as_str()) {
            continue;
        }
        let params = event.params;
        match event.method.as_str() {
            "Network.requestWillBeSent" => {
                if let Ok(sent) = serde_json::from_value(params) {
                    recorder.sent(sent);
                }
            }
            "Network.responseReceived" => {
                if let Ok(received) = serde_json::from_value::<ResponseReceived>(params)
                    && let Some(open) = lock(&recorder.state).open.get_mut(&received.id)
                {
                    open.exchange.status = Some(received.response.status);
                }
            }
            "Network.loadingFinished" => {
                if let Ok(finished) = serde_json::from_value(params) {
                    recorder.finished(finished).await;
                }
            }
            "Network.loadingFailed" => {
                if let Ok(failed) = serde_json::from_value::<LoadingFailed>(params) {
                    recorder.end(&failed.id, ResponseBody::Failed(failed.error));
                }
            }
            _ => {}
        }
    }
}

impl Recorder {
    fn sent(&self, sent: RequestWillBeSent) {
        let mut state = lock(&self.state);
        if let Some(redirect) = sent.redirect
            && let Some(mut open) = state.open.remove(&sent.id)
        {
            open.exchange.status = Some(redirect.status);
            open.exchange.body = ResponseBody::NotKept;
            state.ended.push(open.exchange);
        }
        let method = match sent.request.method.as_str() {
            "GET" => RequestMethod::Get,
            "POST" => RequestMethod::Post,
            _ => return,
        };
        let Ok(url) = Url::parse(&sent.request.url) else {
            return;
        };
        let level = (self.filter)(&url);
        if level == CaptureLevel::Ignore {
            return;
        }
        let request_body = (method == RequestMethod::Post)
            .then(|| post_body(&sent.request))
            .flatten();
        state.open.insert(
            sent.id,
            Open {
                level,
                has_post_data: sent.request.has_post_data && request_body.is_none(),
                exchange: Exchange {
                    method,
                    url,
                    status: None,
                    request_body,
                    body: ResponseBody::NotKept,
                },
            },
        );
    }

    async fn finished(&self, finished: LoadingFinished) {
        let Some((level, missing_post, url)) = lock(&self.state)
            .open
            .get(&finished.id)
            .map(|open| (open.level, open.has_post_data, open.exchange.url.clone()))
        else {
            return;
        };
        if missing_post {
            let post = self
                .cdp
                .call_as::<PostDataResult>(
                    Some(&self.session),
                    "Network.getRequestPostData",
                    json!({"requestId": finished.id}),
                    self.work,
                )
                .await;
            if let (Ok(post), Some(open)) = (post, lock(&self.state).open.get_mut(&finished.id)) {
                open.exchange.request_body = Some(post.post_data);
            }
        }
        if level == CaptureLevel::Status {
            self.end(&finished.id, ResponseBody::NotKept);
            return;
        }
        let read = self
            .cdp
            .call_as::<ResponseBodyResult>(
                Some(&self.session),
                "Network.getResponseBody",
                json!({"requestId": finished.id}),
                self.work,
            )
            .await;
        let body = match read.ok().map(decode_body) {
            Some(Some(text)) if text.len() <= MAX_RESPONSE_BYTES => ResponseBody::Received(text),
            Some(_) => {
                self.oversize(&url);
                ResponseBody::Missing
            }
            None if finished.encoded_length > f64::from(RESOURCE_BUFFER_BYTES) => {
                self.oversize(&url);
                ResponseBody::Missing
            }
            None => ResponseBody::Missing,
        };
        self.end(&finished.id, body);
    }

    fn end(&self, id: &str, body: ResponseBody) {
        let mut state = lock(&self.state);
        if let Some(mut open) = state.open.remove(id) {
            open.exchange.body = body;
            state.ended.push(open.exchange);
        }
    }

    fn oversize(&self, url: &Url) {
        let mut origin = url.clone();
        origin.set_query(None);
        lock(&self.state)
            .oversized
            .get_or_insert_with(|| origin.to_string());
    }
}

/// Decodes a body the browser returned; `None` for base64 that does not decode.
fn decode_body(result: ResponseBodyResult) -> Option<String> {
    if !result.base64 {
        return Some(result.body);
    }
    STANDARD
        .decode(result.body)
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

fn post_body(request: &Request) -> Option<String> {
    if let Some(body) = &request.post_data {
        return Some(body.clone());
    }
    let bytes = request
        .post_data_entries
        .iter()
        .filter_map(|entry| entry.bytes.as_deref())
        .map(|bytes| STANDARD.decode(bytes))
        .collect::<Result<Vec<_>, _>>()
        .ok()?
        .concat();
    (!bytes.is_empty()).then(|| String::from_utf8_lossy(&bytes).into_owned())
}
