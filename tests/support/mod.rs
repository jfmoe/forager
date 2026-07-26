use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::thread;
use std::time::Duration;

pub(crate) struct Fixture {
    pub(crate) url: String,
    handle: thread::JoinHandle<Vec<String>>,
}

impl Fixture {
    pub(crate) fn start(status: u16, content_type: &str, body: &str) -> Self {
        Self::start_sequence(vec![Response::new(status, content_type, body)])
    }

    pub(crate) fn start_sequence(responses: Vec<Response>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture");
        let address = listener.local_addr().expect("fixture address");
        let handle = thread::spawn(move || {
            let mut requests = Vec::new();
            for response in responses {
                let (mut stream, _) = listener.accept().expect("accept request");
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
                    "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response.status,
                    response.content_type,
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
        self.handle.join().expect("fixture thread")
    }
}

pub(crate) struct Response {
    status: u16,
    content_type: String,
    body: String,
    pub(crate) delay: Duration,
}

impl Response {
    pub(crate) fn new(status: u16, content_type: &str, body: &str) -> Self {
        Self {
            status,
            content_type: content_type.into(),
            body: body.into(),
            delay: Duration::ZERO,
        }
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
