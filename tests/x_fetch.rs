//! `forager platform x fetch` against a local xAI Responses fixture that replays the stored
//! thread response.

mod support;

use support::x::{completed, config, fixture};
use support::{Fixture, RunEnvironment};

const KEY: &str = "test-xai-key-a";
const ROOT: &str = "x:2110126570233094527";

#[test]
fn content_format_prints_the_thread_as_markdown() {
    let fixture = Fixture::start_sequence(vec![completed(&fixture("x-thread.json"))]);
    let environment = RunEnvironment::new(&config(&fixture.url, &[KEY]));

    let output = environment.run(&["platform", "x", "fetch", ROOT, "--format", "content"]);
    fixture.finish_all();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(
        (
            output.status.code(),
            stdout
                .starts_with("- **@SpaceX** (root, 2026-10-13T21:52:33Z) `x:2110126570233094527`"),
            stdout.contains(
                "- **@BassonBrain** (reply, 2026-10-13T22:41:44Z) `x:2110138949234487662`\n\n  Wow!"
            ),
        ),
        (Some(0), true, true),
        "stdout: {stdout}"
    );
}
