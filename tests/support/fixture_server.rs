//! The local HTTP server behind every fixture, shared by the command contract tests and, through
//! the crate's fixture server module, the in-crate tests. It holds only what both use; a mode only
//! one of them needs lives with that one, built on [`Fixture::serve`]. Extend it when a test needs
//! new server behavior instead of writing another server.

use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

pub(super) const ACCEPT_DEADLINE: Duration = Duration::from_secs(10);
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(10);

pub(crate) struct Fixture {
    pub(crate) url: String,
    handle: thread::JoinHandle<Vec<String>>,
    stop: mpsc::Sender<()>,
    expected_requests: Option<usize>,
}

impl Fixture {
    pub(crate) fn start(status: u16, content_type: &str, body: &str) -> Self {
        Self::start_sequence(vec![Response::new(status, content_type, body)])
    }

    pub(crate) fn start_json(status: u16, body: &str) -> Self {
        Self::start_sequence(vec![Response::json(status, body)])
    }

    pub(crate) fn start_repeating(response: Response) -> Self {
        Self::serve(
            fixture_listener("repeating fixture"),
            None,
            move |listener, stopped| {
                let mut requests = Vec::new();
                accept_until_stopped(&listener, &stopped, "repeating fixture", |stream| {
                    requests.push(respond(stream, &response));
                });
                requests
            },
        )
    }

    pub(crate) fn start_sequence(responses: Vec<Response>) -> Self {
        Self::start_sequence_with_deadline(responses, ACCEPT_DEADLINE)
    }

    pub(crate) fn start_sequence_with_deadline(
        responses: Vec<Response>,
        accept_deadline: Duration,
    ) -> Self {
        Self::serve_sequence(fixture_listener("fixture"), responses, accept_deadline)
    }

    /// Starts a sequence whose responses name the fixture's own URL, such as a link the client
    /// follows next.
    pub(crate) fn start_sequence_for(responses: impl FnOnce(&str) -> Vec<Response>) -> Self {
        Self::start_sequence_for_with_deadline(ACCEPT_DEADLINE, responses)
    }

    /// [`Self::start_sequence_for`], waiting up to `accept_deadline` for each request.
    pub(crate) fn start_sequence_for_with_deadline(
        accept_deadline: Duration,
        responses: impl FnOnce(&str) -> Vec<Response>,
    ) -> Self {
        let listener = fixture_listener("fixture");
        let address = listener.local_addr().expect("fixture address");
        let responses = responses(&format!("http://{address}"));
        Self::serve_sequence(listener, responses, accept_deadline)
    }

    fn serve_sequence(
        listener: TcpListener,
        responses: Vec<Response>,
        accept_deadline: Duration,
    ) -> Self {
        let expected_requests = responses.len();
        Self::serve(
            listener,
            Some(expected_requests),
            move |listener, stopped| {
                let mut requests = Vec::with_capacity(expected_requests);
                for response in responses {
                    let stream = accept_request(
                        &listener,
                        accept_deadline,
                        expected_requests,
                        requests.len(),
                    );
                    requests.push(respond(stream, &response));
                }
                observe_extra_requests(&listener, &stopped, requests)
            },
        )
    }

    /// Answers each request by its method and path, ignoring the query, so concurrent requests
    /// cannot race for a response. Each route answers one request; routes that share a method
    /// and path answer in the order given. [`Self::finish_all`] returns the requests in route
    /// order, and fails when a route was never requested or a request matched no route.
    pub(crate) fn start_routed(routes: Vec<Route>) -> Self {
        let table = Arc::new(Mutex::new(RouteTable::new(routes)));
        Self::serve(
            fixture_listener("routed fixture"),
            None,
            move |listener, stopped| {
                let mut workers = Vec::new();
                accept_until_stopped(&listener, &stopped, "routed fixture", |stream| {
                    let table = Arc::clone(&table);
                    workers.push(thread::spawn(move || answer_routed(stream, &table)));
                });
                for worker in workers {
                    worker
                        .join()
                        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
                }
                Arc::into_inner(table)
                    .expect("routed fixture workers finished")
                    .into_inner()
                    .expect("routed fixture routes")
                    .finish()
            },
        )
    }

    pub(crate) fn start_canary() -> Self {
        Self::serve(
            fixture_listener("fixture canary"),
            None,
            |listener, stopped| {
                let mut requests = Vec::new();
                loop {
                    match stopped.try_recv() {
                        Ok(()) | Err(mpsc::TryRecvError::Disconnected) => break,
                        Err(mpsc::TryRecvError::Empty) => {}
                    }
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            stream
                                .set_read_timeout(Some(ACCEPT_DEADLINE))
                                .expect("set canary stream read timeout");
                            let request = read_request(&mut stream);
                            let _ = stream.write_all(
                            b"HTTP/1.1 500 Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        );
                            requests.push(request);
                            break;
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(ACCEPT_POLL_INTERVAL);
                        }
                        Err(error) => panic!("accept fixture canary request: {error}"),
                    }
                }
                requests
            },
        )
    }

    /// Serves `listener` on a thread of its own with `server`, which receives the stop signal
    /// and returns the requests it recorded. Finishing the fixture asserts `expected_requests`
    /// when it is given.
    pub(super) fn serve(
        listener: TcpListener,
        expected_requests: Option<usize>,
        server: impl FnOnce(TcpListener, mpsc::Receiver<()>) -> Vec<String> + Send + 'static,
    ) -> Self {
        let address = listener.local_addr().expect("fixture address");
        let (stop, stopped) = mpsc::channel();
        let handle = thread::spawn(move || server(listener, stopped));
        Self {
            url: format!("http://{address}"),
            handle,
            stop,
            expected_requests,
        }
    }

    pub(crate) fn finish(self) -> String {
        self.finish_all()
            .into_iter()
            .next()
            .expect("fixture request")
    }

    pub(crate) fn finish_all(self) -> Vec<String> {
        let _ = self.stop.send(());
        let requests = self.handle.join().unwrap_or_else(|panic| {
            std::panic::resume_unwind(panic);
        });
        if let Some(expected_requests) = self.expected_requests {
            assert_eq!(
                requests.len(),
                expected_requests,
                "fixture received an unexpected number of requests: expected {expected_requests}, received {}",
                requests.len()
            );
        }
        requests
    }
}

/// A response that a routed fixture gives to one request with this method and path.
pub(crate) struct Route {
    method: String,
    path: String,
    exchange: Exchange,
}

enum Exchange {
    Pending(Response),
    Answered(String),
}

impl Route {
    pub(crate) fn new(method: &str, path: &str, response: Response) -> Self {
        Self {
            method: method.into(),
            path: path.into(),
            exchange: Exchange::Pending(response),
        }
    }

    fn is_pending(&self) -> bool {
        matches!(self.exchange, Exchange::Pending(_))
    }

    fn label(&self) -> String {
        format!("{} {}", self.method, self.path)
    }
}

struct RouteTable {
    routes: Vec<Route>,
    unmatched: Vec<String>,
}

impl RouteTable {
    fn new(routes: Vec<Route>) -> Self {
        Self {
            routes,
            unmatched: Vec::new(),
        }
    }

    /// Records `request` against the first unanswered route it matches and hands out that
    /// route's response.
    fn answer(&mut self, request: String) -> Option<Response> {
        let line = request.lines().next().unwrap_or_default();
        let mut parts = line.split(' ');
        let method = parts.next().unwrap_or_default();
        let path = parts
            .next()
            .unwrap_or_default()
            .split('?')
            .next()
            .unwrap_or_default();
        let Some(route) = self
            .routes
            .iter_mut()
            .find(|route| route.is_pending() && route.method == method && route.path == path)
        else {
            self.unmatched.push(line.to_owned());
            return None;
        };
        match std::mem::replace(&mut route.exchange, Exchange::Answered(request)) {
            Exchange::Pending(response) => Some(response),
            Exchange::Answered(_) => unreachable!("only a pending route matches"),
        }
    }

    fn finish(self) -> Vec<String> {
        assert!(
            self.unmatched.is_empty(),
            "routed fixture received requests that match no route: {:?}",
            self.unmatched
        );
        let unrequested = self
            .routes
            .iter()
            .filter(|route| route.is_pending())
            .map(Route::label)
            .collect::<Vec<_>>();
        assert!(
            unrequested.is_empty(),
            "routed fixture routes were never requested: {unrequested:?}"
        );
        self.routes
            .into_iter()
            .filter_map(|route| match route.exchange {
                Exchange::Answered(request) => Some(request),
                Exchange::Pending(_) => None,
            })
            .collect()
    }
}

fn answer_routed(mut stream: TcpStream, table: &Mutex<RouteTable>) {
    let request = read_request(&mut stream);
    let response = table
        .lock()
        .expect("lock routed fixture routes")
        .answer(request);
    let response = response.unwrap_or_else(|| Response::new(404, "text/plain", "no fixture route"));
    write_response(stream, &response);
}

pub(super) fn fixture_listener(label: &str) -> TcpListener {
    let listener =
        TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("bind {label}: {error}"));
    listener
        .set_nonblocking(true)
        .unwrap_or_else(|error| panic!("set {label} listener nonblocking: {error}"));
    listener
}

pub(super) fn observe_extra_requests(
    listener: &TcpListener,
    stopped: &mpsc::Receiver<()>,
    mut requests: Vec<String>,
) -> Vec<String> {
    accept_until_stopped(listener, stopped, "extra fixture", |stream| {
        requests.push(respond(
            stream,
            &Response::new(500, "text/plain", "unexpected request"),
        ));
    });
    requests
}

/// Hands each connection to `serve`, blocking and with a read timeout, until the fixture is
/// stopped.
fn accept_until_stopped(
    listener: &TcpListener,
    stopped: &mpsc::Receiver<()>,
    label: &str,
    mut serve: impl FnMut(TcpStream),
) {
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream
                    .set_nonblocking(false)
                    .unwrap_or_else(|error| panic!("set {label} stream blocking: {error}"));
                stream
                    .set_read_timeout(Some(ACCEPT_DEADLINE))
                    .unwrap_or_else(|error| panic!("set {label} stream read timeout: {error}"));
                serve(stream);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                match stopped.try_recv() {
                    Ok(()) | Err(mpsc::TryRecvError::Disconnected) => return,
                    Err(mpsc::TryRecvError::Empty) => {}
                }
                thread::sleep(ACCEPT_POLL_INTERVAL);
            }
            Err(error) => panic!("accept {label} request: {error}"),
        }
    }
}

pub(super) fn respond(mut stream: TcpStream, response: &Response) -> String {
    let request = read_request(&mut stream);
    write_response(stream, response);
    request
}

pub(super) fn read_request(stream: &mut TcpStream) -> String {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let read = stream.read(&mut buffer).expect("read request");
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
        if request_complete(&request) {
            break;
        }
    }
    String::from_utf8(request).expect("UTF-8 request")
}

fn write_response(mut stream: TcpStream, response: &Response) {
    thread::sleep(response.delay);
    let reason = if response.status == 200 {
        "OK"
    } else {
        "Error"
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        response.content_type,
        response
            .declared_content_length
            .unwrap_or(response.body.len())
    );
    let _ = stream.flush();
    let _ = stream.write_all(response.body.as_bytes());
}

pub(super) fn accept_request(
    listener: &TcpListener,
    deadline: Duration,
    expected_requests: usize,
    received_requests: usize,
) -> TcpStream {
    let started = Instant::now();
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream
                    .set_nonblocking(false)
                    .expect("set fixture stream blocking");
                stream
                    .set_read_timeout(Some(ACCEPT_DEADLINE))
                    .expect("set fixture stream read timeout");
                return stream;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                let elapsed = started.elapsed();
                assert!(
                    elapsed < deadline,
                    "fixture timed out waiting for requests: expected {expected_requests}, received {received_requests}"
                );
                thread::sleep(ACCEPT_POLL_INTERVAL.min(deadline.checked_sub(elapsed).unwrap()));
            }
            Err(error) => panic!("accept fixture request: {error}"),
        }
    }
}

pub(crate) struct Response {
    status: u16,
    content_type: String,
    body: String,
    pub(super) delay: Duration,
    pub(super) declared_content_length: Option<usize>,
}

impl Response {
    pub(crate) fn new(status: u16, content_type: &str, body: &str) -> Self {
        Self {
            status,
            content_type: content_type.into(),
            body: body.into(),
            delay: Duration::ZERO,
            declared_content_length: None,
        }
    }

    pub(crate) fn json(status: u16, body: &str) -> Self {
        Self::new(status, "application/json", body)
    }

    pub(crate) fn sse(status: u16, body: &str) -> Self {
        Self::new(status, "text/event-stream", body)
    }
}

/// A 200 answer with the JSON `body`.
pub(crate) fn ok_json(body: &serde_json::Value) -> Response {
    Response::json(200, &body.to_string())
}

fn request_complete(request: &[u8]) -> bool {
    let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") else {
        return false;
    };
    let headers = String::from_utf8_lossy(&request[..header_end + 4]);
    let content_length = headers
        .lines()
        .find_map(|line| {
            line.split_once(':').and_then(|(name, value)| {
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().expect("content length"))
            })
        })
        .unwrap_or(0);
    request.len() >= header_end + 4 + content_length
}

/// The path of a request the fixture server recorded, without its query.
pub(crate) fn path(request: &str) -> String {
    let target = request.split_whitespace().nth(1).expect("request target");
    target.split('?').next().unwrap_or_default().to_owned()
}

/// The value of a header of a request the fixture server recorded, matched case-insensitively.
pub(crate) fn header(request: &str, name: &str) -> Option<String> {
    request
        .split("\r\n\r\n")
        .next()
        .unwrap_or_default()
        .lines()
        .skip(1)
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.trim()
                .eq_ignore_ascii_case(name)
                .then(|| value.trim().to_owned())
        })
}

/// The JSON body of a request the fixture server recorded.
pub(crate) fn request_json(request: &str) -> serde_json::Value {
    let (_, body) = request.split_once("\r\n\r\n").expect("request body");
    serde_json::from_str(body).expect("request JSON")
}

/// The decoded query parameters of a request the fixture server recorded.
pub(crate) fn query(request: &str) -> BTreeMap<String, String> {
    let target = request.split_whitespace().nth(1).expect("request target");
    reqwest::Url::parse(&format!("http://fixture.test{target}"))
        .expect("request URL")
        .query_pairs()
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}

/// The query parameters `pairs` in the form [`query`] returns, for comparison.
pub(crate) fn pairs(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}
