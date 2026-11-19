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
use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::net::{AttemptFailure, truncate_message};
use crate::providers::browser_profile::{BrowserProfile, LockedProfile, ProfileError, Screen};
use crate::providers::cdp::{Cdp, CdpError};
use crate::providers::shared::acquire_window;
use crate::rate_limit::RateLimiter;
use crate::redact::redact_urls;
use crate::types::{AttemptErrorKind, Deadline};

#[path = "browser_capture.rs"]
mod capture;
#[path = "browser_input.rs"]
mod input;
#[path = "browser_process.rs"]
mod process;
#[cfg(all(test, unix))]
#[path = "browser_real_tests.rs"]
pub(crate) mod real_tests;

pub(crate) use capture::{CaptureFilter, CaptureLevel, Exchange, RequestMethod, ResponseBody};
pub(crate) use input::{InputError, InputFailure, Key, Missed};
use process::ProcessGroup;

const CLEANUP_RESERVE: Duration = Duration::from_secs(5);
const READ_RESERVE: Duration = Duration::from_secs(3);
const CLOSE_LIMIT: Duration = Duration::from_secs(3);
const HANDSHAKE_LIMIT: Duration = Duration::from_secs(20);
const READY_POLL: Duration = Duration::from_millis(250);

/// Whether a window shows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WindowMode {
    Headless,
    Headed,
}

/// What one browser command launches.
pub(crate) struct BrowserSession<'a> {
    pub(crate) executable: &'a Path,
    pub(crate) profile: &'a BrowserProfile,
    /// The page language, such as `zh-CN`.
    pub(crate) language: &'a str,
    pub(crate) window: WindowMode,
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
    window: WindowMode,
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
    if window == WindowMode::Headless {
        arguments.push("--headless=new".into());
        if let Some(screen) = screen {
            arguments.extend(screen_arguments(screen));
        }
    }
    arguments.push("about:blank".into());
    arguments
}

// Browser routes run only on Unix hosts (`host_support`).
fn fingerprint_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
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
        session.window,
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

    /// How many requests the page has sent so far: an exchange whose `sequence` is at least
    /// this answers a request sent after this call. Page logic takes it just before an action
    /// whose answer it waits for.
    pub(crate) fn requests_sent(&self) -> u64 {
        self.capture.requests_sent()
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{WindowMode, launch_arguments};
    use crate::providers::browser_profile::Screen;

    fn arguments(window: WindowMode, screen: Option<Screen>) -> Vec<String> {
        launch_arguments(Path::new("/profile/user-data"), 42, screen, "zh-CN", window)
            .into_iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn a_headless_launch_carries_the_profile_seed_and_language() {
        let arguments = arguments(WindowMode::Headless, None);

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
        let arguments = arguments(WindowMode::Headed, None);

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

        let arguments = arguments(WindowMode::Headless, Some(screen));

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
