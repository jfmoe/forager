//! `forager browser` against a shell stand-in for the browser: how the browser transport
//! launches the browser, bounds it, and cleans it up, seen through `browser login`. The stand-in
//! speaks no CDP, so no test here reaches a page.
#![cfg(unix)]

use std::fs;
use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, Instant};

use fs2::FileExt;
use serde_json::json;

use crate::support::browser::StandIn;
use crate::support::{RunEnvironment, payload};

fn login(environment: &RunEnvironment) -> Output {
    environment.run(&["browser", "login", "xiaohongshu", "--timeout", "10"])
}

fn process_exists(pid: &str) -> bool {
    Command::new("/bin/kill")
        .args(["-0", pid.trim()])
        .output()
        .is_ok_and(|output| output.status.success())
}

#[test]
fn login_launches_a_headed_browser_on_the_platform_profile() {
    let stand_in = StandIn::failing("no display");
    let environment = RunEnvironment::new(&stand_in.config());

    login(&environment);
    let launch = stand_in.launches().remove(0);
    let user_data = environment
        .state_dir
        .join("forager/browser/xiaohongshu/user-data");

    assert_eq!(
        (
            launch.contains(&format!("--user-data-dir={}", user_data.display())),
            launch.contains(&"--lang=zh-CN".to_owned()),
            launch.contains(&"--remote-debugging-pipe".to_owned()),
            launch
                .iter()
                .any(|argument| argument.starts_with("--fingerprint=")),
            launch
                .iter()
                .any(|argument| argument.starts_with("--headless")),
        ),
        (true, true, true, true, false),
        "{launch:?}"
    );
}

#[test]
fn the_fingerprint_seed_stays_the_same_across_launches() {
    let stand_in = StandIn::failing("no display");
    let environment = RunEnvironment::new(&stand_in.config());

    login(&environment);
    // A login counts against the route's 10-second spacing; forgetting the first reservation
    // lets the second launch start at once.
    fs::remove_file(environment.state_dir.join("forager/rate_limit_state.json"))
        .expect("reset the access spacing");
    login(&environment);
    let seeds = stand_in
        .launches()
        .into_iter()
        .map(|launch| {
            launch
                .into_iter()
                .find(|argument| argument.starts_with("--fingerprint="))
                .expect("fingerprint seed")
        })
        .collect::<Vec<_>>();

    assert_eq!(seeds.len(), 2);
    assert_eq!(seeds[0], seeds[1]);
}

#[test]
fn a_browser_that_exits_before_the_handshake_is_a_runtime_failure_with_its_stderr() {
    let stand_in = StandIn::failing("cannot open display :0");
    let environment = RunEnvironment::new(&stand_in.config());

    let output = login(&environment);
    let payload = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            payload["error_kind"].clone(),
            payload["message"]
                .as_str()
                .is_some_and(|message| message.contains("cannot open display :0")),
        ),
        (Some(4), json!("runtime"), true),
        "{payload}"
    );
}

#[test]
fn a_failure_message_is_cut_to_500_characters() {
    let stand_in = StandIn::failing(&"x".repeat(600));
    let environment = RunEnvironment::new(&stand_in.config());

    let output = login(&environment);
    let payload = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            payload["message"]
                .as_str()
                .map(|message| message.chars().count()),
        ),
        (Some(4), Some(500)),
        "{payload}"
    );
}

#[test]
fn a_browser_that_never_answers_is_killed_with_its_process_group_at_the_working_deadline() {
    let stand_in = StandIn::hanging();
    let environment = RunEnvironment::new(&stand_in.config());
    let started = Instant::now();

    let output = login(&environment);
    let elapsed = started.elapsed();
    let pids = ["leader.pid", "child.pid"]
        .map(|name| fs::read_to_string(stand_in.path(name)).expect("recorded pid"));
    let deadline = Instant::now() + Duration::from_secs(3);
    while pids.iter().any(|pid| process_exists(pid)) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }

    assert_eq!(
        (
            output.status.code(),
            payload(&output)["error_kind"].clone(),
            pids.iter().any(|pid| process_exists(pid)),
        ),
        (Some(4), json!("timeout"), false)
    );
    assert!(
        elapsed < Duration::from_secs(9),
        "the working deadline stops the command before the command deadline: {elapsed:?}"
    );
}

#[test]
fn a_missing_browser_executable_is_a_runtime_failure() {
    let environment = RunEnvironment::new("[browser]\npath = \"/nonexistent/forager-browser\"\n");

    let output = login(&environment);
    let payload = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            payload["error_kind"].clone(),
            payload["message"]
                .as_str()
                .is_some_and(|message| message.contains("/nonexistent/forager-browser")),
        ),
        (Some(4), json!("runtime"), true),
        "{payload}"
    );
}

#[test]
fn a_profile_another_process_holds_fails_at_once() {
    let stand_in = StandIn::failing("no display");
    let environment = RunEnvironment::new(&stand_in.config());
    let profile = environment.state_dir.join("forager/browser/xiaohongshu");
    fs::create_dir_all(&profile).expect("create profile");
    let lock = fs::File::create(profile.join("profile.lock")).expect("create lock file");
    lock.lock_exclusive().expect("hold the profile lock");

    let output = login(&environment);
    let payload = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            payload["error_kind"].clone(),
            payload["message"]
                .as_str()
                .is_some_and(|message| message.contains("in use by another forager process")),
            stand_in.launches().len(),
        ),
        (Some(4), json!("runtime"), true, 0),
        "{payload}"
    );
}

#[test]
fn login_for_a_platform_without_a_browser_route_is_an_argument_error() {
    let environment = RunEnvironment::new("");

    let output = environment.run(&["browser", "login", "arxiv"]);

    assert_eq!(
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).contains("supports: xiaohongshu"),
        ),
        (Some(2), true),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn browser_path_comes_from_the_environment_by_the_naming_convention() {
    let stand_in = StandIn::failing("no display");
    let environment = RunEnvironment::new("");

    environment.run_with_env(
        &["browser", "login", "xiaohongshu", "--timeout", "10"],
        &[(
            "FORAGER_BROWSER__PATH",
            &stand_in.executable().display().to_string(),
        )],
    );

    assert_eq!(stand_in.launches().len(), 1);
}

#[test]
fn browser_path_expands_a_leading_tilde_to_home() {
    let stand_in = StandIn::failing("no display");
    let environment = RunEnvironment::new("[browser]\npath = \"~/bin/browser\"\n");
    let home = environment
        .config_dir
        .parent()
        .expect("isolated root")
        .to_path_buf();
    fs::create_dir_all(home.join("bin")).expect("create bin directory");
    std::os::unix::fs::symlink(stand_in.executable(), home.join("bin/browser"))
        .expect("link the stand-in");

    login(&environment);

    assert_eq!(stand_in.launches().len(), 1);
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
fn login_without_the_managed_browser_asks_to_install_it() {
    let environment = RunEnvironment::new("");

    let output = login(&environment);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        (
            output.status.code(),
            stderr.contains("config_error"),
            stderr.contains("forager browser install")
        ),
        (Some(3), true, true),
        "{stderr}"
    );
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
#[test]
fn login_on_a_host_without_a_package_points_at_browser_path() {
    let environment = RunEnvironment::new("");

    let output = login(&environment);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        (output.status.code(), stderr.contains("set `browser.path`")),
        (Some(3), true),
        "{stderr}"
    );
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
#[test]
fn install_on_a_host_without_a_package_is_a_runtime_failure() {
    let environment = RunEnvironment::new("");

    let output = environment.run(&["browser", "install"]);

    assert_eq!(
        (output.status.code(), payload(&output)["error_kind"].clone()),
        (Some(4), json!("runtime"))
    );
}
