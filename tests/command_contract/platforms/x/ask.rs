//! `forager platform x ask` against a local xAI Responses fixture that replays the stored
//! answer.

use serde_json::json;

use crate::support::x::{completed, config, fixture};
use crate::support::{Fixture, RunEnvironment, payload};

const KEY: &str = "test-xai-key-a";

#[test]
fn verbose_output_lists_the_platform_attempt() {
    let fixture = Fixture::start_sequence(vec![completed(&fixture("x-ask.json"))]);
    let environment = RunEnvironment::new(&config(&fixture.url, &[KEY]));

    let output = environment.run(&["platform", "x", "ask", "Starship news", "--verbose"]);
    fixture.finish_all();
    let answer = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &answer["platform"],
            &answer["provider"],
            &answer["query"],
            &answer["provider_attempts"][0]["provider"],
            &answer["provider_attempts"][0]["operation"],
        ),
        (
            Some(0),
            &json!("x"),
            &json!("xai"),
            &json!("Starship news"),
            &json!("xai"),
            &json!("ask")
        ),
        "{answer}"
    );
}
