//! The browser transport against a real Managed Browser and local static pages. These tests
//! never open a real site.
//!
//! They run only when `FORAGER_REAL_BROWSER=1`, and launch the installed Managed Browser, or the
//! executable that `FORAGER_REAL_BROWSER_PATH` names; without the variable each test returns at
//! once. Run them before a release on a host with the Managed Browser:
//! `FORAGER_REAL_BROWSER=1 cargo nextest run --lib real_tests`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use reqwest::Url;
use serde_json::Value;

use super::input::{InputError, Key};
use super::{
    BrowserLogin, BrowserSession, CaptureLevel, Display, Exchange, LoginPage, Page, RequestMethod,
    ResponseBody, run,
};
use crate::catalog::ProviderId;
use crate::config::BrowserRuntimeConfig;
use crate::net::AttemptFailure;
use crate::providers::browser_profile::{BrowserProfile, Screen};
use crate::providers::managed_browser;
use crate::rate_limit::{AccessPolicy, RateLimiter};
use crate::types::Deadline;

const GO_BUTTONS: &str = "document.querySelectorAll('.go')";
const ENABLED: &str = "FORAGER_REAL_BROWSER";
const EXECUTABLE: &str = "FORAGER_REAL_BROWSER_PATH";
const DRIVER_PROFILE: &str = "FORAGER_REAL_BROWSER_DRIVER_PROFILE";
const COMMAND_TIMEOUT: Duration = Duration::from_mins(1);

fn executable() -> Option<PathBuf> {
    if std::env::var_os(ENABLED).is_none_or(|value| value != "1") {
        eprintln!("skipped: set {ENABLED}=1 to run the real browser tests");
        return None;
    }
    let config = BrowserRuntimeConfig {
        path: std::env::var_os(EXECUTABLE).map(PathBuf::from),
    };
    Some(managed_browser::locate(&config).unwrap_or_else(|error| panic!("{error}")))
}

fn block_on<T>(future: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("build test runtime")
        .block_on(future)
}

fn local(url: &Url) -> CaptureLevel {
    if url.host_str() == Some("127.0.0.1") && url.path() != "/favicon.ico" {
        CaptureLevel::Body
    } else {
        CaptureLevel::Ignore
    }
}

/// A Browser Profile and an access limiter in one temporary directory.
struct Sandbox {
    directory: tempfile::TempDir,
    limiter: RateLimiter,
}

impl Sandbox {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("create sandbox");
        let limiter = RateLimiter::in_state_file(
            "real_browser_test",
            AccessPolicy {
                min_interval: Duration::ZERO,
                max_concurrency: 1,
            },
            directory.path().join("rate_limit_state.json"),
        );
        Self { directory, limiter }
    }

    fn profile(&self) -> BrowserProfile {
        BrowserProfile::at(self.directory.path().join("profile"))
    }

    /// Runs `work` on a headless page of the sandbox profile.
    fn run<T>(
        &self,
        executable: &Path,
        work: impl AsyncFnOnce(&mut Page) -> Result<T, AttemptFailure>,
    ) -> T {
        let profile = self.profile();
        let session = BrowserSession {
            executable,
            profile: &profile,
            language: "zh-CN",
            display: Display::Headless,
            capture: local,
        };
        block_on(run(
            &session,
            &self.limiter,
            Deadline::new(COMMAND_TIMEOUT),
            work,
        ))
        .map_or_else(
            |failure| panic!("browser command failed: {failure:?}"),
            |(value, _profile)| value,
        )
    }
}

/// Serves the test pages on 127.0.0.1 until dropped.
struct Pages {
    origin: String,
    stop: Arc<AtomicBool>,
    address: std::net::SocketAddr,
}

impl Pages {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test pages");
        let address = listener.local_addr().expect("test pages address");
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        thread::spawn(move || {
            for stream in listener.incoming() {
                if stopped.load(Ordering::SeqCst) {
                    return;
                }
                if let Ok(stream) = stream {
                    thread::spawn(move || serve(stream));
                }
            }
        });
        Self {
            origin: format!("http://{address}"),
            stop,
            address,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.origin)
    }
}

impl Drop for Pages {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.address);
    }
}

fn serve(mut stream: TcpStream) {
    let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let mut length = 0;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).is_err() || header.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0; length];
    let _ = reader.read_exact(&mut body);
    let path = request_line.split_whitespace().nth(1).unwrap_or("/");
    let path = path.split('?').next().unwrap_or(path);
    let (content_type, content) = match path {
        "/input" => ("text/html", INPUT_PAGE.to_owned()),
        "/ambiguous" => ("text/html", AMBIGUOUS_PAGE.to_owned()),
        "/hover-twin" => ("text/html", hover_page("twin")),
        "/hover-gone" => ("text/html", hover_page("gone")),
        "/hover-moved" => ("text/html", hover_page("moved")),
        "/scroll" => ("text/html", SCROLL_PAGE.to_owned()),
        "/more" => ("application/json", r#"{"count":20}"#.to_owned()),
        "/api" => ("text/html", API_PAGE.to_owned()),
        "/api/data" => ("application/json", r#"{"items":[1,2,3]}"#.to_owned()),
        "/api/echo" | "/api/me" => (
            "application/json",
            if body.is_empty() {
                r#"{"user":"local"}"#.to_owned()
            } else {
                String::from_utf8_lossy(&body).into_owned()
            },
        ),
        "/store" => ("text/html", STORE_PAGE.to_owned()),
        "/login" => ("text/html", LOGIN_PAGE.to_owned()),
        _ => (
            "text/html",
            "<!doctype html><title>blank</title>".to_owned(),
        ),
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: {content_type}; charset=utf-8\r\ncontent-length: {}\r\ncache-control: no-store\r\nconnection: close\r\n\r\n{content}",
        content.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

// The real button is stacked over two decoys with the same class and text: one in an
// `aria-hidden` subtree and one covered by the real button.
const INPUT_PAGE: &str = r#"<!doctype html><meta charset="utf-8"><title>input</title>
<style>#stack{position:relative;width:200px;height:60px;margin:40px}.go{position:absolute;left:0;top:0;width:200px;height:60px}</style>
<div id="stack"><div aria-hidden="true"><button class="go" id="hidden">Go</button></div><button class="go" id="covered">Go</button><button class="go" id="real">Go</button></div>
<input id="field" style="display:block;margin:40px;width:300px;height:30px">
<script>
window.events = [];
for (const type of ['mousemove', 'mousedown', 'mouseup', 'click', 'keydown', 'input']) {
  document.addEventListener(type, (event) => events.push({ type, trusted: event.isTrusted, target: event.target.id }), true);
}
for (const id of ['hidden', 'covered', 'real']) {
  document.getElementById(id).addEventListener('click', () => { window.clicked = id; document.getElementById('field').focus(); });
}
</script>"#;

const AMBIGUOUS_PAGE: &str = r#"<!doctype html><title>ambiguous</title>
<button class="twin" style="margin:40px">One</button><button class="twin" style="margin:40px">Two</button>"#;

// One button that, once the pointer reaches it, gains a hittable twin, disappears, or moves away.
fn hover_page(change: &str) -> String {
    format!(
        r#"<!doctype html><title>hover</title>
<button class="solo" style="position:absolute;left:100px;top:100px;width:120px;height:40px">Go</button>
<script>
const button = document.querySelector('.solo');
button.addEventListener('mouseover', () => {{
  const change = '{change}';
  if (change === 'twin') {{ const twin = button.cloneNode(true); twin.style.top = '300px'; document.body.appendChild(twin); }}
  if (change === 'gone') button.remove();
  if (change === 'moved') button.style.top = '400px';
}}, {{ once: true }});
</script>"#
    )
}

const SCROLL_PAGE: &str = r#"<!doctype html><title>scroll</title>
<div id="list" style="height:300px;width:400px;overflow-y:auto"><div id="items"></div></div>
<script>
const items = document.getElementById('items');
const list = document.getElementById('list');
const add = (count) => { for (let i = 0; i < count; i += 1) { const row = document.createElement('div'); row.style.height = '50px'; row.textContent = 'row'; items.appendChild(row); } };
add(20);
let loading = false;
let page = 1;
list.addEventListener('scroll', () => {
  if (loading || list.scrollTop + list.clientHeight < list.scrollHeight - 100) return;
  loading = true;
  page += 1;
  fetch('/more?page=' + page).then((response) => response.json()).then((more) => { add(more.count); loading = false; });
});
</script>"#;

const API_PAGE: &str = r"<!doctype html><title>api</title>
<script>
fetch('/api/data').then((response) => response.text());
fetch('/api/echo', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ q: '咖啡' }) }).then((response) => response.text());
</script>";

const STORE_PAGE: &str = r"<!doctype html><title>store</title>
<script>localStorage.setItem('kept', 'yes'); document.cookie = 'kept=yes; max-age=3600; path=/';</script>";

const LOGIN_PAGE: &str = r"<!doctype html><title>login</title>
<script>setTimeout(() => fetch('/api/me').then((response) => response.text()), 1500);</script>";

/// Unwraps a native action, failing the test with the reason it did not happen.
fn done<T>(action: Result<T, InputError>) -> T {
    match action {
        Ok(value) => value,
        Err(InputError::Failed(failure)) => panic!("native input failed: {}", failure.reason()),
        Err(InputError::Transport(failure)) => panic!("browser failed: {}", failure.message),
    }
}

/// Polls the captured exchanges until one has each of `paths`, or `within` passes. Returns
/// the first exchange at each path that arrived.
async fn exchanges_at<const N: usize>(
    page: &Page,
    paths: [&str; N],
    within: Duration,
) -> [Option<Exchange>; N] {
    let until = Instant::now() + within;
    let mut seen = Vec::new();
    loop {
        seen.extend(page.take_exchanges().expect("take exchanges"));
        let found = paths.map(|path| {
            seen.iter()
                .find(|exchange| exchange.url.path() == path)
                .cloned()
        });
        if found.iter().all(Option::is_some) || Instant::now() >= until {
            return found;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[test]
fn native_input_reaches_the_page_as_trusted_events_on_the_one_hittable_target() {
    let Some(executable) = executable() else {
        return;
    };
    let pages = Pages::start();

    let (loaded, clicked, value, events) = Sandbox::new().run(&executable, async |page| {
        let navigation = page.navigate(&pages.url("/input")).await?;
        done(page.hover(GO_BUTTONS).await);
        done(page.click(GO_BUTTONS).await);
        done(
            page.confirm("window.clicked === 'real'", Duration::from_secs(2))
                .await,
        );
        done(page.type_text("abc").await);
        done(page.press(Key::SelectAll).await);
        done(page.press(Key::Delete).await);
        done(page.type_text("xy").await);
        done(page.press(Key::Backspace).await);
        done(page.press(Key::Escape).await);
        done(page.press(Key::Enter).await);
        let clicked: String = page.evaluate("window.clicked").await?;
        let value: String = page
            .evaluate("document.getElementById('field').value")
            .await?;
        let events: Vec<Value> = page.evaluate("window.events").await?;
        Ok((navigation.loaded, clicked, value, events))
    });

    let kinds = ["mousemove", "mousedown", "click", "input", "keydown"];
    assert_eq!(
        (
            loaded,
            clicked.as_str(),
            value.as_str(),
            kinds.map(|kind| events.iter().any(|event| event["type"] == kind)),
            events.iter().all(|event| event["trusted"] == true),
        ),
        (true, "real", "x", [true; 5], true),
        "{events:?}"
    );
}

#[test]
fn a_click_whose_effect_never_shows_is_missed() {
    let Some(executable) = executable() else {
        return;
    };
    let pages = Pages::start();

    let missed = Sandbox::new().run(&executable, async |page| {
        page.navigate(&pages.url("/input")).await?;
        done(page.click(GO_BUTTONS).await);
        Ok(
            match page
                .confirm("window.clicked === 'covered'", Duration::from_secs(1))
                .await
            {
                Err(InputError::Failed(failure)) => Some(failure.reason()),
                _ => None,
            },
        )
    });

    assert_eq!(missed, Some("click_missed"));
}

#[test]
fn a_navigation_to_a_closed_port_reports_the_network_error() {
    let Some(executable) = executable() else {
        return;
    };
    let closed = TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .expect("reserve a closed port");

    let error = Sandbox::new().run(&executable, async |page| {
        Ok(page.navigate(&format!("http://{closed}/")).await?.error)
    });

    assert_eq!(error.as_deref(), Some("net::ERR_CONNECTION_REFUSED"));
}

#[test]
fn a_click_refuses_two_hittable_targets() {
    let Some(executable) = executable() else {
        return;
    };
    let pages = Pages::start();

    let failure = Sandbox::new().run(&executable, async |page| {
        page.navigate(&pages.url("/ambiguous")).await?;
        Ok(
            match page.click("document.querySelectorAll('.twin')").await {
                Err(InputError::Failed(failure)) => Some(failure.reason()),
                _ => None,
            },
        )
    });

    assert_eq!(failure, Some("ambiguous_target"));
}

#[test]
fn a_target_that_changes_during_the_hover_is_reported_by_what_changed() {
    let Some(executable) = executable() else {
        return;
    };
    let pages = Pages::start();

    let reasons = Sandbox::new().run(&executable, async |page| {
        let mut reasons = Vec::new();
        for path in ["/hover-twin", "/hover-gone", "/hover-moved"] {
            page.navigate(&pages.url(path)).await?;
            reasons.push(
                match page.click("document.querySelectorAll('.solo')").await {
                    Err(InputError::Failed(failure)) => Some(failure.reason()),
                    _ => None,
                },
            );
        }
        Ok(reasons)
    });

    assert_eq!(
        reasons,
        [
            Some("ambiguous_target"),
            Some("no_target"),
            Some("target_moved")
        ]
    );
}

#[test]
fn wheel_turns_in_a_scrolling_container_make_the_page_load_more() {
    let Some(executable) = executable() else {
        return;
    };
    let pages = Pages::start();

    let more = Sandbox::new().run(&executable, async |page| {
        page.navigate(&pages.url("/scroll")).await?;
        let center: (f64, f64) = page
            .evaluate("(() => { const r = document.getElementById('list').getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2]; })()")
            .await?;
        done(page.wheel(center, 1200.0).await);
        let [more] = exchanges_at(page, ["/more"], Duration::from_secs(5)).await;
        Ok(more)
    });

    assert_eq!(
        more.map(|exchange| exchange.body),
        Some(ResponseBody::Received(r#"{"count":20}"#.into()))
    );
}

#[test]
fn capture_keeps_response_bodies_and_post_bodies() {
    let Some(executable) = executable() else {
        return;
    };
    let pages = Pages::start();

    let (data, echo) = Sandbox::new().run(&executable, async |page| {
        page.navigate(&pages.url("/api")).await?;
        let [data, echo] =
            exchanges_at(page, ["/api/data", "/api/echo"], Duration::from_secs(5)).await;
        Ok((data, echo))
    });

    let data = data.expect("GET exchange");
    let echo = echo.expect("POST exchange");
    assert_eq!(
        (
            data.method,
            data.body,
            echo.method,
            echo.request_body.as_deref(),
            echo.status
        ),
        (
            RequestMethod::Get,
            ResponseBody::Received(r#"{"items":[1,2,3]}"#.into()),
            RequestMethod::Post,
            Some(r#"{"q":"咖啡"}"#),
            Some(200)
        )
    );
}

#[test]
fn a_normal_close_keeps_the_profile_for_the_next_launch() {
    let Some(executable) = executable() else {
        return;
    };
    let pages = Pages::start();
    let sandbox = Sandbox::new();
    sandbox.run(&executable, async |page| {
        page.navigate(&pages.url("/store")).await?;
        Ok(())
    });

    let kept = sandbox.run(&executable, async |page| {
        page.navigate(&pages.url("/other")).await?;
        page.evaluate::<(Option<String>, String)>("[localStorage.getItem('kept'), document.cookie]")
            .await
    });

    assert_eq!(kept, (Some("yes".into()), "kept=yes".into()));
}

#[test]
fn a_headless_launch_reports_the_screen_recorded_at_login() {
    let Some(executable) = executable() else {
        return;
    };
    let sandbox = Sandbox::new();
    let screen = Screen {
        width: 1512,
        height: 982,
        avail_top: 33,
        avail_height: 944,
        device_pixel_ratio: 2.0,
        window_width: 1280,
        window_height: 860,
    };
    sandbox
        .profile()
        .lock()
        .expect("lock profile")
        .record_login(Some(screen))
        .expect("record screen");

    let reported = sandbox.run(&executable, async |page| page.screen().await);

    assert_eq!(reported, screen);
}

#[test]
fn a_login_completes_once_the_page_logic_sees_the_session() {
    let Some(executable) = executable() else {
        return;
    };
    let pages = Pages::start();
    let login = BrowserLogin {
        route: ProviderId::XiaohongshuBrowser,
        profile: "local",
        language: "zh-CN",
        page: LoginPage {
            entry_url: Box::leak(pages.url("/login").into_boxed_str()),
            capture: local,
            logged_in: |exchanges| {
                exchanges.iter().any(|exchange| {
                    exchange.url.path() == "/api/me"
                        && exchange.body == ResponseBody::Received(r#"{"user":"local"}"#.into())
                })
            },
        },
    };

    let screen = Sandbox::new().run(&executable, async |page| {
        crate::browser_setup::await_login(page, &login).await
    });

    assert!(screen.device_pixel_ratio > 0.0, "{screen:?}");
}

/// The process the kill tests terminate: it drives a browser on the profile that
/// `FORAGER_REAL_BROWSER_DRIVER_PROFILE` names, prints `ready`, and waits.
#[test]
fn driver_helper() {
    let Some(profile) = std::env::var_os(DRIVER_PROFILE).map(PathBuf::from) else {
        return;
    };
    let executable = executable().expect("the driver runs only with the real browser enabled");
    let limiter = RateLimiter::in_state_file(
        "real_browser_driver",
        AccessPolicy {
            min_interval: Duration::ZERO,
            max_concurrency: 1,
        },
        profile.with_extension("rate_limit_state.json"),
    );
    let profile = BrowserProfile::at(profile);
    let session = BrowserSession {
        executable: &executable,
        profile: &profile,
        language: "zh-CN",
        display: Display::Headless,
        capture: local,
    };
    let _ = block_on(run(
        &session,
        &limiter,
        Deadline::new(Duration::from_mins(2)),
        async |page| {
            println!("ready");
            std::io::stdout().flush().expect("flush ready");
            page.rest(Duration::from_secs(110)).await;
            Ok(())
        },
    ));
}

fn browser_processes(profile: &Path) -> Vec<String> {
    let output = Command::new("pgrep")
        .arg("-f")
        .arg(profile.join("user-data").as_os_str())
        .output()
        .expect("run pgrep");
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

/// Starts the driver, kills it with `signal`, and returns how long its browser outlived it, or
/// `None` when browser processes were still running 10 seconds later.
fn browser_lifetime_after(signal: nix::sys::signal::Signal) -> Option<Duration> {
    let sandbox = tempfile::tempdir().expect("create sandbox");
    let profile = sandbox.path().join("profile");
    let mut driver = Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "--exact",
            "providers::browser::real_tests::driver_helper",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(DRIVER_PROFILE, &profile)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the driver");
    let stdout = driver.stdout.take().expect("driver stdout");
    let ready = BufReader::new(stdout)
        .lines()
        .map_while(Result::ok)
        .any(|line| line.trim_end().ends_with("ready"));
    assert!(ready, "the driver never reported ready");
    assert!(
        !browser_processes(&profile).is_empty(),
        "no browser runs on the driver's profile"
    );
    let pid = nix::unistd::Pid::from_raw(i32::try_from(driver.id()).expect("driver pid"));
    nix::sys::signal::kill(pid, signal).expect("signal the driver");
    let _ = driver.wait();
    let killed = Instant::now();
    while killed.elapsed() < Duration::from_secs(10) {
        if browser_processes(&profile).is_empty() {
            return Some(killed.elapsed());
        }
        thread::sleep(Duration::from_millis(50));
    }
    None
}

#[test]
fn the_browser_exits_after_its_driver_is_killed() {
    use nix::sys::signal::Signal;

    if executable().is_none() || std::env::var_os(DRIVER_PROFILE).is_some() {
        return;
    }

    let lifetimes = [Signal::SIGINT, Signal::SIGTERM, Signal::SIGKILL]
        .map(|signal| (signal, browser_lifetime_after(signal)));
    eprintln!("browser lifetime after its driver died: {lifetimes:?}");

    assert!(
        lifetimes.iter().all(|(_, lifetime)| lifetime.is_some()),
        "browser processes outlived their driver: {lifetimes:?}"
    );
}
