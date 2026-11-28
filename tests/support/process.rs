//! The process-running part of the shared test environment: it starts a process whose HTTP
//! cannot leave the machine and waits for it under a deadline. The release tests use it alone;
//! the command contract tests reach it through the rest of the environment.

use std::env;
use std::ffi::OsStr;
use std::io::{self, Read, Write};
use std::process::{Child, ChildStdin, Command, ExitStatus, Output, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

/// The runner configuration, which sets the time limit of each test.
const RUNNER_CONFIG: &str = include_str!("../../.config/nextest.toml");

/// What a test's command deadline leaves of the runner's limit: enough for the test's setup
/// before its first wait and for killing a hung command and reporting its output, which take
/// milliseconds. A share of the limit would instead leave most of a long limit unused while
/// leaving too little of a short one for a slow first launch.
const DIAGNOSIS_MARGIN: Duration = Duration::from_millis(250);

/// How long a test may still wait for a command it started before failing with what the command
/// printed: what is left of the time limit the runner applies to the running test less
/// [`DIAGNOSIS_MARGIN`], counted from the test's first wait, so a command that hangs is reported
/// with its output before the runner would terminate the test, however many commands ran before
/// it. Outside the runner, which sets no limit, the limit is that of the exemption list's entries.
pub(crate) fn command_deadline() -> Duration {
    const UNLIMITED: Duration = Duration::from_mins(1);
    static BUDGET_ENDS: OnceLock<Instant> = OnceLock::new();
    let ends = BUDGET_ENDS.get_or_init(|| {
        Instant::now()
            + runner_limit()
                .unwrap_or(UNLIMITED)
                .saturating_sub(DIAGNOSIS_MARGIN)
    });
    ends.saturating_duration_since(Instant::now())
}

/// The time limit of the running test under its profile: that of the first override whose
/// filter names the test or its binary, from the profile's own overrides before the default
/// profile's, and otherwise the profile's own limit or the default profile's. A repository rule
/// keeps the runner configuration to the forms this reads.
fn runner_limit() -> Option<Duration> {
    let profile = env::var("NEXTEST_PROFILE").ok()?;
    let binary = env::var("NEXTEST_BINARY_ID").ok()?;
    let test = env::var("NEXTEST_TEST_NAME").ok()?;
    let config: toml::Table = RUNNER_CONFIG
        .parse()
        .expect("parse the runner configuration");
    let profiles = [profile.as_str(), "default"]
        .iter()
        .filter_map(|name| config.get("profile")?.get(*name))
        .collect::<Vec<_>>();
    let names_this_test = |filter: &str| {
        filter.contains(&format!("test(={test})")) || filter == format!("binary_id({binary})")
    };
    let overridden = profiles.iter().find_map(|profile| {
        profile
            .get("overrides")?
            .as_array()?
            .iter()
            .find(|entry| entry["filter"].as_str().is_some_and(&names_this_test))
    });
    let timeout = match overridden {
        Some(entry) => &entry["slow-timeout"],
        None => profiles
            .iter()
            .find_map(|profile| profile.get("slow-timeout"))?,
    };
    let period = timeout["period"]
        .as_str()?
        .strip_suffix('s')?
        .parse::<u64>()
        .ok()?;
    let terminate_after = u32::try_from(timeout["terminate-after"].as_integer()?).ok()?;
    Some(Duration::from_secs(period) * terminate_after)
}

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
    run_command_with_deadline(command, stdin, command_deadline())
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
    spawn_command_reading_stdout(command, u64::MAX)
}

/// Starts `command` like [`spawn_command`], but closes its stdout once the first `limit` bytes
/// are read, as a reader such as `head` does.
pub(crate) fn spawn_command_reading_stdout(
    command: &mut Command,
    limit: u64,
) -> io::Result<ManagedChild> {
    let mut child = command.spawn()?;
    let stdout = child.stdout.take().expect("capture test command stdout");
    let stderr = child.stderr.take().expect("capture test command stderr");
    Ok(ManagedChild {
        child,
        stdout: spawn_reader(stdout.take(limit)),
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
                "test command exceeded {deadline:?}: {command_label}\nstatus: {}\nstdout: {}\nstderr: {}\n\
                 A test that must wait on production pacing or a whole-second timeout belongs on the \
                 exemption list in .config/nextest.toml.",
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
