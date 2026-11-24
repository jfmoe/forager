//! The `xiaohongshu_browser` fetch against a shell stand-in for the browser: the access token
//! rules of the input and the browser launch. The stand-in speaks no CDP, so no test here
//! reaches a page: the note judgement and the full text are unit-tested with the route, and the
//! page logic itself only the live smoke C28 runs.
#![cfg(unix)]

use std::process::Output;

use crate::support::RunEnvironment;
use crate::support::browser::{StandIn, mark_logged_in};

const NOTE_ID: &str = "66f0a1b2c3d4e5f607100001";
const TOKEN: &str = "ABfakeTOKENcanary4242=";
/// The token without its padding, which no URL encoding changes.
const TOKEN_BODY: &str = "ABfakeTOKENcanary4242";

fn environment(stand_in: &StandIn) -> RunEnvironment {
    let environment =
        RunEnvironment::new(&stand_in.xiaohongshu_config("[\"xiaohongshu_browser\"]"));
    mark_logged_in(&environment.state_dir, "xiaohongshu");
    environment
}

fn fetch(environment: &RunEnvironment, input: &str) -> Output {
    environment.run(&["platform", "xiaohongshu", "fetch", input, "--verbose"])
}

fn printed(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn a_note_without_an_access_token_fails_before_starting_the_browser() {
    let stand_in = StandIn::failing("must not start");

    let output = fetch(&environment(&stand_in), &format!("xiaohongshu:{NOTE_ID}"));

    assert_eq!(
        (
            output.status.code(),
            stand_in.launches().len(),
            printed(&output).contains("`access_url`"),
        ),
        (Some(2), 0, true),
        "{}",
        printed(&output)
    );
}

#[test]
fn the_access_token_never_reaches_the_browser_command_line_or_the_output() {
    let stand_in = StandIn::failing("no CDP here");
    let environment = environment(&stand_in);

    let opened = fetch(
        &environment,
        &format!(
            "https://www.xiaohongshu.com/explore/{NOTE_ID}?xsec_token={TOKEN}&xsec_source=pc_search"
        ),
    );
    let launches = stand_in.launches();

    assert_eq!(
        (
            opened.status.code(),
            launches.len(),
            launches.concat().join(" ").contains(TOKEN_BODY),
        ),
        (Some(4), 1, false),
        "{launches:?}"
    );
    assert!(
        !printed(&opened).contains(TOKEN_BODY),
        "{}",
        printed(&opened)
    );
}
