//! `forager doctor --live` through the binary: each Live Check runs as a child process of the
//! binary, so what the selection, the route pins and the report decide is proven in the crate,
//! and these tests prove the outer path, the exit codes and what reaches the child's
//! environment.

use serde_json::{Value, json};

use crate::support::samples::{sample, sample_json};
use crate::support::web_samples::jina;
use crate::support::{Fixture, Response, RunEnvironment, gemini_api, header, payload, stderr};

const JINA_KEY: &str = "jina-live-marker-key";

/// Jina alone at `url`, with a key the anonymous check must leave out.
fn jina_config(url: &str) -> String {
    format!("[providers.jina]\nurl = {url:?}\nkeys = [\"{JINA_KEY}\"]\n")
}

/// A page body long enough to pass the content gate.
fn article() -> String {
    (1..=5)
        .map(|line| format!("Article paragraph {line} with enough words to pass the gate."))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Each reported check as its identifier and status.
fn statuses(report: &Value) -> Vec<(&str, &str)> {
    report["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .map(|check| {
            (
                check["id"].as_str().expect("check id"),
                check["status"].as_str().expect("check status"),
            )
        })
        .collect()
}

#[test]
fn the_anonymous_web_fetch_check_reads_without_the_configured_key_and_exits_0() {
    let fixture = Fixture::start_sequence(vec![jina(&article())]);
    let environment = RunEnvironment::new(&jina_config(&fixture.url));

    let output = environment.run(&["doctor", "--live", "--provider", "jina", "--tier", "free"]);
    let report = payload(&output);
    let request = fixture.finish();

    assert_eq!(
        (
            output.status.code(),
            &report["mode"],
            statuses(&report),
            &report["checks"][0]["attempts"],
            header(&request, "authorization"),
            request.contains(JINA_KEY),
        ),
        (
            Some(0),
            &json!("live"),
            vec![("jina:web_fetch:anonymous", "passed")],
            &json!(1),
            None,
            false,
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_failed_check_exits_4_and_reports_why() {
    let fixture = Fixture::start_sequence(vec![jina("")]);
    let environment = RunEnvironment::new(&jina_config(&fixture.url));

    let output = environment.run(&["doctor", "--live", "--provider", "jina", "--tier", "free"]);
    let report = payload(&output);
    fixture.finish();

    assert_eq!(
        (
            output.status.code(),
            &report["ok"],
            statuses(&report),
            &report["checks"][0]["error_kind"],
        ),
        (
            Some(4),
            &json!(false),
            vec![("jina:web_fetch:anonymous", "failed")],
            &json!("quality"),
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_run_where_no_selected_check_is_configured_sends_nothing_and_exits_3() {
    let canary = Fixture::start_canary();
    let environment = RunEnvironment::new(&format!("[providers.exa]\nurl = {:?}\n", canary.url));

    let output = environment.run(&["doctor", "--live", "--provider", "exa"]);
    let report = payload(&output);
    let checks = report["checks"].as_array().expect("checks");

    assert_eq!(
        (
            output.status.code(),
            checks.is_empty(),
            checks
                .iter()
                .filter(|check| check["status"] != "unconfigured"
                    || check["attempts"] != 0
                    || !check["message"]
                        .as_str()
                        .is_some_and(|message| message.contains("providers.exa.keys")))
                .collect::<Vec<_>>(),
            canary.finish_all(),
        ),
        (Some(3), false, Vec::<&Value>::new(), Vec::<String>::new()),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn an_unattended_or_ci_run_refuses_the_attended_tier_before_reading_the_configuration() {
    // A configuration that does not parse: reading it would end the run with exit 3 instead.
    let environment = RunEnvironment::new("[providers.jina\n");
    let runs = [
        environment.run(&["doctor", "--live", "--unattended", "--tier", "attended"]),
        environment.run_with_env(
            &["doctor", "--live", "--provider", "xiaohongshu_browser"],
            &[("CI", "true")],
        ),
    ];

    assert_eq!(
        runs.iter()
            .map(|output| (
                output.status.code(),
                stderr(output).contains("attended tier")
            ))
            .collect::<Vec<_>>(),
        [(Some(2), true); 2],
        "{:?}",
        runs.iter().map(stderr).collect::<Vec<_>>()
    );
}

#[test]
fn the_listing_prints_the_declared_checks_without_running_any() {
    let canary = Fixture::start_canary();
    let environment = RunEnvironment::new(&format!(
        "[providers.jina]\nurl = {:?}\nkeys = [\"jina-marker-key\"]\n",
        canary.url
    ));

    let output = environment.run(&["doctor", "--live", "--list"]);
    let listing = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &listing["mode"],
            listing["checks"]
                .as_array()
                .is_some_and(|checks| checks.iter().any(|check| check["id"] == "jina:web_fetch")),
            canary.finish_all().len(),
        ),
        (Some(0), &json!("live_list"), true, 0),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_model_check_reads_its_video_through_youtube_api_whatever_the_configured_order() {
    let youtube_web = Fixture::start_canary();
    let invalid_key = || Response::json(400, &sample("youtube_api/error.json"));
    let youtube_api = Fixture::start_repeating(invalid_key());
    let gemini = Fixture::start_repeating(invalid_key());
    let environment = RunEnvironment::new(&format!(
        "{}\n[providers.youtube_web]\nurl = {:?}\n\n[platforms.youtube]\norder = [\"youtube_web\", \"youtube_api\", \"gemini_api\"]\n",
        gemini_api::config(&youtube_api.url, &gemini.url),
        youtube_web.url
    ));

    let output = environment.run(&["doctor", "--live", "--provider", "gemini_api"]);
    let report = payload(&output);
    gemini.finish_all();

    assert_eq!(
        (
            report["checks"]
                .as_array()
                .expect("checks")
                .iter()
                .map(|check| check["id"].as_str().expect("check id"))
                .collect::<Vec<_>>(),
            youtube_api.finish_all().is_empty(),
            youtube_web.finish_all(),
        ),
        (
            vec!["gemini_api:ask", "gemini_api:transcribe"],
            false,
            Vec::<String>::new()
        ),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn the_hidden_supplemental_web_search_entry_prints_the_providers_results() {
    let stored = sample_json("tavily/search.json");
    let fixture = Fixture::start_sequence(vec![Response::json(200, &stored.to_string())]);
    let environment = RunEnvironment::new(&format!(
        "[providers.tavily]\nurl = {:?}\nkeys = [\"tavily-key\"]\n",
        fixture.url
    ));

    let output = environment.run(&[
        "doctor",
        "--supplemental-web-search",
        "tavily",
        "--query",
        "q",
    ]);
    let result = payload(&output);
    fixture.finish();

    assert_eq!(
        (output.status.code(), &result["results"][0]["url"]),
        (Some(0), &stored["results"][0]["url"]),
        "stderr: {}",
        stderr(&output)
    );
}
