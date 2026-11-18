use std::fs;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::json;

use super::{
    FailureKind, LiveCaseDefinition, LiveRun, XIAOHONGSHU_NO_ACCESS_URL, XIAOHONGSHU_NO_REPLIES,
    XIAOHONGSHU_ONE_COMMENT_PAGE, XIAOHONGSHU_STOPPED, XiaohongshuRun, command_specs,
    comments_coverage, execute_with_deadline, is_refusal, live_case, live_case_process,
    most_commented_access_url, official_status_host, registry_status, status_host_matches_endpoint,
    status_page_reports_outage, xiaohongshu_blocker,
};
use crate::catalog;
use crate::types::Deadline;

#[test]
fn live_case_processes_disable_the_search_result_journal() {
    let specs =
        command_specs("P1", 60, None, &LiveRun::default()).expect("P1 is a registered live case");

    let journal_settings = specs
        .iter()
        .map(|spec| {
            live_case_process(std::path::Path::new("forager"), spec)
                .get_envs()
                .find(|(name, _)| *name == "FORAGER_JOURNAL__ENABLED")
                .and_then(|(_, value)| value)
                .map(|value| value.to_string_lossy().into_owned())
        })
        .collect::<Vec<_>>();

    assert_eq!(
        journal_settings,
        vec![Some("false".to_owned()); specs.len()]
    );
}

#[test]
fn deadline_terminates_the_child_process() {
    let root = tempfile::tempdir().expect("create marker root");
    let marker = root.path().join("child-completed");
    let mut command = Command::new(std::env::current_exe().expect("resolve test executable"));
    command
        .args([
            "--exact",
            "smoke::tests::deadline_child_writes_marker",
            "--nocapture",
        ])
        .env("FORAGER_SMOKE_DEADLINE_MARKER", &marker);

    let started = Instant::now();
    let result = execute_with_deadline(command, Deadline::new(Duration::from_millis(50)));
    let elapsed = started.elapsed();
    thread::sleep(Duration::from_millis(1_100));

    assert_eq!(result.unwrap_err(), "live smoke hard deadline elapsed");
    assert!(elapsed < Duration::from_millis(500));
    assert!(!marker.exists(), "timed-out child kept running");
}

#[test]
fn deadline_child_writes_marker() {
    let Some(marker) = std::env::var_os("FORAGER_SMOKE_DEADLINE_MARKER") else {
        return;
    };
    thread::sleep(Duration::from_secs(1));
    fs::write(marker, "completed").expect("write completion marker");
}

#[test]
fn map_live_case_uses_a_legal_command_timeout() {
    let timeouts = [1, 10, 150, 600].map(|budget| {
        let commands =
            command_specs("C17", budget, None, &LiveRun::default()).expect("C17 command");
        commands[0].arguments[5].clone()
    });

    assert_eq!(timeouts, ["10", "10", "150", "150"]);
}

#[test]
fn official_status_evidence_is_bound_to_the_case_provider() {
    assert_eq!(
        official_status_host("https://status.x.ai/"),
        Some("status.x.ai")
    );
    assert!(status_host_matches_endpoint(
        "status.x.ai",
        "https://api.x.ai/v1"
    ));
    assert!(!status_host_matches_endpoint(
        "status.openai.com",
        "https://api.x.ai/v1"
    ));
    assert_eq!(official_status_host("https://example.com/status"), None);
    assert_eq!(official_status_host("http://status.x.ai/"), None);
}

#[test]
fn official_status_evidence_requires_a_statuspage_outage_indicator() {
    assert_eq!(
        [
            r#"{"status":{"indicator":"major"},"incidents":[]}"#,
            r#"{"status":{"indicator":"none"},"incidents":[{"status":"investigating"}]}"#,
            "Major outage affecting the API",
            "not valid JSON",
        ]
        .map(status_page_reports_outage),
        [true, false, false, false]
    );
}

#[test]
fn registry_status_accepts_registrations_without_credentials() {
    let mut registry = catalog::registrations().to_vec();
    registry[0].credentials = catalog::Credentials::Unused;

    assert!(registry_status(&registry).ok);
}

fn xiaohongshu_case(id: &str) -> LiveCaseDefinition {
    live_case(id).expect("registered Xiaohongshu case")
}

#[test]
fn the_xiaohongshu_comments_case_reads_the_most_commented_search_result() {
    let search = json!({"items": [
        {"access_url": "https://www.xiaohongshu.com/explore/a?xsec_token=A", "comments": "3"},
        {"access_url": "https://www.xiaohongshu.com/explore/b?xsec_token=B", "comments": "1.2万"},
        {"access_url": "https://www.xiaohongshu.com/explore/c?xsec_token=C", "comments": "999"},
        {"access_url": "https://www.xiaohongshu.com/explore/d?xsec_token=D", "comments": "1.2万"},
        {"access_url": "https://www.xiaohongshu.com/explore/e?xsec_token=E"}
    ]});

    assert_eq!(
        most_commented_access_url(&search).as_deref(),
        Some("https://www.xiaohongshu.com/explore/b?xsec_token=B")
    );
}

#[test]
fn the_later_xiaohongshu_cases_read_the_notes_the_search_case_left() {
    let run = LiveRun {
        xiaohongshu: XiaohongshuRun {
            access_url: Some("https://www.xiaohongshu.com/explore/a?xsec_token=A".into()),
            most_commented_access_url: Some(
                "https://www.xiaohongshu.com/explore/b?xsec_token=B".into(),
            ),
            stopped: false,
        },
        ..LiveRun::default()
    };
    let scratch = std::path::Path::new("/scratch");

    let inputs = ["C28", "C29"].map(|id| {
        command_specs(id, 60, Some(scratch), &run).expect("Xiaohongshu case")[0].arguments[..4]
            .to_vec()
    });

    assert_eq!(
        inputs,
        [
            [
                "platform",
                "xiaohongshu",
                "fetch",
                "https://www.xiaohongshu.com/explore/a?xsec_token=A"
            ],
            [
                "platform",
                "xiaohongshu",
                "comments",
                "https://www.xiaohongshu.com/explore/b?xsec_token=B"
            ],
        ]
    );
}

#[test]
fn a_refused_xiaohongshu_case_stops_every_later_one() {
    let stopped = XiaohongshuRun {
        access_url: Some("https://www.xiaohongshu.com/explore/a?xsec_token=A".into()),
        stopped: true,
        ..XiaohongshuRun::default()
    };
    let refusals = [
        (Some(4), json!({"error_kind": "auth"})),
        (Some(4), json!({"error_kind": "parameter"})),
        (Some(4), json!({"error_kind": "runtime"})),
        (Some(3), json!({"error_kind": "config"})),
    ]
    .map(|(code, payload)| is_refusal(code, payload.to_string().as_bytes()));

    assert_eq!(
        (
            refusals,
            ["C27", "C28", "C29"].map(|id| xiaohongshu_blocker(xiaohongshu_case(id), &stopped)),
            ["C27", "C28", "C29"]
                .map(|id| xiaohongshu_blocker(xiaohongshu_case(id), &XiaohongshuRun::default())),
        ),
        (
            [true, true, false, false],
            [Some(XIAOHONGSHU_STOPPED); 3],
            [
                None,
                Some(XIAOHONGSHU_NO_ACCESS_URL),
                Some(XIAOHONGSHU_NO_ACCESS_URL)
            ],
        )
    );
}

#[test]
fn the_comments_case_passes_only_with_a_second_page_and_an_expansion() {
    let comments = |count: usize, replies: usize| {
        json!({"comments": (0..count)
            .map(|index| json!({"replies": vec![json!({}); if index == 0 { replies } else { 0 }]}))
            .collect::<Vec<_>>()})
    };

    let results = [comments(11, 2), comments(10, 2), comments(11, 1)].map(|payload| {
        comments_coverage(&payload)
            .err()
            .map(|failure| (failure.message, failure.kind == FailureKind::Unverified))
    });

    assert_eq!(
        results,
        [
            None,
            Some((XIAOHONGSHU_ONE_COMMENT_PAGE, true)),
            Some((XIAOHONGSHU_NO_REPLIES, true)),
        ]
    );
}
