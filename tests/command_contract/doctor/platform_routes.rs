use serde_json::Value;

use crate::support::doctor::{SHALLOW_PROVIDERS, reachable_responses, shallow_config};
use crate::support::{Fixture, RunEnvironment};

#[test]
fn shallow_doctor_reaches_serpapi_with_a_keyless_get() {
    let fixture = Fixture::start_sequence(reachable_responses(SHALLOW_PROVIDERS.len() + 1));
    let environment = RunEnvironment::new(&format!(
        "{}\n[providers.serpapi]\nurl = \"{}/search.json\"\nkeys = [\"test-serpapi-key\"]\n",
        shallow_config(&fixture.url),
        fixture.url
    ));

    let output = environment.run(&["doctor"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");
    let serpapi = payload["providers"]
        .as_array()
        .expect("providers")
        .iter()
        .find(|provider| provider["provider"] == "serpapi")
        .expect("serpapi status");
    let requests = fixture.finish_all();

    assert_eq!(
        (
            output.status.code(),
            &serpapi["configured"],
            &serpapi["reachable"],
            requests
                .iter()
                .filter_map(|request| request.lines().next())
                .filter(|line| line.contains("/search.json"))
                .collect::<Vec<_>>(),
        ),
        (
            Some(0),
            &Value::Bool(true),
            &Value::Bool(true),
            vec!["GET /search.json HTTP/1.1"],
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn route_status<'a>(payload: &'a Value, route: &str) -> &'a Value {
    payload["providers"]
        .as_array()
        .expect("providers")
        .iter()
        .find(|provider| provider["provider"] == route)
        .unwrap_or_else(|| panic!("{route} status"))
}

/// Runs a shallow doctor where every HTTP provider is reachable, `youtube_web` among them when
/// the YouTube order lists it.
fn shallow_with_youtube_web(enabled: bool) -> (Value, usize) {
    let requests = SHALLOW_PROVIDERS.len() + usize::from(enabled);
    let fixture = Fixture::start_sequence(reachable_responses(requests));
    let order = if enabled {
        "[\"youtube_api\", \"youtube_web\"]"
    } else {
        "[\"youtube_api\"]"
    };
    let environment = RunEnvironment::new(&format!(
        "{}\n[providers.youtube_web]\nurl = {:?}\n\n[platforms.youtube]\norder = {order}\n",
        shallow_config(&fixture.url),
        fixture.url
    ));
    let output = environment.run(&["doctor"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");
    (
        route_status(&payload, "youtube_web").clone(),
        fixture.finish_all().len(),
    )
}

#[test]
fn shallow_doctor_reaches_youtube_web_only_while_an_order_enables_it() {
    let (disabled, disabled_requests) = shallow_with_youtube_web(false);
    let (enabled, enabled_requests) = shallow_with_youtube_web(true);

    assert_eq!(
        (
            &disabled["configured"],
            &disabled["reachable"],
            disabled_requests,
            &enabled["configured"],
            &enabled["reachable"],
            enabled_requests,
        ),
        (
            &Value::Bool(false),
            &Value::Bool(false),
            SHALLOW_PROVIDERS.len(),
            &Value::Bool(true),
            &Value::Bool(true),
            SHALLOW_PROVIDERS.len() + 1,
        )
    );
}

#[test]
fn youtube_web_provider_doctor_refuses_the_route_while_no_order_enables_it() {
    let fixture = Fixture::start_canary();
    let environment = RunEnvironment::new(&format!(
        "[providers.youtube_web]\nurl = {:?}\n",
        fixture.url
    ));

    let output = environment.run(&["doctor", "--provider", "youtube_web"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            &payload["message"],
            fixture.finish_all().len()
        ),
        (
            Some(3),
            &Value::String("config".into()),
            &Value::String(
                "no platform order lists `youtube_web`; add it to the order to enable it".into()
            ),
            0
        )
    );
}

/// The browser route against a shell stand-in for the browser, which speaks no CDP.
#[cfg(unix)]
mod xiaohongshu_browser {
    use serde_json::{Value, json};

    use super::route_status;
    use crate::support::browser::{StandIn, mark_logged_in};
    use crate::support::doctor::{SHALLOW_PROVIDERS, reachable_responses, shallow_config};
    use crate::support::{Fixture, RunEnvironment};

    fn shallow_with_xiaohongshu(order: &str, logged_in: bool) -> (Option<i32>, Value, usize) {
        let fixture = Fixture::start_sequence(reachable_responses(SHALLOW_PROVIDERS.len()));
        let stand_in = StandIn::failing("must not start");
        let environment = RunEnvironment::new(&format!(
            "{}\n{}",
            shallow_config(&fixture.url),
            stand_in.xiaohongshu_config(order)
        ));
        if logged_in {
            mark_logged_in(&environment.state_dir, "xiaohongshu");
        }

        let output = environment.run(&["doctor"]);
        let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");
        fixture.finish_all();
        (
            output.status.code(),
            route_status(&payload, "xiaohongshu_browser").clone(),
            stand_in.launches().len(),
        )
    }

    #[test]
    fn shallow_doctor_checks_the_xiaohongshu_route_with_its_preflight_alone() {
        let enabled = "[\"xiaohongshu_browser\"]";

        let [ready, not_logged_in, disabled] = [
            shallow_with_xiaohongshu(enabled, true),
            shallow_with_xiaohongshu(enabled, false),
            shallow_with_xiaohongshu("[]", true),
        ]
        .map(|(code, status, launches)| {
            (
                code,
                status["configured"].clone(),
                status["reachable"].clone(),
                status["message"].clone(),
                launches,
            )
        });

        assert_eq!(
            [ready, not_logged_in, disabled],
            [
                (Some(0), json!(true), json!(true), Value::Null, 0),
                (
                    Some(4),
                    json!(true),
                    json!(false),
                    json!(
                        "the xiaohongshu Browser Profile has never been logged in; run `forager browser login xiaohongshu`"
                    ),
                    0
                ),
                (Some(0), json!(false), json!(false), Value::Null, 0),
            ]
        );
    }

    #[test]
    fn xiaohongshu_browser_provider_doctor_refuses_a_route_that_cannot_run() {
        let stand_in = StandIn::failing("must not start");
        let disabled = RunEnvironment::new(&stand_in.xiaohongshu_config("[]"));
        let not_logged_in =
            RunEnvironment::new(&stand_in.xiaohongshu_config("[\"xiaohongshu_browser\"]"));

        let outcomes = [disabled, not_logged_in].map(|environment| {
            let output = environment.run(&["doctor", "--provider", "xiaohongshu_browser"]);
            let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");
            (
                output.status.code(),
                payload["error_kind"].clone(),
                payload["message"].clone(),
            )
        });

        assert_eq!(
            (outcomes, stand_in.launches().len()),
            (
                [
                    (
                        Some(3),
                        json!("config"),
                        json!(
                            "no platform order lists `xiaohongshu_browser`; add it to the order to enable it"
                        )
                    ),
                    (
                        Some(3),
                        json!("config"),
                        json!(
                            "the xiaohongshu Browser Profile has never been logged in; run `forager browser login xiaohongshu`"
                        )
                    ),
                ],
                0
            )
        );
    }
}
