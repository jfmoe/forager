use std::fs;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::json;

use super::{
    FailureKind, LiveCaseDefinition, LiveRun, REDDIT_NO_SAFE_POST, REDDIT_SEARCH_NOT_PASSED,
    REDDIT_STOPPED, RedditRun, RedditSearch, XIAOHONGSHU_NO_ACCESS_URL, XIAOHONGSHU_NO_REPLIES,
    XIAOHONGSHU_ONE_COMMENT_PAGE, XIAOHONGSHU_STOPPED, XiaohongshuRun, blocker, command_specs,
    comments_coverage, execute_with_deadline, live_case, live_case_process, max_attempts,
    most_commented_access_url, needs_scratch, official_status_host, refusal, registry_status,
    status_host_matches_endpoint, status_page_reports_outage, stop_after_refusal,
    xiaohongshu_blocker,
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
fn a_rate_limit_notice_or_http_471_refuses_the_case() {
    // The failure payloads `platform xiaohongshu search` prints for these two signals.
    let failures = [
        json!({
            "error_kind": "auth",
            "message": "Xiaohongshu says the account requests too often (请求太频繁); stop, run `forager browser login xiaohongshu` and complete any check the site shows in its window, then retry later",
        }),
        json!({
            "error_kind": "auth",
            "message": "Xiaohongshu answered HTTP 471 and wants a verification; run `forager browser login xiaohongshu` and complete any check the site shows, then retry",
        }),
    ];

    let refusals = failures.map(|payload| refusal(Some(4), payload.to_string().as_bytes()));

    assert_eq!(refusals, [Some(FailureKind::Auth); 2]);
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
    .map(|(code, payload)| refusal(code, payload.to_string().as_bytes()).is_some());

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

fn case(id: &str) -> LiveCaseDefinition {
    live_case(id).expect("registered case")
}

#[test]
fn the_reddit_fetch_case_reads_the_first_search_result_marked_as_not_nsfw() {
    let search = json!({"items": [
        {"ref": "reddit:1aaaaaa", "nsfw": true},
        {"ref": "reddit:1bbbbbb", "nsfw": false},
        {"ref": "reddit:1cccccc", "nsfw": false}
    ]});
    let run = LiveRun {
        reddit: RedditRun {
            search: RedditSearch::passed(&search),
            stopped: false,
        },
        ..LiveRun::default()
    };

    let command = command_specs("C51", 60, Some(std::path::Path::new("/scratch")), &run)
        .expect("Reddit fetch case")[0]
        .arguments[..6]
        .to_vec();

    assert_eq!(
        command,
        [
            "platform",
            "reddit",
            "fetch",
            "reddit:1bbbbbb",
            "--depth",
            "thread"
        ]
    );
}

#[test]
fn a_search_result_without_a_false_nsfw_flag_is_never_the_post_to_read() {
    let search = json!({"items": [
        {"ref": "reddit:1aaaaaa"},
        {"ref": "reddit:1bbbbbb", "nsfw": null},
        {"ref": "reddit:1cccccc", "nsfw": "false"},
        {"ref": "reddit:1dddddd", "nsfw": false}
    ]});

    assert_eq!(
        RedditSearch::passed(&search),
        RedditSearch::Post("reddit:1dddddd".into())
    );
}

#[test]
fn the_reddit_fetch_case_says_whether_the_search_case_failed_or_found_no_post_to_read() {
    let run = |search| LiveRun {
        reddit: RedditRun {
            search,
            stopped: false,
        },
        ..LiveRun::default()
    };
    let unflagged = json!({"items": [
        {"ref": "reddit:1aaaaaa", "nsfw": true},
        {"ref": "reddit:1bbbbbb"}
    ]});

    assert_eq!(
        [
            run(RedditSearch::NotPassed),
            run(RedditSearch::passed(&unflagged)),
        ]
        .map(|run| ["C50", "C51"].map(|id| blocker(case(id), &run))),
        [
            [None, Some(REDDIT_SEARCH_NOT_PASSED)],
            [None, Some(REDDIT_NO_SAFE_POST)],
        ]
    );
}

#[test]
fn auth_on_a_reddit_case_stops_the_later_reddit_cases_only() {
    let stopped_after = |id: &str, kind| {
        let mut run = LiveRun::default();
        stop_after_refusal(case(id), kind, &mut run);
        run.reddit.stopped
    };
    let stopped = LiveRun {
        reddit: RedditRun {
            search: RedditSearch::Post("reddit:1bbbbbb".into()),
            stopped: true,
        },
        ..LiveRun::default()
    };

    assert_eq!(
        (
            [
                stopped_after("C50", FailureKind::Auth),
                stopped_after("C51", FailureKind::Parameter),
                stopped_after("C27", FailureKind::Auth),
            ],
            ["C27", "C36", "C50", "C51"].map(|id| blocker(case(id), &stopped)),
        ),
        (
            [true, false, false],
            [None, None, Some(REDDIT_STOPPED), Some(REDDIT_STOPPED)],
        )
    );
}

#[test]
fn browser_route_cases_run_once() {
    assert_eq!(
        ["C27", "C50", "C51", "C36", "C37"].map(|id| max_attempts(case(id))),
        [1, 1, 1, 3, 3]
    );
}

#[test]
fn every_case_whose_commands_need_a_scratch_directory_gets_one() {
    let run = LiveRun {
        xiaohongshu: XiaohongshuRun {
            access_url: Some("https://www.xiaohongshu.com/explore/a?xsec_token=A".into()),
            most_commented_access_url: Some(
                "https://www.xiaohongshu.com/explore/b?xsec_token=B".into(),
            ),
            stopped: false,
        },
        reddit: RedditRun {
            search: RedditSearch::Post("reddit:1bbbbbb".into()),
            stopped: false,
        },
        polymarket_event: Some("polymarket:some-event".into()),
    };
    let scratch = std::path::Path::new("/scratch");

    let unprovided = super::LIVE_CASES
        .iter()
        .filter(|definition| {
            command_specs(definition.id, 60, None, &run).is_none()
                && command_specs(definition.id, 60, Some(scratch), &run).is_some()
                && !needs_scratch(**definition)
        })
        .map(|definition| definition.id)
        .collect::<Vec<_>>();

    assert_eq!(unprovided, Vec::<&str>::new());
}
