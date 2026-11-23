//! The local HTTP server behind every fixture, shared by the command contract tests and, through
//! the crate root, the in-crate tests. Extend it when a test needs new server behavior instead of
//! writing another server.

use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

const ACCEPT_DEADLINE: Duration = Duration::from_secs(10);
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(10);
const SYNCHRONIZATION_DEADLINE: Duration = Duration::from_secs(2);

pub(crate) struct Fixture {
    pub(crate) url: String,
    handle: thread::JoinHandle<Vec<String>>,
    stop: Option<mpsc::Sender<()>>,
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
        let listener = fixture_listener("repeating fixture");
        let address = listener.local_addr().expect("repeating fixture address");
        let (stop, stopped) = mpsc::channel();
        let handle = thread::spawn(move || {
            let mut requests = Vec::new();
            accept_until_stopped(&listener, &stopped, "repeating fixture", |stream| {
                requests.push(respond(stream, &response));
            });
            requests
        });
        Self {
            url: format!("http://{address}"),
            handle,
            stop: Some(stop),
            expected_requests: None,
        }
    }

    pub(crate) fn start_sequence(responses: Vec<Response>) -> Self {
        Self::start_sequence_with_deadline(responses, ACCEPT_DEADLINE)
    }

    pub(crate) fn start_parallel_sequence(responses: Vec<Response>) -> Self {
        Self::start_parallel_sequence_with_deadline(responses, ACCEPT_DEADLINE)
    }

    pub(crate) fn start_synchronized_sequence(responses: Vec<Response>) -> Self {
        Self::start_synchronized_sequences(vec![responses])
            .pop()
            .expect("synchronized fixture")
    }

    pub(crate) fn start_synchronized_sequences(
        response_sequences: Vec<Vec<Response>>,
    ) -> Vec<Self> {
        let total_requests = response_sequences.iter().map(Vec::len).sum();
        let synchronization = Arc::new(ConnectionBarrier::new(total_requests));
        response_sequences
            .into_iter()
            .map(|responses| {
                let listener = fixture_listener("synchronized fixture");
                let address = listener.local_addr().expect("synchronized fixture address");
                let expected_requests = responses.len();
                let synchronization = Arc::clone(&synchronization);
                let (stop, stopped) = mpsc::channel();
                let handle = thread::spawn(move || {
                    let workers = responses
                        .into_iter()
                        .enumerate()
                        .map(|(received, response)| {
                            let stream = accept_request(
                                &listener,
                                ACCEPT_DEADLINE,
                                expected_requests,
                                received,
                            );
                            let synchronization = Arc::clone(&synchronization);
                            thread::spawn(move || {
                                synchronization.arrive_and_wait();
                                respond(stream, &response)
                            })
                        })
                        .collect::<Vec<_>>();
                    let requests = join_workers(workers);
                    observe_extra_requests(&listener, &stopped, requests)
                });
                Self {
                    url: format!("http://{address}"),
                    handle,
                    stop: Some(stop),
                    expected_requests: Some(expected_requests),
                }
            })
            .collect()
    }

    pub(crate) fn start_parallel_sequence_with_deadline(
        responses: Vec<Response>,
        accept_deadline: Duration,
    ) -> Self {
        let listener = fixture_listener("parallel fixture");
        let address = listener.local_addr().expect("parallel fixture address");
        let expected_requests = responses.len();
        let (stop, stopped) = mpsc::channel();
        let handle = thread::spawn(move || {
            let mut workers = Vec::with_capacity(expected_requests);
            for response in responses {
                let stream =
                    accept_request(&listener, accept_deadline, expected_requests, workers.len());
                workers.push(thread::spawn(move || respond(stream, &response)));
            }
            let requests = join_workers(workers);
            observe_extra_requests(&listener, &stopped, requests)
        });
        Self {
            url: format!("http://{address}"),
            handle,
            stop: Some(stop),
            expected_requests: Some(expected_requests),
        }
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
        let address = listener.local_addr().expect("fixture address");
        let expected_requests = responses.len();
        let (stop, stopped) = mpsc::channel();
        let handle = thread::spawn(move || {
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
        });
        Self {
            url: format!("http://{address}"),
            handle,
            stop: Some(stop),
            expected_requests: Some(expected_requests),
        }
    }

    pub(crate) fn start_canary() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture canary");
        listener
            .set_nonblocking(true)
            .expect("set fixture canary nonblocking");
        let address = listener.local_addr().expect("fixture canary address");
        let (stop, stopped) = mpsc::channel();
        let handle = thread::spawn(move || {
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
                        let mut request = Vec::new();
                        let mut buffer = [0_u8; 4096];
                        loop {
                            let read = stream.read(&mut buffer).expect("read canary request");
                            if read == 0 {
                                break;
                            }
                            request.extend_from_slice(&buffer[..read]);
                            if request_complete(&request) {
                                break;
                            }
                        }
                        let _ = stream.write_all(
                            b"HTTP/1.1 500 Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        );
                        requests.push(String::from_utf8(request).expect("UTF-8 request"));
                        break;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(ACCEPT_POLL_INTERVAL);
                    }
                    Err(error) => panic!("accept fixture canary request: {error}"),
                }
            }
            requests
        });
        Self {
            url: format!("http://{address}"),
            handle,
            stop: Some(stop),
            expected_requests: None,
        }
    }

    pub(crate) fn finish(self) -> String {
        self.finish_all()
            .into_iter()
            .next()
            .expect("fixture request")
    }

    pub(crate) fn finish_all(mut self) -> Vec<String> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
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

struct ConnectionBarrier {
    expected: usize,
    arrived: Mutex<usize>,
    all_arrived: Condvar,
}

impl ConnectionBarrier {
    fn new(expected: usize) -> Self {
        Self {
            expected,
            arrived: Mutex::new(0),
            all_arrived: Condvar::new(),
        }
    }

    fn arrive_and_wait(&self) {
        let mut arrived = self.arrived.lock().expect("lock fixture barrier");
        *arrived += 1;
        if *arrived == self.expected {
            self.all_arrived.notify_all();
            return;
        }
        let (arrived, timeout) = self
            .all_arrived
            .wait_timeout_while(arrived, SYNCHRONIZATION_DEADLINE, |arrived| {
                *arrived < self.expected
            })
            .expect("wait on fixture barrier");
        assert!(
            !timeout.timed_out(),
            "fixture timed out waiting for concurrent requests: expected {}, received {}",
            self.expected,
            *arrived
        );
    }
}

fn fixture_listener(label: &str) -> TcpListener {
    let listener =
        TcpListener::bind("127.0.0.1:0").unwrap_or_else(|error| panic!("bind {label}: {error}"));
    listener
        .set_nonblocking(true)
        .unwrap_or_else(|error| panic!("set {label} listener nonblocking: {error}"));
    listener
}

fn join_workers(workers: Vec<thread::JoinHandle<String>>) -> Vec<String> {
    workers
        .into_iter()
        .map(|worker| {
            worker
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
        })
        .collect()
}

fn observe_extra_requests(
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

fn respond(mut stream: TcpStream, response: &Response) -> String {
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
    String::from_utf8(request).expect("UTF-8 request")
}

fn accept_request(
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
    delay: Duration,
    declared_content_length: Option<usize>,
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

    pub(crate) fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    pub(crate) fn with_declared_content_length(mut self, length: usize) -> Self {
        self.declared_content_length = Some(length);
        self
    }
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
