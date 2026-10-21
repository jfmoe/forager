//! OpenCLI process transport: runs one command of a forager-owned OpenCLI adapter and decodes
//! the OpenCLI 2.x output around its envelope. It holds no site knowledge; route adapters choose
//! the command and its arguments, and read the envelope `data`.

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
/// The OpenCLI exit code of a command line it rejected before running the command.
const USAGE_ERROR: i32 = 2;

/// The Chrome window an adapter command runs in. A background window keeps the user's screen
/// free, but a hidden page may not render menus, so each command declares the window it needs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Window {
    Background,
    Foreground,
}

impl Window {
    const fn flag(self) -> &'static str {
        match self {
            Self::Background => "background",
            Self::Foreground => "foreground",
        }
    }
}

/// One command of a forager-owned OpenCLI adapter.
pub(crate) struct OpenCliCommand<'a> {
    /// The configured OpenCLI executable.
    pub(crate) executable: &'a str,
    pub(crate) adapter: OpenCliAdapter,
    pub(crate) command: &'static str,
    /// Named options, passed as `--<name> <value>`.
    pub(crate) options: Vec<(&'static str, String)>,
    pub(crate) window: Window,
}

impl OpenCliCommand<'_> {
    // Every forager-owned adapter command accepts these flags: JSON output, the declared window,
    // a site session that ends with the command, and no tab kept after it.
    fn session_flags(&self) -> [&'static str; 8] {
        [
            "-f",
            "json",
            "--window",
            self.window.flag(),
            "--site-session",
            "ephemeral",
            "--keep-tab",
            "false",
        ]
    }
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

/// A failed adapter command, and whether its page operations may have run.
#[derive(Debug)]
pub(crate) struct CommandFailure {
    pub(crate) attempt: AttemptFailure,
    /// False only when forager knows the adapter command never ran: OpenCLI was not started, or
    /// it rejected the command line or could not load the adapter command. Any other failure,
    /// including a browser connection lost mid-command, may follow page operations.
    pub(crate) may_have_run: bool,
}

impl CommandFailure {
    fn not_run(attempt: AttemptFailure) -> Self {
        Self {
            attempt,
            may_have_run: false,
        }
    }
}

impl From<AttemptFailure> for CommandFailure {
    fn from(attempt: AttemptFailure) -> Self {
        Self {
            attempt,
            may_have_run: true,
        }
    }
}

impl From<CommandFailure> for AttemptFailure {
    fn from(failure: CommandFailure) -> Self {
        failure.attempt
    }
}

/// Runs the command and decodes its envelope within `deadline`, the attempt deadline.
///
/// The access permit is held until the process is reaped. When the command outlives its
/// working deadline, or the returned future is dropped, the whole process group is killed.
///
/// # Errors
///
/// Returns a command failure for a pacing failure, a missing executable, a nonzero exit, output
/// over its limit, an envelope that does not match the adapter contract, or a missed deadline.
pub(crate) async fn run<T: DeserializeOwned>(
    command: &OpenCliCommand<'_>,
    limiter: &RateLimiter,
    deadline: Deadline,
) -> Result<Envelope<T>, CommandFailure> {
    host_support()
        .map_err(runtime)
        .map_err(CommandFailure::not_run)?;
    let working_deadline = deadline
        .remaining()
        .and_then(|remaining| remaining.checked_sub(CLEANUP_RESERVE))
        .filter(|working| working >= &Duration::from_secs(1))
        .map(Deadline::new)
        .ok_or_else(|| {
            CommandFailure::not_run(AttemptFailure {
                kind: AttemptErrorKind::Timeout,
                status: None,
                message: "no time is left to run an OpenCLI command".into(),
            })
        })?;
    let permit = acquire_window(limiter, working_deadline)
        .await
        .map_err(CommandFailure::not_run)?;
    let Some(working) = working_deadline.remaining() else {
        return Err(CommandFailure::not_run(timeout_failure()));
    };
    let mut group = ProcessGroup::spawn(command, working.as_secs().max(1), permit)
        .map_err(CommandFailure::not_run)?;
    let Ok(output) = tokio::time::timeout(working, group.collect()).await else {
        group.kill_and_reap().await;
        return Err(timeout_failure().into());
    };
    let output = output?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(exit_failure(
            output.status.code(),
            &output.stdout,
            &stderr,
            command.adapter,
        ));
    }
    decode_success(&output.stdout, command.adapter, command.command).map_err(CommandFailure::from)
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
        window: Window::Background,
    };
    run::<serde_json::Value>(&command, limiter, deadline)
        .await
        .map(|_| ())
        .map_err(|failure| failure.attempt.message)
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
            .args(command.session_flags())
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

/// The output envelope of a forager-owned adapter command.
#[derive(Deserialize)]
struct RawEnvelope {
    contract: Option<String>,
    status: Option<String>,
    data: Option<serde_json::Value>,
}

/// The stdout of an OpenCLI 2.x command that exited with 0. OpenCLI 1.x printed the adapter
/// return value bare, so its output has no `ok`.
#[derive(Deserialize)]
struct SuccessOutput {
    ok: Option<bool>,
    data: Option<serde_json::Value>,
}

/// The stdout of an OpenCLI 2.x command that failed.
#[derive(Deserialize)]
struct FailureOutput {
    ok: bool,
    error: ReportedError,
}

#[derive(Deserialize)]
struct ReportedError {
    code: String,
    message: String,
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
        "install or update the forager OpenCLI adapter: copy the `opencli/{site}` directory of the forager skill to `~/.opencli/clis/{site}` (see the forager skill references)",
        site = adapter.site
    )
}

const UPGRADE_HINT: &str = "forager needs OpenCLI 2.x and Browser Bridge 1.0.25 or later; upgrade OpenCLI and its Chrome extension (see the forager skill references)";

/// Decodes the stdout of a command that exited with 0.
fn decode_success<T: DeserializeOwned>(
    stdout: &[u8],
    adapter: OpenCliAdapter,
    command: &str,
) -> Result<Envelope<T>, AttemptFailure> {
    let label = format!("{} {command}", adapter.site);
    let output = serde_json::from_slice::<SuccessOutput>(stdout)
        .map_err(|error| runtime(format!("OpenCLI `{label}` printed no JSON result: {error}")))?;
    match output.ok {
        Some(true) => decode_envelope(output.data.unwrap_or_default(), adapter, &label),
        Some(false) => Err(runtime(format!(
            "OpenCLI `{label}` exited with 0 but reported a failure"
        ))),
        None => Err(runtime(format!(
            "OpenCLI `{label}` printed OpenCLI 1.x output; {UPGRADE_HINT}"
        ))),
    }
}

/// Decodes the adapter envelope that OpenCLI returned as the command result.
fn decode_envelope<T: DeserializeOwned>(
    data: serde_json::Value,
    adapter: OpenCliAdapter,
    label: &str,
) -> Result<Envelope<T>, AttemptFailure> {
    let raw = serde_json::from_value::<RawEnvelope>(data).map_err(|error| {
        runtime(format!(
            "OpenCLI `{label}` returned no adapter envelope: {error}"
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

/// Whether an OpenCLI error code says that OpenCLI has no such adapter command: the adapter
/// failed to load, its site is unknown, or an outdated copy lacks the command or an option.
fn names_missing_adapter(code: &str) -> bool {
    matches!(code, "ADAPTER_LOAD" | "unknown_command") || code.starts_with("commander.")
}

/// Whether an OpenCLI error code says that OpenCLI rejected the command before running it.
fn names_rejected_command(code: &str) -> bool {
    names_missing_adapter(code) || matches!(code, "usage_error" | "ARGUMENT")
}

/// Whether failed output is the YAML error envelope that OpenCLI 1.x wrote to stderr.
fn is_legacy_failure(stdout: &[u8], stderr: &str) -> bool {
    stdout.trim_ascii().is_empty() && stderr.lines().any(|line| line.trim() == "ok: false")
}

/// Maps a nonzero OpenCLI exit to a command failure, read from the error envelope on stdout or
/// else the first line of stderr. Exit 66 (`EMPTY_RESULT`) is never a legitimate empty result:
/// only the adapter envelope's `no_results` status is.
fn exit_failure(
    code: Option<i32>,
    stdout: &[u8],
    stderr: &str,
    adapter: OpenCliAdapter,
) -> CommandFailure {
    let exit = code.map_or_else(|| "a signal".to_owned(), |code| code.to_string());
    let rejected = code == Some(USAGE_ERROR);
    let (error_code, message) = match serde_json::from_slice::<FailureOutput>(stdout) {
        Ok(FailureOutput { ok: false, error }) => (Some(error.code), error.message),
        _ if is_legacy_failure(stdout, stderr) => {
            return CommandFailure {
                attempt: runtime(format!(
                    "OpenCLI exited with {exit} and printed an OpenCLI 1.x error; {UPGRADE_HINT}"
                )),
                may_have_run: !rejected,
            };
        }
        _ => {
            let line = stderr.lines().map(str::trim).find(|line| !line.is_empty());
            (None, line.unwrap_or("no error output").to_owned())
        }
    };
    let missing_adapter = error_code.as_deref().is_some_and(names_missing_adapter);
    let kind = match code {
        Some(69) if missing_adapter => AttemptErrorKind::Runtime,
        Some(69) => AttemptErrorKind::Network,
        Some(75) => AttemptErrorKind::Timeout,
        Some(77) => AttemptErrorKind::Auth,
        _ => AttemptErrorKind::Runtime,
    };
    let may_have_run = !rejected && !error_code.as_deref().is_some_and(names_rejected_command);
    let mut message = format!(
        "OpenCLI exited with {exit}: {}",
        truncate_message(&redact_urls(&message))
    );
    if missing_adapter {
        message = format!("{message}; {}", install_hint(adapter));
    }
    CommandFailure {
        attempt: AttemptFailure {
            kind,
            status: None,
            message,
        },
        may_have_run,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{EnvelopeStatus, decode_success, exit_failure};
    use crate::catalog::OpenCliAdapter;
    use crate::types::AttemptErrorKind;

    const ADAPTER: OpenCliAdapter = OpenCliAdapter {
        site: "ssrn",
        contract: "forager-ssrn/3",
    };

    mod decode_success {
        use super::*;

        fn decode(stdout: &Value) -> Result<(EnvelopeStatus, Value), (AttemptErrorKind, String)> {
            decode_success::<Value>(stdout.to_string().as_bytes(), ADAPTER, "search")
                .map(|envelope| (envelope.status, envelope.data))
                .map_err(|failure| (failure.kind, failure.message))
        }

        fn result(envelope: &Value) -> Value {
            json!({"ok": true, "data": envelope})
        }

        #[test]
        fn returns_the_status_and_data_of_a_matching_contract() {
            let decoded = decode(&result(&json!({
                "contract": "forager-ssrn/3",
                "status": "no_results",
                "data": {"url": "https://example.test"}
            })));

            assert_eq!(
                decoded,
                Ok((
                    EnvelopeStatus::NoResults,
                    json!({"url": "https://example.test"})
                ))
            );
        }

        #[test]
        fn rejects_another_contract_version_with_a_reinstall_hint() {
            let (kind, message) = decode(&result(&json!({
                "contract": "forager-ssrn/2",
                "status": "ok",
                "data": {}
            })))
            .unwrap_err();

            assert_eq!(
                (
                    kind,
                    message.contains("forager-ssrn/3"),
                    message.contains("install or update")
                ),
                (AttemptErrorKind::Runtime, true, true)
            );
        }

        #[test]
        fn rejects_a_result_without_a_contract() {
            let (kind, message) =
                decode(&result(&json!({"name": "opencli", "commands": []}))).unwrap_err();

            assert_eq!(
                (kind, message.contains("contract `none`")),
                (AttemptErrorKind::Runtime, true)
            );
        }

        #[test]
        fn rejects_a_null_result() {
            let decoded = decode(&json!({"ok": true, "data": null}));

            assert_eq!(
                decoded.map_err(|(kind, _)| kind),
                Err(AttemptErrorKind::Runtime)
            );
        }

        #[test]
        fn rejects_an_unknown_status() {
            let decoded = decode(&result(&json!({
                "contract": "forager-ssrn/3",
                "status": "partial",
                "data": {}
            })));

            assert_eq!(
                decoded.map_err(|(kind, _)| kind),
                Err(AttemptErrorKind::Runtime)
            );
        }

        #[test]
        fn rejects_stdout_that_is_not_json() {
            let decoded = decode_success::<Value>(b"items: []", ADAPTER, "search");

            assert_eq!(
                decoded.map(|_| ()).map_err(|failure| failure.kind),
                Err(AttemptErrorKind::Runtime)
            );
        }

        #[test]
        fn rejects_data_of_another_shape() {
            let decoded = decode_success::<Vec<String>>(
                br#"{"ok":true,"data":{"contract":"forager-ssrn/3","status":"ok","data":{"items":[]}}}"#,
                ADAPTER,
                "search",
            );

            assert_eq!(
                decoded.map(|_| ()).map_err(|failure| failure.kind),
                Err(AttemptErrorKind::Runtime)
            );
        }

        #[test]
        fn a_bare_adapter_envelope_asks_to_upgrade_from_opencli_1() {
            let (kind, message) = decode(&json!({
                "contract": "forager-ssrn/3",
                "status": "ok",
                "data": {}
            }))
            .unwrap_err();

            assert_eq!(
                (kind, message.as_str()),
                (
                    AttemptErrorKind::Runtime,
                    "OpenCLI `ssrn search` printed OpenCLI 1.x output; forager needs OpenCLI 2.x and Browser Bridge 1.0.25 or later; upgrade OpenCLI and its Chrome extension (see the forager skill references)"
                )
            );
        }
    }

    mod exit_failure {
        use super::*;

        fn envelope(code: &str, message: &str) -> String {
            json!({"ok": false, "error": {"code": code, "message": message, "exitCode": 1}})
                .to_string()
        }

        fn fail(code: Option<i32>, stdout: &str) -> super::super::CommandFailure {
            exit_failure(code, stdout.as_bytes(), "", ADAPTER)
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
                (Some(2), envelope("ARGUMENT", "--window must be one of")),
                (None, String::new()),
            ]
            .map(|(code, stdout)| fail(code, &stdout).attempt.kind);

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
            let failure = fail(
                Some(69),
                &envelope("ADAPTER_LOAD", "cannot load ssrn/search"),
            );

            assert_eq!(
                failure.attempt.message,
                "OpenCLI exited with 69: cannot load ssrn/search; install or update the forager OpenCLI adapter: copy the `opencli/ssrn` directory of the forager skill to `~/.opencli/clis/ssrn` (see the forager skill references)"
            );
        }

        #[test]
        fn an_unknown_site_or_command_carries_the_install_hint() {
            let hinted = [
                envelope("unknown_command", "Unknown command: ssrn"),
                envelope(
                    "commander.unknownCommand",
                    "error: unknown command 'search'",
                ),
            ]
            .map(|stdout| {
                fail(Some(2), &stdout)
                    .attempt
                    .message
                    .contains("install or update")
            });

            assert_eq!(hinted, [true, true]);
        }

        #[test]
        fn urls_in_the_error_message_are_redacted() {
            let failure = fail(
                Some(1),
                &envelope(
                    "COMMAND_EXEC",
                    "failed at https://example.test/file.pdf?token=secret",
                ),
            );

            assert!(
                !failure.attempt.message.contains("secret"),
                "{}",
                failure.attempt.message
            );
        }

        #[test]
        fn output_without_an_error_envelope_reports_the_first_stderr_line() {
            let failure = exit_failure(None, b"", "\n  Killed by signal 9\nmore detail\n", ADAPTER);

            assert_eq!(
                failure.attempt.message,
                "OpenCLI exited with a signal: Killed by signal 9"
            );
        }

        #[test]
        fn a_yaml_error_on_stderr_asks_to_upgrade_from_opencli_1() {
            let failure = exit_failure(
                Some(77),
                b"",
                "ok: false\nerror:\n  code: AUTH_REQUIRED\n  message: sign in\n  exitCode: 77\n",
                ADAPTER,
            );

            assert_eq!(
                (failure.attempt.kind, failure.attempt.message.as_str()),
                (
                    AttemptErrorKind::Runtime,
                    "OpenCLI exited with 77 and printed an OpenCLI 1.x error; forager needs OpenCLI 2.x and Browser Bridge 1.0.25 or later; upgrade OpenCLI and its Chrome extension (see the forager skill references)"
                )
            );
        }

        #[test]
        fn only_a_rejected_command_line_or_a_missing_adapter_never_ran_the_command() {
            let outcome_unknown = json!({"ok": false, "error": {
                "code": "command_result_unknown",
                "message": "lost",
                "exitCode": 1,
                "outcome": "unknown"
            }})
            .to_string();
            let ran = [
                (
                    Some(69),
                    envelope("ADAPTER_LOAD", "cannot load ssrn/search"),
                ),
                (
                    Some(2),
                    envelope("unknown_command", "Unknown command: ssrn"),
                ),
                (
                    Some(2),
                    envelope("commander.unknownOption", "unknown option"),
                ),
                (Some(2), envelope("usage_error", "bad usage")),
                (Some(1), envelope("usage_error", "bad usage")),
                (Some(1), envelope("ARGUMENT", "bad --timeout")),
                (Some(2), String::new()),
                (Some(69), envelope("BROWSER_CONNECT", "daemon unavailable")),
                (Some(75), envelope("TIMEOUT", "timed out")),
                (Some(1), envelope("COMMAND_EXEC", "boom")),
                (Some(1), outcome_unknown),
                (None, String::new()),
            ]
            .map(|(code, stdout)| fail(code, &stdout).may_have_run);

            assert_eq!(
                ran,
                [
                    false, false, false, false, false, false, false, true, true, true, true, true
                ]
            );
        }
    }
}
