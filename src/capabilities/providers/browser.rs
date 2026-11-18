//! The browser transport: one browser process per forager browser command, driven over CDP.
//!
//! It locates nothing and knows no site. A caller gives it the executable, the Browser Profile,
//! the page language, and whether a window shows; the transport takes the route's access permit,
//! then the profile lock, launches the browser in its own process group with that profile's
//! fingerprint, connects over the debugging pipe, and hands the caller's page logic one page.
//! Page logic reads page state with read-only scripts, changes the page only through native
//! input (`browser_input`), and reads what the page itself requested and received through
//! passive capture (`browser_capture`).
//!
//! Deadlines, in order: the working deadline is the attempt deadline less 5 seconds; the read
//! deadline is 3 seconds before it, when page logic stops waiting and sends no more input; the
//! browser is then closed normally, within its own short limit, so the profile reaches disk; a
//! browser still running at the working deadline, or whose command future is dropped, is killed
//! with its whole process group. The access permit and the profile lock are released only after
//! the browser process is reaped.
//!
//! When forager itself dies (Ctrl-C, SIGTERM, SIGKILL), the kernel closes forager's end of the
//! debugging pipe and the browser exits on its own, so no signal handling is needed.

use std::ffi::OsString;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;

use crate::net::{AttemptFailure, MAX_ERROR_BODY_BYTES, truncate_message};
use crate::providers::browser_profile::{BrowserProfile, LockedProfile, ProfileError, Screen};
use crate::providers::cdp::{Cdp, CdpError};
use crate::providers::shared::acquire_window;
use crate::rate_limit::{RateLimiter, RatePermit};
use crate::redact::redact_urls;
use crate::types::{AttemptErrorKind, Deadline};

#[path = "browser_capture.rs"]
mod capture;
#[path = "browser_input.rs"]
mod input;
#[cfg(all(test, unix))]
#[path = "browser_real_tests.rs"]
mod real_tests;

pub(crate) use capture::{CaptureFilter, CaptureLevel, Exchange, RequestMethod, ResponseBody};
pub(crate) use input::{InputError, InputFailure, Key};

const CLEANUP_RESERVE: Duration = Duration::from_secs(5);
const READ_RESERVE: Duration = Duration::from_secs(3);
const CLOSE_LIMIT: Duration = Duration::from_secs(3);
const HANDSHAKE_LIMIT: Duration = Duration::from_secs(20);
const READY_POLL: Duration = Duration::from_millis(250);
const REAP_POLLS: u32 = 1000;
const REAP_POLL_INTERVAL: Duration = Duration::from_millis(1);

// Rust cannot hand a child extra file descriptors without `unsafe` code, which this crate
// forbids. A POSIX shell moves the two piped standard streams to the descriptors that
// `--remote-debugging-pipe` reads (3) and writes (4), then replaces itself with the browser,
// which keeps the process ID and group.
const PIPE_SHELL: &str = r#"exec "$0" "$@" 3<&0 4>&1 </dev/null >/dev/null"#;

/// Whether a window shows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Display {
    Headless,
    Headed,
}

/// What one browser command launches.
pub(crate) struct BrowserSession<'a> {
    pub(crate) executable: &'a Path,
    pub(crate) profile: &'a BrowserProfile,
    /// The page language, such as `zh-CN`.
    pub(crate) language: &'a str,
    pub(crate) display: Display,
    /// Which requests passive capture keeps, and how much of each.
    pub(crate) capture: CaptureFilter,
}

/// Page logic that logs a user into a platform: where the login starts and how what the page
/// captured shows that it succeeded. A platform's browser route provides it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LoginPage {
    pub(crate) entry_url: &'static str,
    pub(crate) capture: CaptureFilter,
    /// Whether the exchanges captured so far show a logged-in session.
    pub(crate) logged_in: fn(&[Exchange]) -> bool,
}

/// A login into a platform's Browser Profile: the route whose environment and access policy it
/// uses, and that route's login page logic. The provider factory builds it from the route's
/// browser environment declaration.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BrowserLogin {
    /// The route whose access policy a login counts against.
    pub(crate) route: crate::catalog::ProviderId,
    /// The Browser Profile, named after the platform.
    pub(crate) profile: &'static str,
    pub(crate) language: &'static str,
    pub(crate) page: LoginPage,
}

/// Returns whether this host can run browser routes: the transport needs Unix process groups
/// and a POSIX shell.
pub(crate) fn host_support() -> Result<(), String> {
    if cfg!(unix) {
        Ok(())
    } else {
        Err("browser routes need Unix process groups".into())
    }
}

/// The browser's command line, after the executable.
pub(crate) fn launch_arguments(
    user_data: &Path,
    seed: u32,
    screen: Option<Screen>,
    language: &str,
    display: Display,
) -> Vec<OsString> {
    let primary = language.split('-').next().unwrap_or(language);
    let accept = if primary == language {
        language.to_owned()
    } else {
        format!("{language},{primary}")
    };
    let mut arguments = vec![
        OsString::from(format!("--user-data-dir={}", user_data.display())),
        format!("--fingerprint={seed}").into(),
        format!("--fingerprint-platform={}", fingerprint_platform()).into(),
        "--fingerprint-brand=Chrome".into(),
        format!("--lang={language}").into(),
        format!("--accept-lang={accept}").into(),
        "--remote-debugging-pipe".into(),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
    ];
    if display == Display::Headless {
        arguments.push("--headless=new".into());
        if let Some(screen) = screen {
            arguments.extend(screen_arguments(screen));
        }
    }
    arguments.push("about:blank".into());
    arguments
}

fn fingerprint_platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        _ => "linux",
    }
}

/// A headless browser reports an 800×600 screen at device pixel ratio 1 unless told otherwise.
/// `--screen-info` takes physical pixels, so the recorded CSS sizes are scaled by the ratio.
fn screen_arguments(screen: Screen) -> [OsString; 2] {
    let scale = |css: u32| (f64::from(css) * screen.device_pixel_ratio).round();
    let bottom = screen
        .height
        .saturating_sub(screen.avail_top)
        .saturating_sub(screen.avail_height);
    [
        format!(
            "--screen-info={{0,0 {}x{} devicePixelRatio={} workAreaTop={} workAreaBottom={}}}",
            scale(screen.width),
            scale(screen.height),
            screen.device_pixel_ratio,
            scale(screen.avail_top),
            scale(bottom),
        )
        .into(),
        format!(
            "--window-size={},{}",
            screen.window_width, screen.window_height
        )
        .into(),
    ]
}

/// The working and read deadlines of one browser command.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Deadlines {
    pub(crate) work: Deadline,
    pub(crate) read: Deadline,
}

impl Deadlines {
    /// Derives both deadlines from the attempt deadline.
    ///
    /// # Errors
    ///
    /// Returns a Timeout failure when too little time is left to read a page.
    pub(crate) fn from_attempt(deadline: Deadline) -> Result<Self, AttemptFailure> {
        let work = deadline
            .remaining()
            .and_then(|remaining| remaining.checked_sub(CLEANUP_RESERVE))
            .ok_or_else(no_time)?;
        let read = work
            .checked_sub(READ_RESERVE)
            .filter(|read| read >= &Duration::from_secs(1))
            .ok_or_else(no_time)?;
        Ok(Self {
            work: Deadline::new(work),
            read: Deadline::new(read),
        })
    }
}

fn no_time() -> AttemptFailure {
    AttemptFailure {
        kind: AttemptErrorKind::Timeout,
        status: None,
        message: "no time is left to run a browser command".into(),
    }
}

/// Runs one browser command: launches the browser for `session`, gives `work` its page, then
/// closes the browser. Returns the work's value with the profile, still locked, so the caller
/// can record what the command established before releasing it.
///
/// # Errors
///
/// Returns an attempt failure for a pacing failure, a busy or unusable profile, a missing
/// executable, a failed handshake (Runtime), the work's own failure, or a missed working
/// deadline (Timeout).
pub(crate) async fn run<T>(
    session: &BrowserSession<'_>,
    limiter: &RateLimiter,
    deadline: Deadline,
    work: impl AsyncFnOnce(&mut Page) -> Result<T, AttemptFailure>,
) -> Result<(T, LockedProfile), AttemptFailure> {
    host_support().map_err(runtime)?;
    let deadlines = Deadlines::from_attempt(deadline)?;
    if !session.executable.is_file() {
        return Err(runtime(format!(
            "cannot launch the browser `{}`: no such file; run `forager browser install`, or fix `browser.path`",
            session.executable.display()
        )));
    }
    let permit = acquire_window(limiter, deadlines.work).await?;
    let profile = lock_profile(session.profile.clone()).await?;
    let arguments = launch_arguments(
        &profile.user_data_directory(),
        profile.seed(),
        profile.screen(),
        session.language,
        session.display,
    );
    let mut group = ProcessGroup::spawn(session.executable, &arguments, permit, profile)?;
    let Some(working) = deadlines.work.remaining() else {
        group.kill_and_reap().await;
        return Err(deadline_failure());
    };
    let driven = tokio::time::timeout(working, drive(&mut group, session, deadlines, work)).await;
    match driven {
        Err(_) => {
            group.kill_and_reap().await;
            Err(deadline_failure())
        }
        Ok(Err(handshake)) => {
            group.kill_and_reap().await;
            Err(handshake)
        }
        Ok(Ok((worked, cdp))) => {
            close(&mut group, &cdp, deadlines.work).await;
            drop(cdp);
            let profile = group.release();
            worked.map(|value| (value, profile))
        }
    }
}

/// Connects to the browser and runs `work` on its page. The outer error is a failed handshake;
/// the inner result is the work's own, after which the browser is still open to close normally.
async fn drive<T>(
    group: &mut ProcessGroup,
    session: &BrowserSession<'_>,
    deadlines: Deadlines,
    work: impl AsyncFnOnce(&mut Page) -> Result<T, AttemptFailure>,
) -> Result<(Result<T, AttemptFailure>, Cdp), AttemptFailure> {
    let (cdp, events) = group.connect()?;
    let session_id = match attach(&cdp).await {
        Ok(session_id) => session_id,
        Err(error) => return Err(group.handshake_failure(&error).await),
    };
    let worked = match Page::open(cdp.clone(), session_id, events, deadlines, session.capture).await
    {
        Ok(mut page) => work(&mut page).await,
        Err(failure) => Err(failure),
    };
    Ok((worked, cdp))
}

/// Connects to the browser's first page: finds or creates it and attaches a flat session.
async fn attach(cdp: &Cdp) -> Result<String, CdpError> {
    #[derive(Deserialize)]
    struct Targets {
        #[serde(rename = "targetInfos")]
        targets: Vec<Target>,
    }
    #[derive(Deserialize)]
    struct Target {
        #[serde(rename = "targetId")]
        id: String,
        #[serde(rename = "type")]
        kind: String,
    }
    #[derive(Deserialize)]
    struct Created {
        #[serde(rename = "targetId")]
        id: String,
    }
    #[derive(Deserialize)]
    struct Attached {
        #[serde(rename = "sessionId")]
        session: String,
    }
    let handshake = Deadline::new(HANDSHAKE_LIMIT);
    let targets: Targets = cdp
        .call_as(None, "Target.getTargets", json!({}), handshake)
        .await?;
    let page = if let Some(page) = targets
        .targets
        .into_iter()
        .find(|target| target.kind == "page")
    {
        page.id
    } else {
        let created: Created = cdp
            .call_as(
                None,
                "Target.createTarget",
                json!({"url": "about:blank"}),
                handshake,
            )
            .await?;
        created.id
    };
    let attached: Attached = cdp
        .call_as(
            None,
            "Target.attachToTarget",
            json!({"targetId": page, "flatten": true}),
            handshake,
        )
        .await?;
    Ok(attached.session)
}

/// Closes the browser normally so it writes its profile, and kills its group when it has not
/// exited within the close limit or by the working deadline.
async fn close(group: &mut ProcessGroup, cdp: &Cdp, work: Deadline) {
    let limit = work.remaining().unwrap_or_default().min(CLOSE_LIMIT);
    let close = Deadline::new(limit);
    // The browser may end the pipe before it answers; either way it is closing.
    let _ = cdp.call(None, "Browser.close", json!({}), close).await;
    if !group.wait_exit(close.remaining().unwrap_or_default()).await {
        group.kill_and_reap().await;
    }
}

async fn lock_profile(profile: BrowserProfile) -> Result<LockedProfile, AttemptFailure> {
    tokio::task::spawn_blocking(move || profile.lock())
        .await
        .map_err(|error| runtime(format!("cannot open the Browser Profile: {error}")))?
        .map_err(|error| profile_failure(&error))
}

fn profile_failure(error: &ProfileError) -> AttemptFailure {
    runtime(error.to_string())
}

fn deadline_failure() -> AttemptFailure {
    AttemptFailure {
        kind: AttemptErrorKind::Timeout,
        status: None,
        message: "the browser command did not finish before its deadline".into(),
    }
}

pub(crate) fn runtime(message: String) -> AttemptFailure {
    AttemptFailure {
        kind: AttemptErrorKind::Runtime,
        status: None,
        message,
    }
}

fn cdp_failure(error: &CdpError) -> AttemptFailure {
    runtime(truncate_message(&redact_urls(&error.to_string())))
}

/// The page that one browser command drives. Every method that waits stops at the working
/// deadline; input stops at the read deadline.
pub(crate) struct Page {
    cdp: Cdp,
    session: String,
    deadlines: Deadlines,
    capture: capture::Capture,
    pointer: (f64, f64),
}

/// How a navigation ended.
#[derive(Debug)]
pub(crate) struct Navigation {
    /// The network error that stopped it, such as `net::ERR_CONNECTION_REFUSED`.
    pub(crate) error: Option<String>,
    /// Whether the document finished loading before the read deadline.
    pub(crate) loaded: bool,
}

impl Page {
    async fn open(
        cdp: Cdp,
        session: String,
        events: tokio::sync::mpsc::UnboundedReceiver<crate::providers::cdp::CdpEvent>,
        deadlines: Deadlines,
        filter: CaptureFilter,
    ) -> Result<Self, AttemptFailure> {
        let capture =
            capture::Capture::start(cdp.clone(), session.clone(), events, filter, deadlines.work);
        let page = Self {
            cdp,
            session,
            deadlines,
            capture,
            pointer: (
                fastrand::f64().mul_add(300.0, 200.0),
                fastrand::f64().mul_add(200.0, 200.0),
            ),
        };
        page.call("Network.enable", capture::network_enable_params())
            .await?;
        Ok(page)
    }

    /// Whether the browser has closed its end of the connection, as it does when it exits.
    pub(crate) fn closed(&self) -> bool {
        self.cdp.is_closed()
    }

    /// Whether page logic may still wait and send input.
    pub(crate) fn reading(&self) -> bool {
        self.deadlines
            .read
            .remaining()
            .is_some_and(|left| !left.is_zero())
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value, AttemptFailure> {
        self.cdp
            .call(Some(&self.session), method, params, self.deadlines.work)
            .await
            .map_err(|error| cdp_failure(&error))
    }

    /// Opens `url` and waits until its document has loaded, at most until the read deadline.
    ///
    /// # Errors
    ///
    /// Returns a Runtime failure when the browser refuses the navigation command.
    pub(crate) async fn navigate(&mut self, url: &str) -> Result<Navigation, AttemptFailure> {
        #[derive(Deserialize)]
        struct Navigated {
            #[serde(rename = "errorText")]
            error: Option<String>,
        }
        let navigated = self.call("Page.navigate", json!({"url": url})).await?;
        let navigated: Navigated = serde_json::from_value(navigated)
            .map_err(|error| runtime(format!("unexpected Page.navigate result: {error}")))?;
        let mut loaded = false;
        while self.reading() {
            if self
                .evaluate::<String>("document.readyState")
                .await
                .is_ok_and(|state| state == "complete")
            {
                loaded = true;
                break;
            }
            self.rest(READY_POLL).await;
        }
        Ok(Navigation {
            error: navigated.error.filter(|error| !error.is_empty()),
            loaded,
        })
    }

    /// Evaluates a read-only `script` in the page and decodes its value.
    ///
    /// # Errors
    ///
    /// Returns a Runtime failure when the script throws or its value has another shape.
    pub(crate) async fn evaluate<T: DeserializeOwned>(
        &self,
        script: &str,
    ) -> Result<T, AttemptFailure> {
        #[derive(Deserialize)]
        struct Evaluated {
            result: Option<RemoteObject>,
            #[serde(rename = "exceptionDetails")]
            exception: Option<Value>,
        }
        #[derive(Deserialize)]
        struct RemoteObject {
            value: Option<Value>,
        }
        let evaluated = self
            .call(
                "Runtime.evaluate",
                json!({"expression": script, "returnByValue": true, "awaitPromise": true}),
            )
            .await?;
        let evaluated: Evaluated = serde_json::from_value(evaluated)
            .map_err(|error| runtime(format!("unexpected Runtime.evaluate result: {error}")))?;
        if let Some(exception) = evaluated.exception {
            let text = exception
                .pointer("/exception/description")
                .or_else(|| exception.get("text"))
                .and_then(Value::as_str)
                .unwrap_or("unknown exception");
            return Err(runtime(format!(
                "a page script failed: {}",
                truncate_message(&redact_urls(text))
            )));
        }
        let value = evaluated
            .result
            .and_then(|result| result.value)
            .unwrap_or(Value::Null);
        serde_json::from_value(value).map_err(|error| {
            runtime(format!(
                "a page script returned an unexpected value: {error}"
            ))
        })
    }

    /// Takes the exchanges captured since the last call whose responses have ended.
    ///
    /// # Errors
    ///
    /// Returns a Runtime failure when a kept response body exceeded the body limit.
    pub(crate) fn take_exchanges(&self) -> Result<Vec<Exchange>, AttemptFailure> {
        self.capture.take()
    }

    /// Reads the screen and window size the page reports, in CSS pixels.
    ///
    /// # Errors
    ///
    /// Returns a Runtime failure when the page cannot be read.
    pub(crate) async fn screen(&self) -> Result<Screen, AttemptFailure> {
        self.evaluate(
            "({width: screen.width, height: screen.height, avail_top: screen.availTop, avail_height: screen.availHeight, device_pixel_ratio: devicePixelRatio, window_width: outerWidth, window_height: outerHeight})",
        )
        .await
    }

    /// Waits `duration`, but never past the read deadline.
    pub(crate) async fn rest(&self, duration: Duration) {
        let left = self.deadlines.read.remaining().unwrap_or_default();
        tokio::time::sleep(duration.min(left)).await;
    }
}

/// Owns a launched browser's process group until its leader is reaped. Dropping it earlier
/// kills the whole group, and keeps the access permit and the profile lock until the leader is
/// gone.
struct ProcessGroup {
    child: Option<Child>,
    permit: Option<RatePermit>,
    profile: Option<LockedProfile>,
    stderr: Arc<Mutex<Vec<u8>>>,
    stderr_task: Option<JoinHandle<()>>,
}

impl ProcessGroup {
    fn spawn(
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
            permit: Some(permit),
            profile: Some(profile),
            stderr,
            stderr_task,
        })
    }

    /// Starts the CDP connection over the debugging pipe.
    fn connect(
        &mut self,
    ) -> Result<
        (
            Cdp,
            tokio::sync::mpsc::UnboundedReceiver<crate::providers::cdp::CdpEvent>,
        ),
        AttemptFailure,
    > {
        let child = self.child.as_mut().expect("the group owns its leader");
        let (Some(commands), Some(answers)) = (child.stdin.take(), child.stdout.take()) else {
            return Err(runtime("cannot open the browser debugging pipe".into()));
        };
        Ok(Cdp::start(answers, commands))
    }

    /// The failure of a handshake that `error` ended: the browser exited, answered garbage, or
    /// never answered. It carries the last line the browser wrote to stderr.
    async fn handshake_failure(&mut self, error: &CdpError) -> AttemptFailure {
        let exited = self.wait_exit(Duration::from_millis(200)).await;
        if let Some(task) = self.stderr_task.take()
            && exited
        {
            let _ = tokio::time::timeout(Duration::from_millis(200), task).await;
        }
        let line = last_line(&self.stderr.lock().unwrap_or_else(PoisonError::into_inner));
        runtime(format!(
            "the browser did not complete the CDP handshake ({}): {}",
            if exited {
                "it exited"
            } else {
                error_summary(error)
            },
            truncate_message(&redact_urls(&line))
        ))
    }

    /// Waits up to `within` for the leader to exit, and reaps it when it does.
    async fn wait_exit(&mut self, within: Duration) -> bool {
        let Some(child) = self.child.as_mut() else {
            return true;
        };
        if matches!(tokio::time::timeout(within, child.wait()).await, Ok(Ok(_))) {
            self.child = None;
            // Helpers the browser left behind in its group go with it.
            return true;
        }
        false
    }

    async fn kill_and_reap(&mut self) {
        if let Some(child) = &self.child {
            kill_group(child);
        }
        if let Some(child) = self.child.as_mut() {
            let _ = child.wait().await;
        }
        self.child = None;
    }

    /// Releases the permit of a reaped browser and returns its still-locked profile.
    fn release(mut self) -> LockedProfile {
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
        // A killed process exits at once, so a short blocking wait reaps it here and the permit
        // and the profile lock outlive the process even when the runtime shuts down next.
        for _ in 0..REAP_POLLS {
            if !matches!(child.try_wait(), Ok(None)) {
                break;
            }
            std::thread::sleep(REAP_POLL_INTERVAL);
        }
        self.permit = None;
        self.profile = None;
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
    use std::path::Path;

    use super::{Display, launch_arguments};
    use crate::providers::browser_profile::Screen;

    #[cfg(unix)]
    #[test]
    fn dropping_the_command_kills_the_browser_process_group() {
        use std::os::unix::fs::PermissionsExt;
        use std::time::Duration;

        use super::{BrowserSession, CaptureLevel, run};
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
            display: Display::Headless,
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
            });
        let running = |file: &std::path::PathBuf| {
            let pid = std::fs::read_to_string(file).expect("recorded pid");
            let pid = nix::unistd::Pid::from_raw(pid.trim().parse().expect("pid"));
            nix::sys::signal::kill(pid, None).is_ok()
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while pid_files.iter().any(running) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let alive = pid_files.each_ref().map(running);

        assert_eq!(alive, [false, false]);
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

    fn arguments(display: Display, screen: Option<Screen>) -> Vec<String> {
        launch_arguments(
            Path::new("/profile/user-data"),
            42,
            screen,
            "zh-CN",
            display,
        )
        .into_iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect()
    }

    #[test]
    fn a_headless_launch_carries_the_profile_seed_and_language() {
        let arguments = arguments(Display::Headless, None);

        assert_eq!(
            arguments
                .iter()
                .filter(|argument| !argument.starts_with("--fingerprint-platform="))
                .map(String::as_str)
                .collect::<Vec<_>>(),
            [
                "--user-data-dir=/profile/user-data",
                "--fingerprint=42",
                "--fingerprint-brand=Chrome",
                "--lang=zh-CN",
                "--accept-lang=zh-CN,zh",
                "--remote-debugging-pipe",
                "--no-first-run",
                "--no-default-browser-check",
                "--headless=new",
                "about:blank",
            ]
        );
    }

    #[test]
    fn a_headed_launch_has_no_headless_switch() {
        let arguments = arguments(Display::Headed, None);

        assert!(
            !arguments
                .iter()
                .any(|argument| argument.starts_with("--headless")),
            "{arguments:?}"
        );
    }

    #[test]
    fn a_headless_launch_reports_the_recorded_screen_in_physical_pixels() {
        let screen = Screen {
            width: 1512,
            height: 982,
            avail_top: 25,
            avail_height: 944,
            device_pixel_ratio: 2.0,
            window_width: 1200,
            window_height: 800,
        };

        let arguments = arguments(Display::Headless, Some(screen));

        assert_eq!(
            arguments[arguments.len() - 3..],
            [
                "--screen-info={0,0 3024x1964 devicePixelRatio=2 workAreaTop=50 workAreaBottom=26}",
                "--window-size=1200,800",
                "about:blank",
            ]
        );
    }
}
