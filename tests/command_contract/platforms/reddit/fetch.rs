//! The `reddit_browser` fetch against a shell stand-in for the browser. The launch arguments,
//! the read deadline, and the enabling steps are the search's, one browser transport; this
//! proves that a fetch passes the same preflight and reaches the browser on the Reddit profile.
//! The judgement of the post page projection is unit-tested with the route, the projection
//! script runs on local pages in the real-browser tests, and only the Live Check
//! `reddit_browser:fetch` reads a post on the site.
#![cfg(unix)]

use serde_json::json;

use crate::support::browser::{StandIn, mark_logged_in};
use crate::support::{RunEnvironment, payload};

const ENABLED: &str = "[\"reddit_browser\"]";

#[test]
fn a_fetch_with_a_logged_in_profile_launches_one_browser_on_that_profile() {
    let stand_in = StandIn::failing("no CDP here");
    let environment = RunEnvironment::new(&stand_in.reddit_config(ENABLED));
    mark_logged_in(&environment.state_dir, "reddit");
    let user_data = environment
        .state_dir
        .join("forager/browser/reddit/user-data");

    let output = environment.run(&["platform", "reddit", "fetch", "reddit:1q2w3e4"]);
    let payload = payload(&output);
    let launches = stand_in.launches();

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
    assert!(
        launches[0].contains(&format!("--user-data-dir={}", user_data.display())),
        "{launches:?}"
    );
}

#[test]
fn a_fetch_with_a_profile_that_never_logged_in_launches_no_browser() {
    let stand_in = StandIn::failing("must not start");
    let environment = RunEnvironment::new(&stand_in.reddit_config(ENABLED));

    let output = environment.run(&["platform", "reddit", "fetch", "reddit:1q2w3e4"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        (
            output.status.code(),
            stand_in.launches().len(),
            stderr.contains(
                "reddit_browser cannot run: the reddit Browser Profile has never been logged in; run `forager browser login reddit`"
            ),
        ),
        (Some(3), 0, true),
        "{stderr}"
    );
}
