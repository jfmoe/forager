//! The browser's process group: launching the browser in a group of its own with the debugging
//! pipe on descriptors 3 and 4, keeping the tail of its output, and waiting for, killing, and
//! reaping it. The group holds the access permit and the profile lock until its leader is
//! reaped.

use std::ffi::OsString;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;

use super::runtime;
use crate::net::{AttemptFailure, MAX_ERROR_BODY_BYTES, truncate_message};
use crate::providers::browser_profile::LockedProfile;
use crate::providers::cdp::{Cdp, CdpError, CdpEvent};
use crate::rate_limit::RatePermit;
use crate::redact::redact_urls;

// Rust cannot hand a child extra file descriptors without `unsafe` code, which this crate
// forbids. A POSIX shell moves the two piped standard streams to the descriptors that
// `--remote-debugging-pipe` reads (3) and writes (4), then replaces itself with the browser,
// which keeps the process ID and group. Its stdout goes to /dev/null: when it joined the stderr
// pipe, a real-browser run left browsers spinning after their driver died.
const PIPE_SHELL: &str = r#"exec "$0" "$@" 3<&0 4>&1 </dev/null >/dev/null"#;

/// Owns a launched browser's process group until its leader is reaped. Dropping it earlier
/// kills the whole group, and keeps the access permit and the profile lock until the leader is
/// gone.
pub(super) struct ProcessGroup {
    child: Option<Child>,
    /// How the leader exited, once it was reaped by waiting for it.
    exit: Option<std::process::ExitStatus>,
    permit: Option<RatePermit>,
    profile: Option<LockedProfile>,
    stderr: Arc<Mutex<Vec<u8>>>,
    stderr_task: Option<JoinHandle<()>>,
}

impl ProcessGroup {
    pub(super) fn spawn(
        executable: &Path,
        arguments: &[OsString],
        permit: RatePermit,
        profile: LockedProfile,
    ) -> Result<Self, AttemptFailure> {
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(PIPE_SHELL)
            .arg(executable)
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command.spawn().map_err(|error| {
            runtime(format!(
                "cannot launch the browser `{}`: {error}",
                executable.display()
            ))
        })?;
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let stderr_task = child
            .stderr
            .take()
            .map(|pipe| tokio::spawn(keep_tail(pipe, Arc::clone(&stderr))));
        Ok(Self {
            child: Some(child),
            exit: None,
            permit: Some(permit),
            profile: Some(profile),
            stderr,
            stderr_task,
        })
    }

    /// Starts the CDP connection over the debugging pipe.
    pub(super) fn connect(
        &mut self,
    ) -> Result<(Cdp, tokio::sync::mpsc::UnboundedReceiver<CdpEvent>), AttemptFailure> {
        let child = self.child.as_mut().expect("the group owns its leader");
        let (Some(commands), Some(answers)) = (child.stdin.take(), child.stdout.take()) else {
            return Err(runtime("cannot open the browser debugging pipe".into()));
        };
        Ok(Cdp::start(answers, commands))
    }

    /// The failure of a handshake that `error` ended: the browser exited, answered garbage, or
    /// never answered. It carries how the browser exited and the last line it wrote to stderr.
    pub(super) async fn handshake_failure(&mut self, error: &CdpError) -> AttemptFailure {
        let exited = self.wait_exit(Duration::from_millis(200)).await;
        if let Some(task) = self.stderr_task.take()
            && exited
        {
            let _ = tokio::time::timeout(Duration::from_millis(200), task).await;
        }
        let line = last_line(&self.stderr.lock().unwrap_or_else(PoisonError::into_inner));
        runtime(format!(
            "the browser did not complete the CDP handshake ({}): {}",
            match (exited, self.exit) {
                (true, Some(status)) => format!("it exited with {status}"),
                (true, None) => "it exited".to_owned(),
                (false, _) => error_summary(error).to_owned(),
            },
            truncate_message(&redact_urls(&line))
        ))
    }

    /// Waits up to `within` for the leader to exit, and reaps it when it does.
    pub(super) async fn wait_exit(&mut self, within: Duration) -> bool {
        let Some(child) = self.child.as_mut() else {
            return true;
        };
        if let Ok(Ok(status)) = tokio::time::timeout(within, child.wait()).await {
            self.child = None;
            self.exit = Some(status);
            // Helpers the browser left behind in its group go with it.
            return true;
        }
        false
    }

    pub(super) async fn kill_and_reap(&mut self) {
        if let Some(child) = &self.child {
            kill_group(child);
        }
        if let Some(child) = self.child.as_mut() {
            let _ = child.wait().await;
        }
        self.child = None;
    }

    /// Releases the permit of a reaped browser and returns its still-locked profile.
    pub(super) fn release(mut self) -> LockedProfile {
        self.permit = None;
        self.profile
            .take()
            .expect("only a group that still holds its profile is released")
    }
}

fn error_summary(error: &CdpError) -> &'static str {
    match error {
        CdpError::Timeout { .. } => "no answer",
        CdpError::Command { .. } => "an error answer",
        CdpError::Closed(_) => "the pipe closed",
    }
}

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        if let Some(task) = self.stderr_task.take() {
            task.abort();
        }
        let Some(mut child) = self.child.take() else {
            return;
        };
        kill_group(&child);
        // A task reaps the killed leader, so the drop never blocks a runtime thread, and holds
        // the permit and the profile lock until then. A runtime that shuts down first means
        // forager is exiting, and the kernel releases both locks with the process.
        let held = (self.permit.take(), self.profile.take());
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ = child.wait().await;
                drop(held);
            });
        }
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

/// Reads everything the browser writes to stderr, so the browser never blocks on a full pipe,
/// and keeps the last bytes of it, where the reason a browser exits comes.
async fn keep_tail(mut pipe: impl AsyncRead + Unpin, kept: Arc<Mutex<Vec<u8>>>) {
    let mut buffer = [0; 8192];
    loop {
        match pipe.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(read) => {
                let mut kept = kept.lock().unwrap_or_else(PoisonError::into_inner);
                kept.extend_from_slice(&buffer[..read]);
                let excess = kept.len().saturating_sub(MAX_ERROR_BODY_BYTES);
                kept.drain(..excess);
            }
        }
    }
}

/// The last non-empty line of the kept stderr.
fn last_line(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .unwrap_or("no error output")
        .to_owned()
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::super::WindowMode;

    #[cfg(unix)]
    #[test]
    fn dropping_the_command_kills_the_browser_group_and_frees_the_profile_once_reaped() {
        use std::os::unix::fs::PermissionsExt;
        use std::time::Duration;

        use super::super::{BrowserSession, CaptureLevel, run};
        use crate::providers::browser_profile::BrowserProfile;
        use crate::rate_limit::{AccessPolicy, RateLimiter};
        use crate::types::Deadline;

        let sandbox = tempfile::tempdir().expect("create sandbox");
        let browser = sandbox.path().join("browser");
        std::fs::write(
            &browser,
            format!(
                "#!/bin/sh\n/bin/sleep 60 &\nprintf '%s\\n' \"$!\" > '{0}/child.pid'\nprintf '%s\\n' \"$$\" > '{0}/leader.pid'\nwait\n",
                sandbox.path().display()
            ),
        )
        .expect("write stand-in");
        std::fs::set_permissions(&browser, std::fs::Permissions::from_mode(0o755))
            .expect("make stand-in executable");
        let profile = BrowserProfile::at(sandbox.path().join("profile"));
        let session = BrowserSession {
            executable: &browser,
            profile: &profile,
            language: "zh-CN",
            window: WindowMode::Headless,
            capture: |_| CaptureLevel::Ignore,
        };
        let limiter = RateLimiter::in_state_file(
            "drop_test",
            AccessPolicy {
                min_interval: Duration::ZERO,
                max_concurrency: 1,
            },
            sandbox.path().join("rate_limit_state.json"),
        );
        let pid_files = ["leader.pid", "child.pid"].map(|name| sandbox.path().join(name));
        let running = |file: &std::path::PathBuf| {
            let pid = std::fs::read_to_string(file).expect("recorded pid");
            let pid = nix::unistd::Pid::from_raw(pid.trim().parse().expect("pid"));
            nix::sys::signal::kill(pid, None).is_ok()
        };

        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("build test runtime")
            .block_on(async {
                let command = Box::pin(run(
                    &session,
                    &limiter,
                    Deadline::new(Duration::from_secs(30)),
                    async |_page| Ok(()),
                ));
                let started = Box::pin(async {
                    while !pid_files.iter().all(|file| file.exists()) {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                });
                let raced = tokio::time::timeout(
                    Duration::from_secs(5),
                    futures_util::future::select(command, started),
                )
                .await;
                let Ok(futures_util::future::Either::Right(((), command))) = raced else {
                    panic!("the stand-in never started");
                };
                drop(command);
                let deadline = std::time::Instant::now() + Duration::from_secs(3);
                while (pid_files.iter().any(running) || profile.lock().is_err())
                    && std::time::Instant::now() < deadline
                {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            });
        let alive = pid_files.each_ref().map(running);

        assert_eq!((alive, profile.lock().is_ok()), ([false, false], true));
    }

    #[cfg(unix)]
    #[test]
    fn a_browser_that_exits_before_the_handshake_reports_its_exit_status_and_last_stderr_line() {
        use std::os::unix::fs::PermissionsExt;
        use std::time::Duration;

        use super::super::{BrowserSession, CaptureLevel, run};
        use crate::providers::browser_profile::BrowserProfile;
        use crate::rate_limit::{AccessPolicy, RateLimiter};
        use crate::types::{AttemptErrorKind, Deadline};

        let sandbox = tempfile::tempdir().expect("create sandbox");
        let browser = sandbox.path().join("browser");
        std::fs::write(
            &browser,
            "#!/bin/sh\necho 'Opening in existing browser session.' >&2\nexit 3\n",
        )
        .expect("write stand-in");
        std::fs::set_permissions(&browser, std::fs::Permissions::from_mode(0o755))
            .expect("make stand-in executable");
        let profile = BrowserProfile::at(sandbox.path().join("profile"));
        let session = BrowserSession {
            executable: &browser,
            profile: &profile,
            language: "zh-CN",
            window: WindowMode::Headless,
            capture: |_| CaptureLevel::Ignore,
        };
        let limiter = RateLimiter::in_state_file(
            "handshake_test",
            AccessPolicy {
                min_interval: Duration::ZERO,
                max_concurrency: 1,
            },
            sandbox.path().join("rate_limit_state.json"),
        );

        let failure = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("build test runtime")
            .block_on(run(
                &session,
                &limiter,
                Deadline::new(Duration::from_secs(30)),
                async |_page| Ok(()),
            ))
            .map(|_| ())
            .expect_err("the stand-in exits before the handshake");

        assert_eq!(
            (failure.kind, failure.message.as_str()),
            (
                AttemptErrorKind::Runtime,
                "the browser did not complete the CDP handshake (it exited with exit status: 3): Opening in existing browser session."
            )
        );
    }

    #[test]
    fn the_last_stderr_line_survives_output_beyond_the_kept_size() {
        use std::sync::{Arc, Mutex};

        use super::{MAX_ERROR_BODY_BYTES, keep_tail, last_line};

        let mut output = "starting up\n".repeat(MAX_ERROR_BODY_BYTES / 4);
        output.push_str("fatal: the profile is in use\n\n");
        let kept = Arc::new(Mutex::new(Vec::new()));

        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("build test runtime")
            .block_on(keep_tail(output.as_bytes(), Arc::clone(&kept)));

        let kept = kept.lock().expect("kept stderr");
        assert_eq!(
            (kept.len() <= MAX_ERROR_BODY_BYTES, last_line(&kept)),
            (true, "fatal: the profile is in use".to_owned())
        );
    }
}
