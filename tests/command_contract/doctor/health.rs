//! The health summary through the binary: it reads the telemetry and the schedule records the
//! state directory holds, and sends no request.

use std::fs;
use std::path::Path;

use chrono::{DateTime, TimeDelta, Utc};
use serde_json::{Value, json};

use crate::support::web_samples::jina;
use crate::support::{Fixture, RunEnvironment};

/// Records a scheduled live run of `tiers` at `at` in the state directory `state`.
fn record_scheduled_run(state: &Path, at: DateTime<Utc>, tiers: &[&str]) {
    let directory = state.join("live_runs");
    fs::create_dir_all(&directory).expect("create the run records");
    fs::write(
        directory.join(format!(
            "live_run_{}_1.json",
            at.timestamp_nanos_opt()
                .expect("a time within the nanosecond range")
        )),
        json!({"recorded_at": at, "scheduled": true, "tiers": tiers, "checks": []}).to_string(),
    )
    .expect("write the run record");
}

/// Each schedule of a health summary and its status.
fn schedules(payload: &Value) -> Vec<String> {
    payload["schedules"]
        .as_array()
        .expect("schedules")
        .iter()
        .map(|schedule| {
            let name = schedule["tier"].as_str().or(schedule["workflow"].as_str());
            format!(
                "{}: {}",
                name.unwrap_or_default(),
                schedule["status"].as_str().unwrap_or_default()
            )
        })
        .collect()
}

#[test]
fn the_health_summary_reads_telemetry_and_the_schedule_records_without_a_request() {
    let page = (1..=12)
        .map(|line| format!("Paragraph {line} of the page, long enough to keep as content."))
        .collect::<Vec<_>>()
        .join("\n");
    let pages = Fixture::start_sequence(vec![jina(&page)]);
    let environment = RunEnvironment::new(&format!(
        "[providers.jina]\nurl = {:?}\n\n[capabilities.web_fetch]\norder = [\"jina\"]\n",
        pages.url
    ));
    let fetch = environment.run(&["fetch", "https://example.test/page"]);
    let state = environment.state_dir.join("forager");
    let now = Utc::now();
    record_scheduled_run(&state, now - TimeDelta::days(30), &["free"]);
    fs::write(
        state.join("github_workflows.json"),
        json!({
            "schema_version": 1,
            "workflows": [{"workflow": "build", "interval_days": 7, "last_success": now - TimeDelta::days(1)}],
        })
        .to_string(),
    )
    .expect("write the workflow record");

    let health = environment.run(&["doctor", "--health"]);
    let requests = pages.finish_all();
    let payload: Value = serde_json::from_slice(&health.stdout).expect("parse the health JSON");

    assert_eq!(
        (
            fetch.status.code(),
            health.status.code(),
            &payload["mode"],
            &payload["ok"],
            &payload["providers"][0]["calls"],
            schedules(&payload),
            requests.len(),
        ),
        (
            Some(0),
            Some(4),
            &json!("health"),
            &json!(false),
            &json!(1),
            vec![
                "free: overdue".to_owned(),
                "metered: not_recorded".to_owned(),
                "build: current".to_owned(),
            ],
            1,
        ),
        "{payload}\nstderr: {}",
        String::from_utf8_lossy(&health.stderr)
    );
}

/// The shallow and the offline report share the line, so the offline report, which sends
/// nothing, proves it.
#[test]
fn the_doctor_report_adds_one_health_line_when_a_scheduled_tier_is_overdue() {
    let environment = RunEnvironment::new("");
    record_scheduled_run(
        &environment.state_dir.join("forager"),
        Utc::now() - TimeDelta::days(30),
        &["free"],
    );

    let doctor = environment.run(&["doctor", "--offline"]);
    let payload: Value = serde_json::from_slice(&doctor.stdout).expect("parse doctor JSON");

    assert_eq!(
        payload["health"],
        json!("tier free overdue; run `forager doctor --health` for the report"),
        "{payload}"
    );
}
