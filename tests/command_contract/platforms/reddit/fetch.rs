//! The `reddit_browser` fetch against a shell stand-in for the browser. The launch, the read
//! deadline, and the enabling steps are the search's, one browser transport; this proves that a
//! fetch passes the same preflight. The judgement of the post page projection is unit-tested
//! with the route, the projection script runs on local pages in the real-browser tests, and only
//! the live smoke C51 reads a post on the site.
#![cfg(unix)]

use crate::support::RunEnvironment;
use crate::support::browser::StandIn;

#[test]
fn a_fetch_with_a_profile_that_never_logged_in_launches_no_browser() {
    let stand_in = StandIn::failing("must not start");
    let environment = RunEnvironment::new(&stand_in.reddit_config("[\"reddit_browser\"]"));

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
