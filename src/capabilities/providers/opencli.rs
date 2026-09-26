//! OpenCLI process transport: runs one command of a forager-owned OpenCLI adapter and decodes
//! its output envelope. It holds no site knowledge; route adapters choose the command and its
//! arguments, and read the envelope `data`.

use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};

use crate::catalog::OpenCliAdapter;
use crate::net::{AttemptFailure, MAX_ERROR_BODY_BYTES, MAX_RESPONSE_BYTES, truncate_message};
use crate::providers::shared::acquire_window;
use crate::rate_limit::{RateLimiter, RatePermit};
use crate::redact::redact_urls;
use crate::types::{AttemptErrorKind, Deadline};

// OpenCLI gets the attempt budget minus this reserve as its own command timeout, so it can close
// its tab before forager kills the process group and reports the attempt.
const CLEANUP_RESERVE: Duration = Duration::from_secs(5);
const REAP_POLLS: u32 = 1000;
const REAP_POLL_INTERVAL: Duration = Duration::from_millis(1);

// Every forager-owned adapter command accepts these flags: JSON output, a background window, a
// site session that ends with the command, and no tab kept after it.
const SESSION_FLAGS: [&str; 8] = [
    "-f",
    "json",
    "--window",
    "background",
    "--site-session",
    "ephemeral",
    "--keep-tab",
    "false",
];

/// One command of a forager-owned OpenCLI adapter.
pub(crate) struct OpenCliCommand<'a> {
    /// The configured OpenCLI executable.
    pub(crate) executable: &'a str,
    pub(crate) adapter: OpenCliAdapter,
    pub(crate) command: &'static str,
    /// Named options, passed as `--<name> <value>`.
    pub(crate) options: Vec<(&'static str, String)>,
}

/// Returns whether this host can run process routes: they need Unix process groups to stop
/// every process an OpenCLI command starts.
pub(crate) fn host_support() -> Result<(), String> {
    if cfg!(unix) {
        Ok(())
    } else {
        Err("OpenCLI process routes need Unix process groups".into())
    }
}

/// Runs the command and decodes its envelope within `deadline`, the attempt deadline.
///
/// The access permit is held until the process is reaped. When the command outlives its
/// working deadline, or the returned future is dropped, the whole process group is killed.
///
/// # Errors
///
/// Returns an attempt failure for a pacing failure, a missing executable, a nonzero exit, output
/// over its limit, an envelope that does not match the adapter contract, or a missed deadline.
pub(crate) async fn run<T: DeserializeOwned>(
    command: &OpenCliCommand<'_>,
    limiter: &RateLimiter,
    deadline: Deadline,
) -> Result<Envelope<T>, AttemptFailure> {
    host_support().map_err(runtime)?;
    let working_deadline = deadline
        .remaining()
        .and_then(|remaining| remaining.checked_sub(CLEANUP_RESERVE))
        .filter(|working| working >= &Duration::from_secs(1))
        .map(Deadline::new)
        .ok_or_else(|| AttemptFailure {
            kind: AttemptErrorKind::Timeout,
            status: None,
            message: "no time is left to run an OpenCLI command".into(),
        })?;
    let permit = acquire_window(limiter, working_deadline).await?;
    let Some(working) = working_deadline.remaining() else {
        return Err(timeout_failure());
    };
    let mut group = ProcessGroup::spawn(command, working.as_secs().max(1), permit)?;
    let Ok(output) = tokio::time::timeout(working, group.collect()).await else {
        group.kill_and_reap().await;
        return Err(timeout_failure());
    };
    let output = output?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(exit_failure(output.status.code(), &stderr, command.adapter));
    }
    decode_envelope(&output.stdout, command.adapter, command.command)
}

/// Runs the adapter's `contract` command, which needs no browser, and checks that the installed
/// adapter answers the contract version forager expects.
///
/// # Errors
///
/// Returns the failure message, with the install hint when the adapter is missing or outdated.
pub(crate) async fn check_contract(
    executable: &str,
    adapter: OpenCliAdapter,
    limiter: &RateLimiter,
    deadline: Deadline,
) -> Result<(), String> {
    let command = OpenCliCommand {
        executable,
        adapter,
        command: "contract",
        options: Vec::new(),
    };
    run::<serde_json::Value>(&command, limiter, deadline)
        .await
        .map(|_| ())
        .map_err(|failure| failure.message)
}

fn timeout_failure() -> AttemptFailure {
    AttemptFailure {
        kind: AttemptErrorKind::Timeout,
        status: None,
        message: "OpenCLI command did not finish before its deadline".into(),
    }
}

struct ProcessOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

/// Owns a spawned OpenCLI process group until its leader is reaped. Dropping it earlier kills
/// the whole group and keeps the access permit until a background task reaps the leader.
struct ProcessGroup {
    child: Option<Child>,
    permit: Option<RatePermit>,
}

impl ProcessGroup {
    fn spawn(
        command: &OpenCliCommand<'_>,
        timeout_seconds: u64,
        permit: RatePermit,
    ) -> Result<Self, AttemptFailure> {
        let mut process = Command::new(command.executable);
        process
            .arg(command.adapter.site)
            .arg(command.command)
            .args(
                command
                    .options
                    .iter()
                    .flat_map(|(name, value)| [format!("--{name}"), value.clone()]),
            )
            .args(["--timeout", &timeout_seconds.to_string()])
            .args(SESSION_FLAGS)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        process.process_group(0);
        let child = process.spawn().map_err(|error| {
            runtime(format!(
                "cannot run OpenCLI executable `{}`: {error}; install OpenCLI or set the route `command`",
                command.executable
            ))
        })?;
        Ok(Self {
            child: Some(child),
            permit: Some(permit),
        })
    }

    /// Reads both pipes to their end and reaps the leader. Stdout over the protocol limit kills
    /// the group.
    async fn collect(&mut self) -> Result<ProcessOutput, AttemptFailure> {
        let child = self.child.as_mut().expect("the group owns its leader");
        let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
            return Err(runtime("cannot capture OpenCLI output".into()));
        };
        let pipes = futures_util::try_join!(read_stdout(stdout), async {
            Ok(read_prefix(stderr, MAX_ERROR_BODY_BYTES).await)
        });
        let (stdout, stderr) = match pipes {
            Ok(pipes) => pipes,
            Err(failure) => {
                self.kill_and_reap().await;
                return Err(failure);
            }
        };
        let status = self.reap().await?;
        Ok(ProcessOutput {
            status,
            stdout,
            stderr,
        })
    }

    async fn reap(&mut self) -> Result<ExitStatus, AttemptFailure> {
        let child = self.child.as_mut().expect("the group owns its leader");
        let status = child
            .wait()
            .await
            .map_err(|error| runtime(format!("cannot wait for OpenCLI: {error}")))?;
        self.child = None;
        self.permit = None;
        Ok(status)
    }

    async fn kill_and_reap(&mut self) {
        if let Some(child) = &self.child {
            kill_group(child);
        }
        let _ = self.reap().await;
    }
}

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        kill_group(&child);
        // A killed process exits at once, so a short blocking wait reaps it here and the permit
        // outlives the process even when the runtime shuts down right after this drop.
        for _ in 0..REAP_POLLS {
            if !matches!(child.try_wait(), Ok(None)) {
                break;
            }
            std::thread::sleep(REAP_POLL_INTERVAL);
        }
        self.permit = None;
    }
}

/// Kills the process group that `child` leads. The leader is not reaped yet, so its process ID
/// still names the group.
#[cfg(unix)]
fn kill_group(child: &Child) {
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;

    if let Some(id) = child.id().and_then(|id| i32::try_from(id).ok()) {
        let _ = killpg(Pid::from_raw(id), Signal::SIGKILL);
    }
}

#[cfg(not(unix))]
fn kill_group(_child: &Child) {}

async fn read_stdout(reader: impl AsyncRead + Unpin) -> Result<Vec<u8>, AttemptFailure> {
    let mut bytes = Vec::new();
    let read = reader
        .take(MAX_RESPONSE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .await;
    match read {
        Ok(_) if bytes.len() > MAX_RESPONSE_BYTES => Err(runtime(
            "OpenCLI output exceeded the 4 MiB protocol limit".into(),
        )),
        Ok(_) => Ok(bytes),
        Err(error) => Err(runtime(format!("cannot read OpenCLI output: {error}"))),
    }
}

/// Keeps the first `limit` bytes and drains the rest, so the process never blocks on a full
/// pipe.
async fn read_prefix(mut reader: impl AsyncRead + Unpin, limit: usize) -> Vec<u8> {
    let mut kept = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) | Err(_) => return kept,
            Ok(read) => {
                let room = limit.saturating_sub(kept.len());
                kept.extend_from_slice(&buffer[..read.min(room)]);
            }
        }
    }
}

/// Whether the adapter confirmed a result or the site's own empty-result notice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EnvelopeStatus {
    Ok,
    NoResults,
}

/// The decoded output of an adapter command.
#[derive(Debug)]
pub(crate) struct Envelope<T> {
    pub(crate) status: EnvelopeStatus,
    pub(crate) data: T,
}

#[derive(Deserialize)]
struct RawEnvelope {
    contract: Option<String>,
    status: Option<String>,
    data: Option<serde_json::Value>,
}

fn runtime(message: String) -> AttemptFailure {
    AttemptFailure {
        kind: AttemptErrorKind::Runtime,
        status: None,
        message,
    }
}

fn install_hint(adapter: OpenCliAdapter) -> String {
    format!(
        "install or update the forager OpenCLI adapter: copy the `opencli/{site}` directory of the forager skill to `~/.opencli/clis/{site}` (see the skill platform reference)",
        site = adapter.site
    )
}

/// Decodes the stdout of a command that exited with 0.
fn decode_envelope<T: DeserializeOwned>(
    stdout: &[u8],
    adapter: OpenCliAdapter,
    command: &str,
) -> Result<Envelope<T>, AttemptFailure> {
    let label = format!("{} {command}", adapter.site);
    let raw = serde_json::from_slice::<RawEnvelope>(stdout).map_err(|error| {
        runtime(format!(
            "OpenCLI `{label}` printed no JSON envelope: {error}"
        ))
    })?;
    let contract = raw.contract.as_deref().unwrap_or("none");
    if contract != adapter.contract {
        return Err(runtime(format!(
            "OpenCLI `{label}` answers contract `{contract}`, but forager expects `{}`; {}",
            adapter.contract,
            install_hint(adapter)
        )));
    }
    let status = match raw.status.as_deref() {
        Some("ok") => EnvelopeStatus::Ok,
        Some("no_results") => EnvelopeStatus::NoResults,
        other => {
            return Err(runtime(format!(
                "OpenCLI `{label}` returned unknown envelope status `{}`",
                other.unwrap_or("none")
            )));
        }
    };
    let data = serde_json::from_value(raw.data.unwrap_or_default())
        .map_err(|error| runtime(format!("OpenCLI `{label}` returned invalid data: {error}")))?;
    Ok(Envelope { status, data })
}

/// The `code` and `message` of the YAML error envelope OpenCLI writes to stderr, or the first
/// line of other stderr output.
struct ErrorReport {
    code: Option<String>,
    message: String,
}

fn error_report(stderr: &str) -> ErrorReport {
    let field = |name: &str| {
        stderr.lines().find_map(|line| {
            line.trim_start()
                .strip_prefix(name)
                .map(|value| value.trim().trim_matches(['\'', '"']).to_owned())
        })
    };
    let message = field("message:").unwrap_or_else(|| {
        stderr
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("no error output")
            .to_owned()
    });
    ErrorReport {
        code: field("code:"),
        message: truncate_message(&redact_urls(&message)),
    }
}

/// Maps a nonzero OpenCLI exit to an attempt failure. Exit 66 (`EMPTY_RESULT`) is never a
/// legitimate empty result: only the envelope's `no_results` status is.
fn exit_failure(code: Option<i32>, stderr: &str, adapter: OpenCliAdapter) -> AttemptFailure {
    let report = error_report(stderr);
    let loads_no_adapter = report.code.as_deref() == Some("ADAPTER_LOAD")
        || report.message.contains("unknown command");
    let kind = match code {
        Some(69) if loads_no_adapter => AttemptErrorKind::Runtime,
        Some(69) => AttemptErrorKind::Network,
        Some(75) => AttemptErrorKind::Timeout,
        Some(77) => AttemptErrorKind::Auth,
        _ => AttemptErrorKind::Runtime,
    };
    let code = code.map_or_else(|| "a signal".to_owned(), |code| code.to_string());
    let mut message = format!("OpenCLI exited with {code}: {}", report.message);
    if loads_no_adapter {
        message = format!("{message}; {}", install_hint(adapter));
    }
    AttemptFailure {
        kind,
        status: None,
        message,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{EnvelopeStatus, decode_envelope, exit_failure};
    use crate::catalog::OpenCliAdapter;
    use crate::types::AttemptErrorKind;

    const ADAPTER: OpenCliAdapter = OpenCliAdapter {
        site: "ssrn",
        contract: "forager-ssrn/1",
    };

    fn decode(value: &Value) -> Result<(EnvelopeStatus, Value), (AttemptErrorKind, String)> {
        decode_envelope::<Value>(value.to_string().as_bytes(), ADAPTER, "search")
            .map(|envelope| (envelope.status, envelope.data))
            .map_err(|failure| (failure.kind, failure.message))
    }

    mod decode_envelope {
        use super::*;

        #[test]
        fn returns_the_status_and_data_of_a_matching_contract() {
            let result = decode(&json!({
                "contract": "forager-ssrn/1",
                "status": "no_results",
                "data": {"url": "https://example.test"}
            }));

            assert_eq!(
                result,
                Ok((
                    EnvelopeStatus::NoResults,
                    json!({"url": "https://example.test"})
                ))
            );
        }

        #[test]
        fn rejects_another_contract_version_with_a_reinstall_hint() {
            let (kind, message) = decode(&json!({
                "contract": "forager-ssrn/2",
                "status": "ok",
                "data": {}
            }))
            .unwrap_err();

            assert_eq!(
                (
                    kind,
                    message.contains("forager-ssrn/2"),
                    message.contains("install or update")
                ),
                (AttemptErrorKind::Runtime, true, true)
            );
        }

        #[test]
        fn rejects_output_without_a_contract() {
            let (kind, message) = decode(&json!({"name": "opencli", "commands": []})).unwrap_err();

            assert_eq!(
                (kind, message.contains("contract `none`")),
                (AttemptErrorKind::Runtime, true)
            );
        }

        #[test]
        fn rejects_an_unknown_status() {
            let result = decode(&json!({
                "contract": "forager-ssrn/1",
                "status": "partial",
                "data": {}
            }));

            assert_eq!(
                result.map_err(|(kind, _)| kind),
                Err(AttemptErrorKind::Runtime)
            );
        }

        #[test]
        fn rejects_stdout_that_is_not_json() {
            let result = decode_envelope::<Value>(b"items: []", ADAPTER, "search");

            assert_eq!(
                result.map(|_| ()).map_err(|failure| failure.kind),
                Err(AttemptErrorKind::Runtime)
            );
        }

        #[test]
        fn rejects_data_of_another_shape() {
            let result = decode_envelope::<Vec<String>>(
                br#"{"contract":"forager-ssrn/1","status":"ok","data":{"items":[]}}"#,
                ADAPTER,
                "search",
            );

            assert_eq!(
                result.map(|_| ()).map_err(|failure| failure.kind),
                Err(AttemptErrorKind::Runtime)
            );
        }
    }

    mod exit_failure {
        use super::*;

        fn envelope(code: &str, message: &str) -> String {
            format!("ok: false\nerror:\n  code: {code}\n  message: {message}\n  exitCode: 1\n")
        }

        #[test]
        fn maps_each_exit_code_to_its_error_kind() {
            let kinds = [
                (
                    Some(66),
                    envelope("EMPTY_RESULT", "ssrn/search returned no data"),
                ),
                (Some(69), envelope("BROWSER_CONNECT", "daemon unavailable")),
                (
                    Some(69),
                    envelope("ADAPTER_LOAD", "cannot load ssrn/search"),
                ),
                (Some(75), envelope("TIMEOUT", "timed out")),
                (
                    Some(77),
                    envelope("AUTH_REQUIRED", "verification did not clear"),
                ),
                (Some(77), envelope("LOGIN_WALL", "login wall")),
                (Some(1), envelope("COMMAND_EXEC", "boom")),
                (Some(2), "error: unknown option '--window'\n".to_owned()),
                (None, String::new()),
            ]
            .map(|(code, stderr)| exit_failure(code, &stderr, ADAPTER).kind);

            assert_eq!(
                kinds,
                [
                    AttemptErrorKind::Runtime,
                    AttemptErrorKind::Network,
                    AttemptErrorKind::Runtime,
                    AttemptErrorKind::Timeout,
                    AttemptErrorKind::Auth,
                    AttemptErrorKind::Auth,
                    AttemptErrorKind::Runtime,
                    AttemptErrorKind::Runtime,
                    AttemptErrorKind::Runtime,
                ]
            );
        }

        #[test]
        fn an_adapter_load_failure_carries_the_install_hint() {
            let failure = exit_failure(
                Some(69),
                &envelope("ADAPTER_LOAD", "cannot load ssrn/search"),
                ADAPTER,
            );

            assert_eq!(
                failure.message,
                "OpenCLI exited with 69: cannot load ssrn/search; install or update the forager OpenCLI adapter: copy the `opencli/ssrn` directory of the forager skill to `~/.opencli/clis/ssrn` (see the skill platform reference)"
            );
        }

        #[test]
        fn a_missing_site_carries_the_install_hint() {
            let failure = exit_failure(Some(2), "error: unknown command 'ssrn'\n", ADAPTER);

            assert!(
                failure.message.contains("install or update"),
                "{}",
                failure.message
            );
        }

        #[test]
        fn urls_in_the_error_message_are_redacted() {
            let failure = exit_failure(
                Some(1),
                &envelope(
                    "COMMAND_EXEC",
                    "failed at https://example.test/file.pdf?token=secret",
                ),
                ADAPTER,
            );

            assert!(!failure.message.contains("secret"), "{}", failure.message);
        }
    }
}
