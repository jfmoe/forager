#![expect(
    dead_code,
    reason = "each command contract test binary compiles only the fixture capabilities it uses"
)]

//! Shared fixtures and the shared test environment for the command contract tests.

use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, ExitStatus, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

mod fixture_server;

pub(crate) use fixture_server::*;

pub(crate) mod arxiv_feed;
pub(crate) mod browser;
pub(crate) mod doctor;
pub(crate) mod gemini_api;
pub(crate) mod gemini_api_samples;
pub(crate) mod hn;
pub(crate) mod hn_samples;
pub(crate) mod polymarket;
pub(crate) mod polymarket_samples;
pub(crate) mod samples;
pub(crate) mod scholar;
pub(crate) mod smoke;
pub(crate) mod ssrn_samples;
pub(crate) mod x;
pub(crate) mod x_samples;
pub(crate) mod youtube;
pub(crate) mod youtube_api_samples;
pub(crate) mod youtube_web;

const COMMAND_DEADLINE: Duration = Duration::from_secs(30);
const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(5);

/// The stored Jina Reader answer whose page content is `content`.
pub(crate) fn jina_response(content: &str) -> String {
    let mut answer: serde_json::Value =
        serde_json::from_str(&samples::sample("jina/read.json")).expect("stored Reader answer");
    answer["data"]["content"] = serde_json::json!(content);
    answer.to_string()
}

pub(crate) fn request_json(request: &str) -> serde_json::Value {
    let body = request
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .expect("request body");
    serde_json::from_str(body).expect("parse request JSON")
}

/// Reads the single Search Result Journal record an environment wrote.
/// Every key of the execution object in a Search Result Journal record of schema version 1.
pub(crate) const JOURNAL_EXECUTION_KEYS: [&str; 5] = [
    "capability_gaps",
    "deadline_budget",
    "plan_summary",
    "provider_attempts",
    "terminal_attribution",
];

/// The keys of a JSON object, sorted, so a test can assert an object's complete shape.
pub(crate) fn object_keys(value: &serde_json::Value) -> Vec<&str> {
    let mut keys = value
        .as_object()
        .expect("JSON object")
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    keys.sort_unstable();
    keys
}

pub(crate) fn read_only_journal(environment: &RunEnvironment) -> serde_json::Value {
    let entry = fs::read_dir(environment.state_dir.join("forager/journal"))
        .expect("read journal")
        .next()
        .expect("journal entry")
        .expect("valid journal entry");
    serde_json::from_slice(&fs::read(entry.path()).expect("read journal record"))
        .expect("parse journal record")
}

/// The proxy every spawned binary uses: a closed port, so a request that reaches past the
/// fixtures fails at once instead of reaching a real site.
const CLOSED_PROXY: &str = "http://127.0.0.1:9";

/// The forager binary with an empty environment whose HTTP goes through a closed port.
pub(crate) fn forager_command() -> Command {
    forager_command_through(CLOSED_PROXY)
}

pub(crate) fn forager_command_through(proxy: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_forager"));
    command.env_clear();
    proxy_through(&mut command, proxy);
    #[cfg(windows)]
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }
    command
}

/// A program, such as a release script, that keeps the environment it inherits but sends its
/// HTTP, and that of every process it starts, through the closed port.
pub(crate) fn command_behind_closed_proxy(program: &str) -> Command {
    let mut command = Command::new(program);
    proxy_through(&mut command, CLOSED_PROXY);
    command
}

/// Sends HTTP through `proxy`, except to loopback, whatever proxy settings the process would
/// otherwise inherit.
fn proxy_through(command: &mut Command, proxy: &str) {
    for inherited in [
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "ALL_PROXY",
        "no_proxy",
    ] {
        command.env_remove(inherited);
    }
    command
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env("HTTP_PROXY", proxy)
        .env("HTTPS_PROXY", proxy);
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
        let mut command = forager_command();
        command
            .args(arguments)
            .env("FORAGER_CONFIG_DIR", &self.config_dir)
            .env("XDG_STATE_HOME", &self.state_dir)
            .env("HOME", self.root.path());
        for (name, value) in variables {
            command.env(name, value);
        }
        run_command(&mut command, None)
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
