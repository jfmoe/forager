//! Live smoke with `--opt-in`: a platform case whose route the order leaves out runs when this
//! machine meets the route's prerequisites, and otherwise names the one it misses.

use crate::support::browser::StandIn;
use crate::support::smoke::{DISABLED_PLATFORMS, SmokeEnvironment, anonymous_routes_at, case};
use crate::support::{Fixture, Response};
use serde_json::Value;

fn refusing() -> Fixture {
    Fixture::start_repeating(Response::json(401, "{}"))
}

/// Runs live smoke under `config` and returns the status and message of each case in `ids`.
// Debug path formatting supplies the quoted and escaped TOML string literal required by fixtures.
#[expect(clippy::unnecessary_debug_formatting)]
fn run<const N: usize>(config: &str, arguments: &[&str], ids: [&str; N]) -> [(Value, Value); N] {
    let environment =
        SmokeEnvironment::new(|journal_dir| format!("{config}[journal]\ndir = {journal_dir:?}\n"));
    let output = environment.run(&[&["smoke", "--live", "--timeout", "90"], arguments].concat());
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse live smoke JSON");
    ids.map(|id| {
        let case = case(&payload, id);
        (case["status"].clone(), case["message"].clone())
    })
}

#[test]
fn opt_in_runs_a_platform_case_whose_route_the_order_leaves_out() {
    let refusing = refusing();
    let youtube = Fixture::start_sequence_for(|origin| {
        vec![
            crate::support::youtube_web::watch_page(),
            crate::support::youtube_web::player(origin),
            crate::support::youtube_web::srv1(),
        ]
    });
    let config = format!(
        "{}[platforms.youtube]\norder = [\"youtube_api\"]\n",
        anonymous_routes_at(&refusing.url, &youtube.url)
    );

    let [without] = run(&format!("{config}{DISABLED_PLATFORMS}"), &[], ["C38"]);
    let [with] = run(&config, &["--opt-in"], ["C38"]);
    let requests = youtube.finish_all();

    assert_eq!(
        (without, with, requests[0].starts_with("GET /watch?v=jNQXAC9IVRw ")),
        (
            (
                Value::String("unconfigured".into()),
                Value::String(
                    "platforms.youtube.order does not list `youtube_web`; pass --opt-in to run it where its prerequisites are met".into()
                ),
            ),
            (Value::String("passed".into()), Value::Null),
            true,
        )
    );
}

#[test]
fn opt_in_names_the_keys_a_left_out_route_misses() {
    let refusing = refusing();
    let config = format!(
        "{}[platforms.scholar]\norder = []\n",
        anonymous_routes_at(&refusing.url, &refusing.url)
    );

    let cases = run(&config, &["--opt-in"], ["C24"]);

    assert_eq!(
        cases,
        [(
            Value::String("unconfigured".into()),
            Value::String("providers.serpapi.keys has no configured credentials".into()),
        )]
    );
}

#[test]
fn opt_in_names_a_browser_profile_never_logged_in_without_launching_the_browser() {
    let refusing = refusing();
    let stand_in = StandIn::failing("must not start");
    let config = format!(
        "{}{}",
        anonymous_routes_at(&refusing.url, &refusing.url),
        stand_in.config()
    );

    let cases = run(&config, &["--opt-in"], ["C27"]);

    assert_eq!(
        (cases, stand_in.launches().len()),
        (
            [(
                Value::String("unconfigured".into()),
                Value::String(
                    "the xiaohongshu Browser Profile has never been logged in; run `forager browser login xiaohongshu`".into()
                ),
            )],
            0,
        )
    );
}

#[test]
fn opt_in_names_a_missing_managed_browser() {
    let refusing = refusing();

    let [(status, message)] = run(
        &anonymous_routes_at(&refusing.url, &refusing.url),
        &["--opt-in"],
        ["C27"],
    );

    // Only macOS arm64 has a Managed Browser package; elsewhere the fix is `browser.path`.
    let hint = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "`forager browser install`"
    } else {
        "`browser.path`"
    };
    let message = message.as_str().unwrap_or_default();
    assert_eq!(
        (
            status,
            message.contains("fingerprint-chromium") && message.contains(hint)
        ),
        (Value::String("unconfigured".into()), true),
        "{message}"
    );
}

#[test]
fn opt_in_keeps_the_configured_order_for_a_model_operation() {
    let refusing = refusing();
    let gemini = Fixture::start_repeating(Response::json(500, "{}"));
    let config = format!(
        "{}[providers.gemini_api]\nurl = \"{}\"\nkeys = [\"test-gemini-key\"]\n[platforms.youtube]\norder = [\"youtube_api\"]\n",
        anonymous_routes_at(&refusing.url, &refusing.url),
        gemini.url
    );

    let cases = run(&config, &["--opt-in"], ["C41", "C42"]);

    assert_eq!(
        (cases, gemini.finish_all().len()),
        (
            [(); 2].map(|()| (
                Value::String("unconfigured".into()),
                Value::String(
                    "platforms.youtube.order does not list `gemini_api`; a model operation runs with the configured order only".into()
                ),
            )),
            0,
        )
    );
}
