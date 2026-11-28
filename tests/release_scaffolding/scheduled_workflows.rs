//! The issue a scheduled GitHub workflow keeps, through `.github/scripts/report-scheduled-run.sh`
//! run against a stand-in `gh`.

use super::stand_ins::{Fakes, issue_list};

/// The calls of a scheduled workflow's report when its jobs ended as `results` and the open
/// issue is `open_issue`, if any.
fn workflow_report_calls(results: &str, open_issue: &str) -> Vec<String> {
    let mut fakes = Fakes::new();
    fakes.program(
        "gh",
        &format!(r#"case "$1 $2" in "issue list") echo {open_issue} ;; esac"#),
    );
    let output = fakes.run(
        ".github/scripts/report-scheduled-run.sh",
        &["workflow:scheduled-build", "Scheduled release build fails"],
        &[
            ("GITHUB_SERVER_URL", "https://github.example"),
            ("GITHUB_REPOSITORY", "owner/repo"),
            ("GITHUB_RUN_ID", "42"),
            ("RESULTS", results),
        ],
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    fakes.calls()
}

const RUN_URL: &str = "https://github.example/owner/repo/actions/runs/42";

fn workflow_issue_list() -> String {
    issue_list("owner/repo", "workflow:scheduled-build")
}

#[test]
fn a_failed_scheduled_workflow_without_an_open_issue_opens_one() {
    assert_eq!(
        workflow_report_calls("success failure", ""),
        [
            workflow_issue_list(),
            "gh label create workflow:scheduled-build --repo owner/repo --force --color D93F0B \
             --description A scheduled workflow's long-lived failure issue"
                .to_owned(),
            format!(
                "gh issue create --repo owner/repo --label workflow:scheduled-build --title \
                 Scheduled release build fails --body The scheduled workflow failed in {RUN_URL}. \
                 A later successful run closes this issue."
            ),
        ]
    );
}

#[test]
fn a_scheduled_workflow_that_did_not_succeed_comments_on_its_open_issue() {
    assert_eq!(
        workflow_report_calls("success cancelled", "5"),
        [
            workflow_issue_list(),
            format!("gh issue comment 5 --repo owner/repo --body Failed again in {RUN_URL}."),
        ]
    );
}

#[test]
fn a_successful_scheduled_workflow_closes_its_open_issue() {
    assert_eq!(
        workflow_report_calls("success success", "5"),
        [
            workflow_issue_list(),
            format!("gh issue close 5 --repo owner/repo --comment Passed again in {RUN_URL}."),
        ]
    );
}

#[test]
fn a_successful_scheduled_workflow_without_an_open_issue_changes_nothing() {
    assert_eq!(
        workflow_report_calls("success success", ""),
        [workflow_issue_list()]
    );
}
