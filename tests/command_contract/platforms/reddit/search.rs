//! The `reddit_browser` search against a shell stand-in for the browser: the preflight before
//! the route chain, the enabling steps, and how one search launches the browser. The stand-in
//! speaks no CDP, so no test here reaches a page: the judgement of what the page projection
//! shows is unit-tested with the route, the projection script runs on local pages in the
//! real-browser tests, and only the live smoke C50 runs the search on the site.
#![cfg(unix)]

use std::process::Output;

use serde_json::json;

use crate::support::browser::{StandIn, mark_logged_in};
use crate::support::{RunEnvironment, payload, stderr};

const ENABLED: &str = "[\"reddit_browser\"]";

fn search(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "reddit", "search", "async runtime"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

#[test]
fn a_search_runs_one_headless_english_browser_command_on_the_reddit_profile() {
    let stand_in = StandIn::failing("no CDP here");
    let environment = RunEnvironment::new(&stand_in.reddit_config(ENABLED));
    mark_logged_in(&environment.state_dir, "reddit");
    let profile = environment.state_dir.join("forager/browser/reddit");

    let output = search(&environment, &["--subreddit", "rust", "--sort", "new"]);
    let payload = payload(&output);
    let launches = stand_in.launches();
    let launch = launches.first().cloned().unwrap_or_default();
    let has = |argument: &str| launch.iter().any(|launched| launched == argument);

    assert_eq!(
        (
            output.status.code(),
            payload["error_kind"].clone(),
            payload["attempts"]["providers"].clone(),
            launches.len(),
        ),
        (Some(4), json!("runtime"), json!(["reddit_browser"]), 1),
        "{payload}"
    );
    assert_eq!(
        [
            &format!("--user-data-dir={}", profile.join("user-data").display()),
            "--lang=en-US",
            "--accept-lang=en-US,en",
            "--headless=new",
        ]
        .map(has),
        [true; 4],
        "{launch:?}"
    );
}

#[test]
fn a_browser_that_never_answers_is_one_timed_out_attempt() {
    let stand_in = StandIn::hanging();
    let environment = RunEnvironment::new(&format!(
        "{}\n[providers.reddit_browser]\ntimeout = 10\n",
        stand_in.reddit_config(ENABLED)
    ));
    mark_logged_in(&environment.state_dir, "reddit");

    let output = search(&environment, &[]);

    assert_eq!(
        (
            output.status.code(),
            payload(&output)["error_kind"].clone(),
            stand_in.launches().len(),
        ),
        (Some(4), json!("timeout"), 1),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn a_route_that_cannot_run_fails_the_preflight_without_launching_the_browser() {
    let stand_in = StandIn::failing("must not start");
    let not_logged_in = RunEnvironment::new(&stand_in.reddit_config(ENABLED));
    let not_installed = RunEnvironment::new(&format!("[platforms.reddit]\norder = {ENABLED}\n"));
    mark_logged_in(&not_installed.state_dir, "reddit");

    let outcomes = [not_logged_in, not_installed].map(|environment| {
        let output = search(&environment, &[]);
        (output.status.code(), stderr(&output))
    });

    assert_eq!(stand_in.launches().len(), 0);
    let [(login_code, login), (install_code, install)] = outcomes;
    assert_eq!((login_code, install_code), (Some(3), Some(3)));
    assert!(
        login.contains(
            "reddit_browser cannot run: the reddit Browser Profile has never been logged in; run `forager browser login reddit`"
        ),
        "{login}"
    );
    // Only macOS arm64 has a Managed Browser package; elsewhere the fix is `browser.path`.
    let install_hint = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "`forager browser install`"
    } else {
        "`browser.path`"
    };
    assert!(
        install.contains("reddit_browser cannot run: ") && install.contains(install_hint),
        "{install}"
    );
}

#[test]
fn the_default_empty_order_refuses_the_search_with_the_steps_and_the_account_risk() {
    let stand_in = StandIn::failing("must not start");
    let environment = RunEnvironment::new(&stand_in.config());

    let output = search(&environment, &[]);
    let stderr = stderr(&output);

    assert_eq!(
        (
            output.status.code(),
            stand_in.launches().len(),
            stderr.contains("platforms.reddit.order") && stderr.contains("reddit_browser"),
        ),
        (Some(3), 0, true),
        "{stderr}"
    );
}
