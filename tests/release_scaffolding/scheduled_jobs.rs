//! The maintainer's scheduled jobs in `scripts/schedules/`, run against stand-ins.

use std::fs;

use serde_json::{Value, json};

use super::stand_ins::{Fakes, issue_list};

const REPOSITORY: &str = "jfmoe/forager";

/// Writes a live report holding `notifications` and returns its path.
fn report(fakes: &Fakes, notifications: &Value) -> String {
    let path = fakes.home().join("report.json");
    fs::create_dir_all(fakes.home()).expect("create the home directory");
    fs::write(
        &path,
        json!({"mode": "live", "notifications": notifications}).to_string(),
    )
    .expect("write the report");
    path.to_str().expect("a Unicode path").to_owned()
}

fn notification(provider: &str, action: &str) -> Value {
    json!({
        "provider": provider,
        "action": action,
        "label": format!("live-check:{provider}"),
        "title": format!("Live Checks of {provider} fail"),
        "body": format!("body of {provider}"),
    })
}

const LIVE_ISSUES: &str = "scripts/schedules/forager-live-issues";

/// Answers `gh issue list` with an open issue for some labels.
const OPEN_ISSUES: &str = r#"case "$1 $2 $*" in
  "issue list "*live-check:jina*) echo 7 ;;
  "issue list "*live-check:xai*) echo 9 ;;
  "issue list "*) echo ;;
esac"#;

#[test]
fn the_live_issue_job_opens_comments_on_or_closes_each_providers_issue_and_notifies_each_open_and_update()
 {
    let mut fakes = Fakes::new();
    fakes.program("gh", OPEN_ISSUES);
    let report = report(
        &fakes,
        &json!([
            notification("exa", "open"),
            notification("jina", "update"),
            notification("hn_api", "close"),
            notification("xai", "close"),
        ]),
    );

    let output = fakes.run(LIVE_ISSUES, &[&report, "4"], &[]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        fakes.calls(),
        [
            issue_list(REPOSITORY, "live-check:exa"),
            "gh label create live-check:exa --repo jfmoe/forager --force --color FBCA04 \
             --description forager's scheduled Live Checks of one provider"
                .to_owned(),
            "gh issue create --repo jfmoe/forager --label live-check:exa --title Live Checks of \
             exa fail --body body of exa"
                .to_owned(),
            "notify forager: Live Checks of exa fail | open: the issue labelled live-check:exa"
                .to_owned(),
            issue_list(REPOSITORY, "live-check:jina"),
            "gh issue comment 7 --repo jfmoe/forager --body body of jina".to_owned(),
            "notify forager: Live Checks of jina fail | update: the issue labelled \
             live-check:jina"
                .to_owned(),
            issue_list(REPOSITORY, "live-check:hn_api"),
            issue_list(REPOSITORY, "live-check:xai"),
            "gh issue close 9 --repo jfmoe/forager --comment body of xai".to_owned(),
        ]
    );
}

#[test]
fn the_live_issue_job_notifies_a_failed_gh_command_and_a_run_that_checked_nothing() {
    let mut fakes = Fakes::new();
    fakes.program(
        "gh",
        r#"case "$1 $2 $*" in
  "issue list "*live-check:exa*) exit 1 ;;
  "issue list "*) echo ;;
  "issue create "*) exit 1 ;;
esac"#,
    );
    let report = report(
        &fakes,
        &json!([notification("exa", "open"), notification("jina", "open"),]),
    );

    let output = fakes.run(LIVE_ISSUES, &[&report, "3"], &[]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        fakes
            .calls()
            .into_iter()
            .filter(|call| call.starts_with("notify"))
            .collect::<Vec<_>>(),
        [
            format!("notify forager live run | doctor --live exited 3; read {report}"),
            "notify forager: Live Checks of exa fail | could not find the issue labelled \
             live-check:exa to open it"
                .to_owned(),
            "notify forager: Live Checks of jina fail | could not open the issue labelled \
             live-check:jina"
                .to_owned(),
            "notify forager: Live Checks of jina fail | open: the issue labelled live-check:jina"
                .to_owned(),
        ]
    );
}

/// A `forager` whose live run reports no notification and exits as `FAKE_FORAGER_EXIT` says, and
/// whose health summary exits 4.
const FORAGER: &str = r#"case "$*" in
  *--health*) exit 4 ;;
  *) echo '{"mode": "live", "notifications": []}'; exit "${FAKE_FORAGER_EXIT:-0}" ;;
esac"#;

/// The calls of the daily job when its live run exits `exit_code`.
fn daily_job_calls(exit_code: &str) -> Vec<String> {
    let mut fakes = Fakes::new();
    fakes.program("forager", FORAGER);
    fakes.program("gh", "");
    let output = fakes.run(
        "scripts/schedules/forager-live-daily",
        &[],
        &[("FAKE_FORAGER_EXIT", exit_code)],
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    fakes.calls()
}

const DAILY_LIVE_RUN: &str = "forager doctor --live --unattended --tier free --timeout 900";

#[test]
fn the_daily_job_runs_the_free_tier_unattended_and_leaves_a_failed_run_to_its_decision() {
    assert_eq!(daily_job_calls("4"), [DAILY_LIVE_RUN]);
}

#[test]
fn the_daily_job_notifies_a_run_that_checked_nothing() {
    let calls = daily_job_calls("2");

    assert_eq!(
        (
            calls.first().map(String::as_str),
            calls.len(),
            calls[1].starts_with("notify forager live run | doctor --live exited 2;"),
        ),
        (Some(DAILY_LIVE_RUN), 2, true),
        "{calls:?}"
    );
}

/// A `gh` that answers the last successful run of the build workflow and runs `advisories` for
/// the advisory workflow's.
fn workflow_answers(advisories: &str) -> String {
    format!(
        r#"case "$1 $2 $*" in
  "run list "*scheduled-build.yml*) echo 2026-11-23T03:30:00Z ;;
  "run list "*scheduled-advisories.yml*) {advisories} ;;
esac"#
    )
}

fn workflow_record(fakes: &Fakes) -> Option<Value> {
    let text = fs::read_to_string(fakes.state().join("forager/github_workflows.json")).ok()?;
    Some(serde_json::from_str(&text).expect("parse the record"))
}

#[test]
fn the_weekly_job_runs_the_metered_tier_records_each_workflows_last_success_and_notifies_a_health_finding()
 {
    let mut fakes = Fakes::new();
    fakes.program("forager", FORAGER);
    fakes.program("gh", &workflow_answers("echo"));

    let output = fakes.run("scripts/schedules/forager-live-weekly", &[], &[]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        workflow_record(&fakes),
        Some(json!({
            "schema_version": 1,
            "workflows": [
                {"workflow": "scheduled-build.yml", "interval_days": 7, "last_success": "2026-11-23T03:30:00Z"},
                {"workflow": "scheduled-advisories.yml", "interval_days": 7, "last_success": null},
            ],
        }))
    );
    assert_eq!(
        fakes
            .calls()
            .into_iter()
            .filter(|call| !call.starts_with("gh "))
            .collect::<Vec<_>>(),
        [
            "forager doctor --live --unattended --tier metered --timeout 900".to_owned(),
            "forager doctor --health --format markdown".to_owned(),
            format!(
                "notify forager health | doctor --health flags something; read \
                 {}/Library/Logs/forager/health.md",
                fakes.home().display()
            ),
        ]
    );
}

#[test]
fn the_weekly_job_keeps_the_last_workflow_record_when_a_query_fails() {
    let mut fakes = Fakes::new();
    fakes.program("forager", FORAGER);
    fakes.program("gh", &workflow_answers("exit 1"));
    let earlier = json!({"schema_version": 1, "workflows": []});
    fs::create_dir_all(fakes.state().join("forager")).expect("create the state directory");
    fs::write(
        fakes.state().join("forager/github_workflows.json"),
        earlier.to_string(),
    )
    .expect("write the earlier record");

    let output = fakes.run("scripts/schedules/forager-live-weekly", &[], &[]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(workflow_record(&fakes), Some(earlier));
    assert!(
        fakes.calls().contains(
            &"notify forager weekly job | could not read the last success of the GitHub \
              scheduled workflows"
                .to_owned()
        ),
        "{:?}",
        fakes.calls()
    );
}
