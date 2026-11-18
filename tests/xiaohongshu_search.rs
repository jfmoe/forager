//! The `xiaohongshu_browser` search against a shell stand-in for the browser: the preflight
//! before the route chain, the enabling steps, and how one search launches the browser. The
//! stand-in speaks no CDP, so no test here reaches a page: the judgement of what the page logic
//! observes is unit-tested with the route, and the page logic itself only the live smoke C27
//! runs.
#![cfg(unix)]

mod support;

use std::fs;
use std::process::Output;

use serde_json::{Value, json};

use support::RunEnvironment;
use support::browser::{StandIn, mark_logged_in};

const ENABLED: &str = "[\"xiaohongshu_browser\"]";

fn payload(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "parse JSON stdout: {error}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn search(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "xiaohongshu", "search", "咖啡"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

#[test]
fn a_search_runs_one_headless_browser_command_on_the_logged_in_profile() {
    let stand_in = StandIn::failing("no CDP here");
    let environment = RunEnvironment::new(&stand_in.xiaohongshu_config(ENABLED));
    mark_logged_in(&environment.state_dir, "xiaohongshu");
    let profile = environment.state_dir.join("forager/browser/xiaohongshu");
    // The screen a headed login recorded, in CSS pixels.
    fs::write(
        profile.join("profile.json"),
        r#"{"version":1,"seed":7,"screen":{"width":1512,"height":982,"avail_top":25,"avail_height":944,"device_pixel_ratio":2.0,"window_width":1200,"window_height":800}}"#,
    )
    .expect("write the profile identity");

    let output = search(&environment, &["--verbose"]);
    let payload = payload(&output);
    let launches = stand_in.launches();
    let launch = launches.first().cloned().unwrap_or_default();
    let has = |argument: &str| launch.iter().any(|launched| launched == argument);

    assert_eq!(
        (
            output.status.code(),
            payload["error_kind"].clone(),
            payload["message"]
                .as_str()
                .is_some_and(|message| message.contains("no CDP here")),
            payload["provider_attempts"]
                .as_array()
                .map(|attempts| attempts
                    .iter()
                    .map(|attempt| attempt["transport"].clone())
                    .collect::<Vec<_>>()),
            launches.len(),
        ),
        (
            Some(4),
            json!("runtime"),
            true,
            Some(vec![json!("browser")]),
            1
        ),
        "{payload}"
    );
    assert_eq!(
        [
            &format!("--user-data-dir={}", profile.join("user-data").display()),
            "--fingerprint=7",
            "--lang=zh-CN",
            "--headless=new",
            "--screen-info={0,0 3024x1964 devicePixelRatio=2 workAreaTop=50 workAreaBottom=26}",
            "--window-size=1200,800",
        ]
        .map(has),
        [true; 6],
        "{launch:?}"
    );
}

#[test]
fn a_browser_that_never_answers_is_one_timed_out_attempt() {
    let stand_in = StandIn::hanging();
    let environment = RunEnvironment::new(&format!(
        "{}\n[providers.xiaohongshu_browser]\ntimeout = 10\n",
        stand_in.xiaohongshu_config(ENABLED)
    ));
    mark_logged_in(&environment.state_dir, "xiaohongshu");

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
    let not_logged_in = RunEnvironment::new(&stand_in.xiaohongshu_config(ENABLED));
    let not_installed =
        RunEnvironment::new(&format!("[platforms.xiaohongshu]\norder = {ENABLED}\n"));
    mark_logged_in(&not_installed.state_dir, "xiaohongshu");

    let outcomes = [not_logged_in, not_installed].map(|environment| {
        let output = search(&environment, &[]);
        (output.status.code(), stderr(&output))
    });

    assert_eq!(stand_in.launches().len(), 0);
    let [(login_code, login), (install_code, install)] = outcomes;
    assert_eq!((login_code, install_code), (Some(3), Some(3)));
    assert!(
        login.contains(
            "xiaohongshu_browser cannot run: the xiaohongshu Browser Profile has never been logged in; run `forager browser login xiaohongshu`"
        ),
        "{login}"
    );
    assert!(
        install.contains("xiaohongshu_browser cannot run: ")
            && install.contains("`forager browser install`"),
        "{install}"
    );
}

#[test]
fn the_default_empty_order_refuses_the_search_with_the_steps_to_enable_it() {
    let stand_in = StandIn::failing("must not start");
    let environment = RunEnvironment::new(&stand_in.config());

    let output = search(&environment, &[]);

    assert_eq!(
        (output.status.code(), stand_in.launches().len()),
        (Some(3), 0),
        "{}",
        stderr(&output)
    );
    assert!(
        stderr(&output).contains(
            "platforms.xiaohongshu.order has no configured route for xiaohongshu search; to use xiaohongshu_browser, run `forager browser install`, run `forager browser login xiaohongshu` and log in, then add `xiaohongshu_browser` to platforms.xiaohongshu.order"
        ),
        "{}",
        stderr(&output)
    );
}
