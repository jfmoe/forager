//! A configuration or command left from `reddit_web`, the anonymous HTTP route that
//! `reddit_browser` replaced (ADR-0037), fails before any request: the configuration checks
//! reject the old route and its section, and the arguments no longer have the old cursor.

mod support;

use std::process::Output;

use support::RunEnvironment;

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn an_order_that_names_reddit_web_is_a_configuration_error() {
    let environment = RunEnvironment::new("[platforms.reddit]\norder = [\"reddit_web\"]\n");

    let output = environment.run(&["platform", "reddit", "search", "rust"]);

    assert_eq!(output.status.code(), Some(3), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("invalid value for `platforms.reddit.order`"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_reddit_web_section_is_a_configuration_error() {
    let environment = RunEnvironment::new("[providers.reddit_web]\ntimeout = 30\n");

    let output = environment.run(&["platform", "reddit", "search", "rust"]);

    assert_eq!(output.status.code(), Some(3), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("unknown field `reddit_web`"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn doctor_does_not_know_reddit_web() {
    let environment = RunEnvironment::new("");

    let output = environment.run(&["doctor", "--provider", "reddit_web"]);

    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("invalid provider `reddit_web`"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn reddit_search_has_no_cursor() {
    let environment = RunEnvironment::new("[platforms.reddit]\norder = [\"reddit_browser\"]\n");

    let output = environment.run(&[
        "platform",
        "reddit",
        "search",
        "rust",
        "--cursor",
        "v1.reddit_web.e30",
    ]);

    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("unexpected argument '--cursor'"),
        "{}",
        stderr(&output)
    );
}
