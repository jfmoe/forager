use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const ACCEPT_DEADLINE: Duration = Duration::from_secs(10);
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(10);

pub(crate) struct Fixture {
    pub(crate) url: String,
    handle: thread::JoinHandle<Vec<String>>,
}

impl Fixture {
    pub(crate) fn start(status: u16, content_type: &str, body: &str) -> Self {
        Self::start_sequence(vec![Response::new(status, content_type, body)])
    }

    pub(crate) fn start_sequence(responses: Vec<Response>) -> Self {
        Self::start_sequence_with_deadline(responses, ACCEPT_DEADLINE)
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
                let mut stream = accept_request(
                    &listener,
                    accept_deadline,
                    expected_requests,
                    requests.len(),
                );
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
                let headers = response
                    .headers
                    .iter()
                    .map(|(name, value)| format!("{name}: {value}\r\n"))
                    .collect::<String>();
                let _ = write!(
                    stream,
                    "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response.status,
                    response.content_type,
                    headers,
                    response.body.len(),
                    response.body
                );
                requests.push(String::from_utf8(request).expect("UTF-8 request"));
            }
            requests
        });
        Self {
            url: format!("http://{address}"),
            handle,
        }
    }

    pub(crate) fn finish(self) -> String {
        self.finish_all()
            .into_iter()
            .next()
            .expect("fixture request")
    }

    pub(crate) fn finish_all(self) -> Vec<String> {
        self.handle.join().unwrap_or_else(|panic| {
            std::panic::resume_unwind(panic);
        })
    }
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
                return stream;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                let elapsed = started.elapsed();
                assert!(
                    elapsed < deadline,
                    "fixture timed out waiting for requests: expected {expected_requests}, received {received_requests}"
                );
                thread::sleep(ACCEPT_POLL_INTERVAL.min(deadline - elapsed));
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
    pub(crate) delay: Duration,
}

impl Response {
    pub(crate) fn new(status: u16, content_type: &str, body: &str) -> Self {
        Self {
            status,
            content_type: content_type.into(),
            body: body.into(),
            headers: Vec::new(),
            delay: Duration::ZERO,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
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
        command.output().expect("run forager")
    }

    #[allow(dead_code)]
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
        let mut child = command.spawn().expect("spawn forager");
        child
            .stdin
            .take()
            .expect("child stdin")
            .write_all(stdin.as_bytes())
            .expect("write child stdin");
        child.wait_with_output().expect("wait forager")
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
