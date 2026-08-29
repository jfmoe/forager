#![expect(
    dead_code,
    reason = "each integration test compiles only the fixture capabilities it uses"
)]

//! Shared HTTP fixtures for integration tests.
//!
//! When a test needs new server behavior, extend this module instead of creating
//! another server in the test file.

use std::fs;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, ExitStatus, Output, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const ACCEPT_DEADLINE: Duration = Duration::from_secs(10);
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(10);
const COMMAND_DEADLINE: Duration = Duration::from_secs(30);
const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(5);

pub(crate) struct Fixture {
    pub(crate) url: String,
    handle: thread::JoinHandle<Vec<String>>,
    stop: Option<mpsc::Sender<()>>,
}

impl Fixture {
    pub(crate) fn start(status: u16, content_type: &str, body: &str) -> Self {
        Self::start_sequence(vec![Response::new(status, content_type, body)])
    }

    pub(crate) fn start_json(status: u16, body: &str) -> Self {
        Self::start_sequence(vec![Response::json(status, body)])
    }

    pub(crate) fn start_sequence(responses: Vec<Response>) -> Self {
        Self::start_sequence_with_deadline(responses, ACCEPT_DEADLINE)
    }

    pub(crate) fn start_parallel_sequence(responses: Vec<Response>) -> Self {
        Self::start_parallel_sequence_with_deadline(responses, ACCEPT_DEADLINE)
    }

    pub(crate) fn start_parallel_sequence_with_deadline(
        responses: Vec<Response>,
        accept_deadline: Duration,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind parallel fixture");
        listener
            .set_nonblocking(true)
            .expect("set parallel fixture listener nonblocking");
        let address = listener.local_addr().expect("parallel fixture address");
        let handle = thread::spawn(move || {
            let expected_requests = responses.len();
            let mut workers = Vec::with_capacity(expected_requests);
            for response in responses {
                let stream =
                    accept_request(&listener, accept_deadline, expected_requests, workers.len());
                workers.push(thread::spawn(move || respond(stream, &response)));
            }
            workers
                .into_iter()
                .map(|worker| {
                    worker
                        .join()
                        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
                })
                .collect()
        });
        Self {
            url: format!("http://{address}"),
            handle,
            stop: None,
        }
    }

    pub(crate) fn start_sequence_with_deadline(
        responses: Vec<Response>,
        accept_deadline: Duration,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture");
        listener
            .set_nonblocking(true)
            .expect("set fixture listener nonblocking");
        let address = listener.local_addr().expect("fixture address");
        let handle = thread::spawn(move || {
            let expected_requests = responses.len();
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
            requests
        });
        Self {
            url: format!("http://{address}"),
            handle,
            stop: None,
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
                if stopped.try_recv().is_ok() {
                    break;
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
        self.handle.join().unwrap_or_else(|panic| {
            std::panic::resume_unwind(panic);
        })
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
    let mut headers = String::new();
    for (name, value) in &response.headers {
        headers.push_str(name);
        headers.push_str(": ");
        headers.push_str(value);
        headers.push_str("\r\n");
    }
    let _ = write!(
        stream,
        "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        response.content_type,
        headers,
        response
            .declared_content_length
            .unwrap_or(response.body.len())
    );
    let _ = stream.flush();
    thread::sleep(response.body_delay);
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
    headers: Vec<(String, String)>,
    delay: Duration,
    body_delay: Duration,
    declared_content_length: Option<usize>,
}

impl Response {
    pub(crate) fn new(status: u16, content_type: &str, body: &str) -> Self {
        Self {
            status,
            content_type: content_type.into(),
            body: body.into(),
            headers: Vec::new(),
            delay: Duration::ZERO,
            body_delay: Duration::ZERO,
            declared_content_length: None,
        }
    }

    pub(crate) fn json(status: u16, body: &str) -> Self {
        Self::new(status, "application/json", body)
    }

    pub(crate) fn sse(status: u16, body: &str) -> Self {
        Self::new(status, "text/event-stream", body)
    }

    pub(crate) fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub(crate) fn with_session(self, session: &str) -> Self {
        self.with_header("Mcp-Session-Id", session)
    }

    pub(crate) fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    pub(crate) fn with_body_delay(mut self, delay: Duration) -> Self {
        self.body_delay = delay;
        self
    }

    pub(crate) fn with_declared_content_length(mut self, length: usize) -> Self {
        self.declared_content_length = Some(length);
        self
    }
}

pub(crate) fn jina_response(content: &str) -> String {
    serde_json::json!({"data": {"content": content}}).to_string()
}

pub(crate) fn request_json(request: &str) -> serde_json::Value {
    let body = request
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .expect("request body");
    serde_json::from_str(body).expect("parse request JSON")
}

pub(crate) struct RunEnvironment {
    root: tempfile::TempDir,
    pub(crate) config_dir: PathBuf,
    pub(crate) state_dir: PathBuf,
}

impl RunEnvironment {
    pub(crate) fn new(config: &str) -> Self {
        let root = tempfile::tempdir().expect("create isolated root");
        let config_dir = root.path().join("config");
        let state_dir = root.path().join("state");
        fs::create_dir_all(&config_dir).expect("create config directory");
        fs::write(config_dir.join("config.toml"), config).expect("write config");
        Self {
            root,
            config_dir,
            state_dir,
        }
    }

    pub(crate) fn run(&self, arguments: &[&str]) -> Output {
        self.run_with_env(arguments, &[])
    }

    pub(crate) fn run_with_env(&self, arguments: &[&str], variables: &[(&str, &str)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_forager"));
        command
            .args(arguments)
            .env_clear()
            .env("FORAGER_CONFIG_DIR", &self.config_dir)
            .env("XDG_STATE_HOME", &self.state_dir)
            .env("HOME", self.root.path());
        for (name, value) in variables {
            command.env(name, value);
        }
        run_command(&mut command, None)
    }

    pub(crate) fn run_with_stdin(&self, arguments: &[&str], stdin: &str) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_forager"));
        command
            .args(arguments)
            .env_clear()
            .env("FORAGER_CONFIG_DIR", &self.config_dir)
            .env("XDG_STATE_HOME", &self.state_dir)
            .env("HOME", self.root.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        run_command(&mut command, Some(stdin.as_bytes()))
    }
}

pub(crate) fn run_command(command: &mut Command, stdin: Option<&[u8]>) -> Output {
    run_command_with_deadline(command, stdin, COMMAND_DEADLINE)
}

pub(crate) fn run_command_with_deadline(
    command: &mut Command,
    stdin: Option<&[u8]>,
    deadline: Duration,
) -> Output {
    let command_label = format!("{command:?}");
    match stdin {
        Some(_) => command.stdin(Stdio::piped()),
        None => command.stdin(Stdio::null()),
    };
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = spawn_command(command).expect("spawn test command");
    if let Some(input) = stdin {
        child
            .take_stdin()
            .expect("capture test command stdin")
            .write_all(input)
            .expect("write test command stdin");
    }
    wait_for_managed_child(child, &command_label, deadline)
}

pub(crate) struct ManagedChild {
    child: Child,
    stdout: thread::JoinHandle<io::Result<Vec<u8>>>,
    stderr: thread::JoinHandle<io::Result<Vec<u8>>>,
}

impl ManagedChild {
    pub(crate) fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.child.stdin.take()
    }

    fn finish(self, status: ExitStatus) -> Output {
        Output {
            status,
            stdout: join_reader(self.stdout, "stdout"),
            stderr: join_reader(self.stderr, "stderr"),
        }
    }
}

pub(crate) fn spawn_command(command: &mut Command) -> io::Result<ManagedChild> {
    let mut child = command.spawn()?;
    let stdout = child.stdout.take().expect("capture test command stdout");
    let stderr = child.stderr.take().expect("capture test command stderr");
    Ok(ManagedChild {
        child,
        stdout: spawn_reader(stdout),
        stderr: spawn_reader(stderr),
    })
}

pub(crate) fn wait_for_child(child: ManagedChild, command_label: &str) -> Output {
    wait_for_managed_child(child, command_label, COMMAND_DEADLINE)
}

fn wait_for_managed_child(
    mut child: ManagedChild,
    command_label: &str,
    deadline: Duration,
) -> Output {
    let command_deadline = Instant::now() + deadline;
    loop {
        match child.child.try_wait() {
            Ok(Some(status)) => return child.finish(status),
            Ok(None) => {}
            Err(error) => {
                let _ = child.child.kill();
                let _ = child.child.wait();
                panic!("could not poll test command {command_label}: {error}");
            }
        }

        let now = Instant::now();
        if now >= command_deadline {
            let _ = child.child.kill();
            let status = child.child.wait().expect("reap timed out test command");
            let output = child.finish(status);
            panic!(
                "test command exceeded {deadline:?}: {command_label}\nstatus: {}\nstdout: {}\nstderr: {}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
        }
        thread::sleep(COMMAND_POLL_INTERVAL.min(command_deadline - now));
    }
}

fn spawn_reader(mut reader: impl Read + Send + 'static) -> thread::JoinHandle<io::Result<Vec<u8>>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).map(|_| bytes)
    })
}

fn join_reader(reader: thread::JoinHandle<io::Result<Vec<u8>>>, label: &str) -> Vec<u8> {
    reader
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
        .unwrap_or_else(|error| panic!("capture test command {label}: {error}"))
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
