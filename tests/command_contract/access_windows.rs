use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{fs, thread};

use serde_json::{Value, json};

use crate::support::doctor::{reachable_responses, shallow_config};
use crate::support::{Fixture, Response, RunEnvironment};

/// One send of each HTTP route that has an Access Policy, and the configuration that reaches
/// it. The platform checklist requires a row for every such route.
const SENDS: &[(&str, &str, &[&str])] = &[
    (
        "arxiv_api",
        "[providers.arxiv_api]\nurl = \"{url}\"\n",
        &["platform", "arxiv", "search", "x"],
    ),
    (
        "ssrn_crossref",
        "[providers.ssrn_crossref]\nurl = \"{url}\"\n",
        &["platform", "ssrn", "search", "x"],
    ),
    (
        "hn_api",
        "[providers.hn_api]\nurl = \"{url}\"\nfirebase_url = \"{url}\"\n",
        &["platform", "hn", "search", "x"],
    ),
    (
        "polymarket_api",
        "[providers.polymarket_api]\nurl = \"{url}\"\n",
        &["platform", "polymarket", "search", "x"],
    ),
    (
        "youtube_api",
        "[providers.youtube_api]\nurl = \"{url}\"\nkeys = [\"k\"]\n\
         [platforms.youtube]\norder = [\"youtube_api\"]\n",
        &["platform", "youtube", "search", "x"],
    ),
    (
        "youtube_web",
        "[providers.youtube_web]\nurl = \"{url}\"\n[platforms.youtube]\norder = [\"youtube_web\"]\n",
        &["platform", "youtube", "fetch", "youtube:jNQXAC9IVRw"],
    ),
    (
        "gemini_api",
        "[providers.gemini_api]\nurl = \"{url}\"\nkeys = [\"k\"]\n",
        &["doctor", "--provider", "gemini_api"],
    ),
];

fn epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("wall clock")
        .as_millis()
}

fn state_path(environment: &RunEnvironment) -> std::path::PathBuf {
    environment.state_dir.join("forager/rate_limit_state.json")
}

/// Records reservations a minute ahead, as if another process had just reserved each window.
fn reserve_windows(environment: &RunEnvironment, routes: &[&str]) {
    let reserved_at_ms = epoch_ms() + Duration::from_mins(1).as_millis();
    let routes = routes
        .iter()
        .map(|route| {
            (
                (*route).to_owned(),
                json!({"reserved_at_ms": reserved_at_ms}),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let path = state_path(environment);
    fs::create_dir_all(path.parent().expect("state directory")).expect("create state directory");
    fs::write(
        path,
        json!({"schema_version": 1, "routes": routes}).to_string(),
    )
    .expect("write rate limit state");
}

#[test]
fn every_route_with_an_access_policy_waits_for_its_window_before_sending() {
    let mut sent = Vec::new();
    for (route, config, arguments) in SENDS {
        let fixture = Fixture::start_canary();
        let environment = RunEnvironment::new(&config.replace("{url}", &fixture.url));
        reserve_windows(&environment, &[route]);

        let mut arguments = arguments.to_vec();
        arguments.extend(["--timeout", "1"]);
        let output = environment.run(&arguments);
        let requests = fixture.finish_all().len();
        if output.status.code() != Some(4) || requests != 0 {
            sent.push(format!(
                "{route}: exit {:?}, {requests} requests\n{}",
                output.status.code(),
                String::from_utf8_lossy(&output.stdout)
            ));
        }
    }

    assert!(
        sent.is_empty(),
        "these routes sent inside a reserved window: {sent:#?}"
    );
}

#[test]
fn a_retry_waits_for_the_window_again() {
    let held = Fixture::start_held(Response::json(503, "{}"));
    let environment = RunEnvironment::new(&format!(
        "[providers.hn_api]\nurl = \"{url}\"\nfirebase_url = \"{url}\"\n\
         [retry]\nmax_attempts = 2\nmultiplier = 1\nmax_wait = 0\n",
        url = held.url
    ));

    // The first request is in flight, so its window is reserved; another process then reserves
    // the next one before the retry asks for it.
    let (output, fixture) = thread::scope(|scope| {
        let run =
            scope.spawn(|| environment.run(&["platform", "hn", "search", "x", "--timeout", "5"]));
        held.wait_for_request();
        reserve_windows(&environment, &["hn_api"]);
        let fixture = held.release();
        (run.join().expect("run the search"), fixture)
    });
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse search JSON");

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            fixture.finish_all().len()
        ),
        (Some(4), &Value::String("timeout".into()), 1),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn shallow_doctor_waits_for_every_reserved_window() {
    let paced = ["arxiv_api", "ssrn_crossref", "hn_api", "polymarket_api"];
    let fixture = Fixture::start_sequence(reachable_responses(5));
    let environment = RunEnvironment::new(&shallow_config(&fixture.url));
    reserve_windows(&environment, &paced);

    let output = environment.run(&["doctor", "--timeout", "1"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");
    let reachable = payload["providers"]
        .as_array()
        .expect("providers")
        .iter()
        .filter(|provider| paced.contains(&provider["provider"].as_str().unwrap_or_default()))
        .map(|provider| {
            (
                provider["provider"].as_str().unwrap_or_default().to_owned(),
                provider["reachable"] == true,
            )
        })
        .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(
        (reachable, fixture.finish_all().len()),
        (paced.map(|route| (route.to_owned(), false)).into(), 5)
    );
}
