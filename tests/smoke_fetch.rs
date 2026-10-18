//! Live smoke of the Web Fetch cases: a case proves its provider's keys, not the Anonymous
//! Fallback behind them.

mod support;

use serde_json::Value;
use support::smoke::{DISABLED_PLATFORMS, SmokeEnvironment, case};
use support::{Fixture, Response, jina_response};

#[test]
fn the_jina_case_fails_when_only_the_anonymous_request_reads_the_page() {
    let refused = || Response::json(401, r#"{"message":"Invalid API key"}"#);
    let anonymous = || Response::json(200, &jina_response(&"Smoke canary content. ".repeat(30)));
    // Live smoke runs a failing case three times; each run sends the key, then no key.
    let jina = Fixture::start_sequence(
        (0..3)
            .flat_map(|_| [refused(), anonymous()])
            .collect::<Vec<_>>(),
    );
    let environment = SmokeEnvironment::new(|journal_dir| {
        format!(
            "[providers.jina]\nurl = {:?}\nkeys = [\"jina-key\"]\n[journal]\ndir = {journal_dir:?}\n{DISABLED_PLATFORMS}",
            jina.url
        )
    });

    let output = environment.run(&["smoke", "--live", "--timeout", "20"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse live smoke JSON");

    assert_eq!(
        case(&payload, "C07")["status"],
        "failed",
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    jina.finish_all();
}
