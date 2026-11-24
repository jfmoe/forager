//! The process-running part of the shared test environment: it starts a process whose HTTP
//! cannot leave the machine and waits for it under a deadline. The release tests use it alone;
//! the command contract tests reach it through the rest of the environment.

use std::ffi::OsStr;
use std::io::{self, Read, Write};
use std::process::{Child, ChildStdin, Command, ExitStatus, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// How long a test waits for a command it started before failing with what the command printed.
pub(crate) const COMMAND_DEADLINE: Duration = Duration::from_secs(30);
const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(5);

/// The proxy every spawned binary uses: a closed port, so a request that reaches past the
/// fixtures fails at once instead of reaching a real site.
pub(crate) const CLOSED_PROXY: &str = "http://127.0.0.1:9";

/// A program, such as a release script or a helper test, that keeps the environment it inherits
/// but sends its HTTP, and that of every process it starts, through the closed port.
pub(crate) fn command_behind_closed_proxy(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    proxy_through(&mut command, CLOSED_PROXY);
    command
}

/// Sends HTTP through `proxy`, except to loopback, whatever proxy settings the process would
/// otherwise inherit.
pub(crate) fn proxy_through(command: &mut Command, proxy: &str) {
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
    wait_for_child(child, &command_label, deadline)
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

/// Waits for `child` to exit and collects its output, killing it and failing the test with what
/// it printed when it outlives `deadline`.
pub(crate) fn wait_for_child(
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
