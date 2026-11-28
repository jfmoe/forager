use std::collections::VecDeque;
use std::time::Duration;

use serde_json::{Value, json};

use super::{
    CONFIRMATION_DELAY, Confirmation, Declared, Execution, LiveError, LiveSelection, Prepared,
    Status, declared, exit_code, prepare, run_checks, select, summarize,
};
use crate::catalog::{
    Field, LiveArgument, LiveCheck, LiveCommand, LiveContract, LiveInput, LiveTier, ProviderId,
    RoutePin,
};
use crate::redact::Secret;
use crate::types::{Deadline, Platform};

const RESULTS: LiveContract =
    LiveContract::fields(&[Field::list("results", 1), Field::text("results[].url")]);

const PASSING: &[LiveArgument] = &[LiveArgument::Text("pass")];

fn check(id: &'static str, tier: LiveTier) -> LiveCheck {
    LiveCheck::new(
        id,
        tier,
        LiveCommand {
            arguments: PASSING,
            routes: RoutePin::Configured,
        },
        RESULTS,
        "a test sample",
    )
}

fn on(provider: ProviderId, check: LiveCheck) -> Declared {
    Declared {
        provider: Some(provider),
        check,
    }
}

fn passed() -> Execution {
    Execution::Exited {
        code: Some(0),
        stdout: json!({"results": [{"url": "https://example.com/"}]})
            .to_string()
            .into_bytes(),
    }
}

fn failed(error_kind: &str, message: &str) -> Execution {
    Execution::Exited {
        code: Some(4),
        stdout: json!({"error_kind": error_kind, "message": message})
            .to_string()
            .into_bytes(),
    }
}

/// What a run of `declared` reports, every check selected and configured, when each command
/// ends as `executions` says in turn; also returns the commands started.
fn run(
    declared: &[Declared],
    executions: Vec<Execution>,
    deadline: Deadline,
) -> (Vec<Value>, Vec<Prepared>, u8) {
    let (results, started, exit, _) = run_with(declared, executions, deadline, false);
    (results, started, exit)
}

/// As [`run`], in an unattended run when `confirming`; also returns each confirmation delay
/// the run waited.
fn run_with(
    declared: &[Declared],
    executions: Vec<Execution>,
    deadline: Deadline,
    confirming: bool,
) -> (Vec<Value>, Vec<Prepared>, u8, Vec<Duration>) {
    let mut executions = VecDeque::from(executions);
    let mut started = Vec::new();
    let mut execute = |command: &Prepared, _: Duration| {
        started.push(Prepared {
            arguments: command.arguments.clone(),
            environment: command.environment.clone(),
        });
        executions
            .pop_front()
            .expect("an execution for every command")
    };
    let mut waited = Vec::new();
    let mut wait = |delay| waited.push(delay);
    let results = run_checks(
        declared,
        &vec![true; declared.len()],
        &vec![None; declared.len()],
        deadline,
        &[Secret::from("sk-live-secret")],
        &mut execute,
        confirming.then_some(Confirmation {
            delay: CONFIRMATION_DELAY,
            wait: &mut wait,
        }),
    );
    let exit = exit_code(&results);
    let results = results
        .iter()
        .map(|result| serde_json::to_value(result).expect("serialize result"))
        .collect();
    (results, started, exit, waited)
}

fn long() -> Deadline {
    Deadline::new(Duration::from_mins(10))
}

fn statuses(results: &[Value]) -> Vec<(&str, &str, Option<&str>)> {
    results
        .iter()
        .map(|result| {
            (
                result["id"].as_str().unwrap_or_default(),
                result["status"].as_str().unwrap_or_default(),
                result["cause"].as_str(),
            )
        })
        .collect()
}

fn selected_ids(selection: &LiveSelection) -> Vec<&'static str> {
    let declared = declared();
    let selected = select(&declared, selection).expect("the selection is accepted");
    declared
        .iter()
        .zip(selected)
        .filter(|(_, selected)| *selected)
        .map(|(entry, _)| entry.check.id)
        .collect()
}

fn tier_of(id: &str) -> LiveTier {
    declared()
        .into_iter()
        .find(|entry| entry.check.id == id)
        .expect("a declared check")
        .check
        .tier
}

#[test]
fn a_run_without_a_tier_selection_runs_the_free_and_metered_checks_and_no_attended_one() {
    let selected = selected_ids(&LiveSelection::default());

    assert_eq!(
        (
            ["hn_api:search", "serpapi:search", "reddit_browser:search"]
                .map(|id| selected.contains(&id)),
            selected.iter().any(|id| tier_of(id) == LiveTier::Attended),
        ),
        ([true, true, false], false)
    );
}

#[test]
fn a_tier_selection_runs_only_the_checks_of_the_selected_tiers() {
    for tiers in [
        vec![LiveTier::Free],
        vec![LiveTier::Metered, LiveTier::Attended],
    ] {
        let selected = selected_ids(&LiveSelection {
            tiers: tiers.clone(),
            ..LiveSelection::default()
        });

        assert!(
            !selected.is_empty() && selected.iter().all(|id| tiers.contains(&tier_of(id))),
            "{tiers:?}: {selected:?}"
        );
    }
}

#[test]
fn naming_an_attended_provider_selects_its_attended_checks() {
    let selected = selected_ids(&LiveSelection {
        provider: Some(ProviderId::RedditBrowser),
        ..LiveSelection::default()
    });

    assert_eq!(selected, ["reddit_browser:search", "reddit_browser:fetch"]);
}

#[test]
fn selecting_a_platform_alone_does_not_select_its_attended_checks() {
    let by_platform = |tiers: Vec<LiveTier>| {
        selected_ids(&LiveSelection {
            tiers,
            platform: Some(Platform::Reddit),
            ..LiveSelection::default()
        })
    };

    assert_eq!(
        (
            by_platform(Vec::new()),
            by_platform(vec![LiveTier::Attended])
        ),
        (
            Vec::new(),
            vec!["reddit_browser:search", "reddit_browser:fetch"]
        )
    );
}

#[test]
fn a_platform_selection_runs_a_non_attended_route_of_the_platform() {
    let selected = selected_ids(&LiveSelection {
        platform: Some(Platform::Youtube),
        ..LiveSelection::default()
    });

    assert!(
        selected.contains(&"youtube_api:fetch") && !selected.contains(&"youtube_web:fetch"),
        "{selected:?}"
    );
}

#[test]
fn an_unattended_run_refuses_the_attended_tier_and_an_attended_provider() {
    for selection in [
        LiveSelection {
            tiers: vec![LiveTier::Free, LiveTier::Attended],
            unattended: true,
            ..LiveSelection::default()
        },
        LiveSelection {
            provider: Some(ProviderId::XiaohongshuBrowser),
            unattended: true,
            ..LiveSelection::default()
        },
    ] {
        assert!(
            matches!(
                select(&declared(), &selection),
                Err(LiveError::Refused(message)) if message.contains("--unattended")
            ),
            "{selection:?}"
        );
    }
}

#[test]
fn an_unattended_run_excludes_the_expensive_and_manual_only_checks() {
    let youtube = |unattended| {
        selected_ids(&LiveSelection {
            platform: Some(Platform::Youtube),
            unattended,
            ..LiveSelection::default()
        })
    };
    let flagged = declared()
        .into_iter()
        .filter(|entry| entry.check.expensive || entry.check.manual_only)
        .map(|entry| entry.check.id)
        .collect::<Vec<_>>();

    assert_eq!(
        (
            flagged.clone(),
            youtube(false)
                .into_iter()
                .filter(|id| flagged.contains(id))
                .collect::<Vec<_>>(),
            youtube(true)
                .into_iter()
                .filter(|id| flagged.contains(id))
                .count(),
        ),
        (
            vec![
                "youtube_api:search",
                "gemini_api:ask",
                "gemini_api:transcribe"
            ],
            vec![
                "youtube_api:search",
                "gemini_api:ask",
                "gemini_api:transcribe"
            ],
            0
        )
    );
}

#[test]
fn selecting_a_check_also_selects_the_check_it_depends_on() {
    let upstream = check("xai:main_search", LiveTier::Metered);
    let dependent = LiveCheck {
        command: LiveCommand {
            arguments: PASSING,
            routes: RoutePin::Platform(Platform::X, &[ProviderId::Xai]),
        },
        ..check("xai:search", LiveTier::Metered).depends_on("xai:main_search")
    };
    let declared = [
        on(ProviderId::Xai, upstream),
        on(ProviderId::Xai, dependent),
    ];

    let selected = select(
        &declared,
        &LiveSelection {
            platform: Some(Platform::X),
            ..LiveSelection::default()
        },
    )
    .expect("the selection is accepted");

    assert_eq!(selected, [true, true]);
}

#[test]
fn a_network_error_or_a_timeout_is_retried_once() {
    let declared = [on(
        ProviderId::Exa,
        check("exa:docs_search", LiveTier::Metered),
    )];
    for execution in [failed("network", "connection reset"), Execution::TimedOut] {
        let (results, started, exit) = run(&declared, vec![execution, passed()], long());

        assert_eq!(
            (
                results[0]["status"].as_str(),
                results[0]["attempts"].as_u64(),
                started.len(),
                exit
            ),
            (Some("passed"), Some(2), 2, 0)
        );
    }
    let (results, _, exit) = run(
        &declared,
        vec![failed("network", "reset"), failed("network", "reset")],
        long(),
    );
    assert_eq!(
        (
            results[0]["status"].as_str(),
            results[0]["attempts"].as_u64(),
            exit
        ),
        (Some("failed"), Some(2), 4)
    );
}

#[test]
fn a_contract_authentication_or_parameter_failure_is_not_retried() {
    let declared = [on(
        ProviderId::Exa,
        check("exa:docs_search", LiveTier::Metered),
    )];
    let empty = || Execution::Exited {
        code: Some(0),
        stdout: json!({"results": []}).to_string().into_bytes(),
    };
    for (execution, error_kind) in [
        (empty(), "contract"),
        (failed("auth", "key rejected"), "auth"),
        (failed("parameter", "bad query"), "parameter"),
    ] {
        let (results, started, _) = run(&declared, vec![execution], long());

        assert_eq!(
            (results[0]["error_kind"].as_str(), started.len()),
            (Some(error_kind), 1)
        );
    }
}

#[test]
fn a_browser_check_is_never_retried() {
    let declared = [on(
        ProviderId::XiaohongshuBrowser,
        check("xiaohongshu_browser:search", LiveTier::Attended),
    )];
    for execution in [failed("network", "reset"), Execution::TimedOut] {
        let (results, started, _) = run(&declared, vec![execution], long());

        assert_eq!(
            (results[0]["status"].as_str(), started.len()),
            (Some("failed"), 1)
        );
    }
}

/// Three independent checks on one browser site.
fn site(provider: ProviderId) -> [Declared; 3] {
    let id = |operation| match (provider, operation) {
        (ProviderId::XiaohongshuBrowser, 0) => "xiaohongshu_browser:a",
        (ProviderId::XiaohongshuBrowser, 1) => "xiaohongshu_browser:b",
        (ProviderId::XiaohongshuBrowser, _) => "xiaohongshu_browser:c",
        (_, 0) => "reddit_browser:a",
        (_, 1) => "reddit_browser:b",
        _ => "reddit_browser:c",
    };
    [0, 1, 2].map(|operation| on(provider, check(id(operation), LiveTier::Attended)))
}

#[test]
fn each_xiaohongshu_stop_signal_ends_the_sites_remaining_checks() {
    let signals = [
        failed(
            "auth",
            "Xiaohongshu answered HTTP 461 and wants a verification",
        ),
        failed("parameter", "Xiaohongshu answered code 300031"),
        failed(
            "runtime",
            "the page `验证码` at https://www.xiaohongshu.com/website-login/captcha is not a search page",
        ),
        failed(
            "runtime",
            "Xiaohongshu answered `/api/sns/web/v1/search/notes` with code 999999 (未知错误)",
        ),
    ];
    for signal in signals {
        let (results, started, exit) =
            run(&site(ProviderId::XiaohongshuBrowser), vec![signal], long());

        assert_eq!(
            (statuses(&results), started.len(), exit),
            (
                vec![
                    ("xiaohongshu_browser:a", "failed", None),
                    ("xiaohongshu_browser:b", "not_started", Some("site_stop")),
                    ("xiaohongshu_browser:c", "not_started", Some("site_stop")),
                ],
                1,
                4
            )
        );
    }
}

#[test]
fn two_timeouts_in_a_row_end_the_sites_remaining_checks() {
    for provider in [ProviderId::XiaohongshuBrowser, ProviderId::RedditBrowser] {
        let (results, started, _) = run(
            &site(provider),
            vec![Execution::TimedOut, Execution::TimedOut],
            long(),
        );

        assert_eq!(
            (
                results
                    .iter()
                    .map(|result| (result["status"].as_str(), result["cause"].as_str()))
                    .collect::<Vec<_>>(),
                started.len()
            ),
            (
                vec![
                    (Some("failed"), None),
                    (Some("failed"), None),
                    (Some("not_started"), Some("site_stop"))
                ],
                2
            ),
            "{provider:?}"
        );
    }
}

#[test]
fn on_reddit_only_an_authentication_failure_stops_the_site_at_once() {
    let (stopped, ..) = run(
        &site(ProviderId::RedditBrowser),
        vec![failed("auth", "Prove your humanity")],
        long(),
    );
    let (continued, started, _) = run(
        &site(ProviderId::RedditBrowser),
        vec![failed("parameter", "no such post"), passed(), passed()],
        long(),
    );

    assert_eq!(
        (
            statuses(&stopped)[1],
            statuses(&continued)
                .iter()
                .map(|(_, status, _)| *status)
                .collect::<Vec<_>>(),
            started.len()
        ),
        (
            ("reddit_browser:b", "not_started", Some("site_stop")),
            vec!["failed", "passed", "passed"],
            3
        )
    );
}

#[test]
fn a_check_whose_dependency_left_no_input_does_not_start_and_names_it() {
    let dependent = |id| LiveCheck {
        command: LiveCommand {
            arguments: &[
                LiveArgument::Text("fetch"),
                LiveArgument::Input(LiveInput::FirstItem("ref")),
            ],
            routes: RoutePin::Configured,
        },
        ..check(id, LiveTier::Free).depends_on("polymarket_api:search")
    };
    let declared = [
        on(
            ProviderId::PolymarketApi,
            check("polymarket_api:search", LiveTier::Free),
        ),
        on(
            ProviderId::PolymarketApi,
            dependent("polymarket_api:fetch:live"),
        ),
    ];

    let (results, started, exit) = run(&declared, vec![failed("auth", "rejected")], long());

    assert_eq!(
        (
            statuses(&results)[1],
            results[1]["message"].as_str(),
            started.len(),
            exit
        ),
        (
            ("polymarket_api:fetch:live", "not_started", Some("no_input")),
            Some("not started: polymarket_api:search did not pass"),
            1,
            4
        )
    );
}

#[test]
fn a_dependent_check_reads_the_value_its_dependency_returned() {
    let dependent = LiveCheck {
        command: LiveCommand {
            arguments: &[
                LiveArgument::Text("fetch"),
                LiveArgument::Input(LiveInput::FirstSafeRef),
            ],
            routes: RoutePin::Configured,
        },
        ..check("reddit_browser:fetch", LiveTier::Attended).depends_on("reddit_browser:search")
    };
    let declared = [
        on(
            ProviderId::RedditBrowser,
            check("reddit_browser:search", LiveTier::Attended),
        ),
        on(ProviderId::RedditBrowser, dependent),
    ];
    let search = Execution::Exited {
        code: Some(0),
        stdout: json!({
            "results": [{"url": "https://example.com/"}],
            "items": [
                {"ref": "reddit:post/nsfw", "nsfw": true},
                {"ref": "reddit:post/unflagged"},
                {"ref": "reddit:post/safe", "nsfw": false}
            ]
        })
        .to_string()
        .into_bytes(),
    };

    let (_, started, _) = run(&declared, vec![search, passed()], long());

    assert_eq!(started[1].arguments[..2], ["fetch", "reddit:post/safe"]);
}

#[test]
fn a_check_the_time_limit_left_unstarted_counts_as_failed() {
    let declared = [on(
        ProviderId::Exa,
        check("exa:docs_search", LiveTier::Metered),
    )];

    let (results, started, exit) = run(&declared, Vec::new(), Deadline::new(Duration::ZERO));

    assert_eq!(
        (statuses(&results), started.len(), exit),
        (
            vec![("exa:docs_search", "not_started", Some("deadline"))],
            0,
            4
        )
    );
}

#[test]
fn the_summary_counts_each_status_and_the_unselected_checks() {
    let declared = [
        on(ProviderId::Exa, check("exa:docs_search", LiveTier::Metered)),
        on(ProviderId::Exa, check("exa:similar", LiveTier::Metered)),
        on(
            ProviderId::Tavily,
            check("tavily:site_map", LiveTier::Metered),
        ),
        on(ProviderId::Jina, check("jina:web_fetch", LiveTier::Metered)),
    ];
    let mut executions = vec![passed(), failed("auth", "rejected")].into_iter();
    let mut execute = |_: &Prepared, _: Duration| executions.next().expect("an execution");

    let results = run_checks(
        &declared,
        &[true, true, true, false],
        &[
            None,
            None,
            Some("providers.tavily.keys has no configured credentials".into()),
            None,
        ],
        long(),
        &[],
        &mut execute,
        None,
    );

    assert_eq!(
        serde_json::to_value(summarize(&results, declared.len())).expect("serialize summary"),
        json!({"passed": 1, "failed": 1, "unconfigured": 1, "not_started": 0, "unselected": 1})
    );
}

#[test]
fn a_run_where_no_selected_check_is_configured_exits_3() {
    let declared = [on(
        ProviderId::Exa,
        check("exa:docs_search", LiveTier::Metered),
    )];
    let mut execute = |_: &Prepared, _: Duration| -> Execution { unreachable!("nothing starts") };

    let results = run_checks(
        &declared,
        &[true],
        &[Some(
            "providers.exa.keys has no configured credentials".into(),
        )],
        long(),
        &[],
        &mut execute,
        None,
    );

    assert_eq!(
        (results[0].outcome.status, exit_code(&results)),
        (Status::Unconfigured, 3)
    );
}

#[test]
fn a_failure_message_never_shows_a_configured_credential() {
    let declared = [on(
        ProviderId::Exa,
        check("exa:docs_search", LiveTier::Metered),
    )];

    let (results, ..) = run(
        &declared,
        vec![failed("auth", "key sk-live-secret was rejected")],
        long(),
    );

    let message = results[0]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("auth") && !message.contains("sk-live-secret"),
        "{message}"
    );
}

#[test]
fn every_check_command_runs_once_and_outside_the_search_result_journal() {
    for entry in declared() {
        let prepared = prepare(&entry.check, Some("input"), None);
        let environment = |name: &str| {
            prepared
                .environment
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
        };

        assert_eq!(
            (
                environment("FORAGER_JOURNAL__ENABLED"),
                environment("FORAGER_RETRY__MAX_ATTEMPTS")
            ),
            (Some("false"), Some("1")),
            "{}",
            entry.check.id
        );
    }
}

#[test]
fn the_anonymous_web_fetch_check_removes_the_keys_of_its_provider() {
    let anonymous = declared()
        .into_iter()
        .find(|entry| matches!(entry.check.command.routes, RoutePin::AnonymousWebFetch(_)))
        .expect("an anonymous Web Fetch check");

    let prepared = prepare(&anonymous.check, None, None);

    assert!(
        prepared
            .environment
            .contains(&("FORAGER_PROVIDERS__JINA__KEYS".to_owned(), "[]".to_owned())),
        "{:?}",
        prepared.environment
    );
}

#[cfg(unix)]
#[test]
fn a_command_past_its_time_limit_is_killed() {
    let command = Prepared {
        arguments: vec!["30".into()],
        environment: Vec::new(),
    };

    let execution = super::execute(std::path::Path::new("/bin/sleep"), &command, Duration::ZERO);

    assert!(matches!(execution, Execution::TimedOut), "{execution:?}");
}

#[test]
fn every_declared_check_has_a_unique_identifier() {
    let declared = declared();
    let mut ids = declared
        .iter()
        .map(|entry| entry.check.id)
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();

    assert_eq!(ids.len(), declared.len());
}

#[test]
fn a_listing_shows_every_declared_check_and_whether_the_selection_runs_it() {
    let listing = super::list(&LiveSelection {
        tiers: vec![LiveTier::Free],
        ..LiveSelection::default()
    })
    .expect("the selection is accepted");
    let listing = serde_json::to_value(listing).expect("serialize listing");
    let checks = listing["checks"].as_array().expect("checks");

    assert_eq!(
        (
            checks.len(),
            checks
                .iter()
                .all(|check| (check["tier"] == "free") == (check["selected"] == true)),
        ),
        (declared().len(), true)
    );
}

fn failed_with_status(error_kind: &str, http_status: u16) -> Execution {
    Execution::Exited {
        code: Some(4),
        stdout: json!({
            "error_kind": error_kind,
            "message": "upstream refused",
            "provider_attempts": [
                {"provider": "exa", "error_kind": error_kind, "http_status": http_status},
            ],
        })
        .to_string()
        .into_bytes(),
    }
}

fn broke_contract() -> Execution {
    Execution::Exited {
        code: Some(0),
        stdout: json!({
            "results": [{"url": "", "title": "MARKER-CONTENT"}],
            "provider_attempts": [{"provider": "exa", "error_kind": null, "http_status": 200}],
        })
        .to_string()
        .into_bytes(),
    }
}

#[test]
fn a_failure_reports_its_rule_path_observation_error_kind_http_status_and_duration() {
    let declared = [on(
        ProviderId::Exa,
        check("exa:docs_search", LiveTier::Metered),
    )];

    let (contract, ..) = run(&declared, vec![broke_contract()], long());
    let (command, ..) = run(
        &declared,
        vec![failed_with_status("quota_exhausted", 429)],
        long(),
    );
    let reported = |result: &Value| {
        (
            result["error_kind"].clone(),
            result["http_status"].clone(),
            result["detail"].clone(),
            result["duration_ms"].is_u64(),
        )
    };

    assert_eq!(
        (reported(&contract[0]), reported(&command[0])),
        (
            (
                json!("contract"),
                json!(200),
                json!({
                    "rule": "nonblank",
                    "path": "results[].url",
                    "at": "results[0].url",
                    "observed": {"type": "string", "length": 0},
                }),
                true
            ),
            (
                json!("quota_exhausted"),
                json!(429),
                json!({"rule": "command_succeeds"}),
                true
            ),
        )
    );
}

#[test]
fn an_unattended_run_reruns_a_failed_check_once_after_the_delay_and_the_rerun_decides() {
    let declared = [on(
        ProviderId::Exa,
        check("exa:docs_search", LiveTier::Metered),
    )];
    let outcome = |executions| {
        let (results, started, exit, waited) = run_with(&declared, executions, long(), true);
        (
            results[0]["status"].clone(),
            results[0]["rerun"].clone(),
            results[0]["confirmed"].clone(),
            started.len(),
            exit,
            waited,
        )
    };

    assert_eq!(
        (
            outcome(vec![broke_contract(), passed()]),
            outcome(vec![broke_contract(), broke_contract()]),
        ),
        (
            (
                json!("passed"),
                json!(true),
                Value::Null,
                2,
                0,
                vec![CONFIRMATION_DELAY]
            ),
            (
                json!("failed"),
                json!(true),
                json!(true),
                2,
                4,
                vec![CONFIRMATION_DELAY]
            ),
        )
    );
}

#[test]
fn the_failed_checks_of_an_unattended_run_share_one_delay_before_their_reruns() {
    let declared = [
        on(ProviderId::Exa, check("exa:docs_search", LiveTier::Metered)),
        on(ProviderId::Exa, check("exa:similar", LiveTier::Metered)),
    ];

    let (results, started, exit, waited) = run_with(
        &declared,
        vec![broke_contract(), broke_contract(), passed(), passed()],
        long(),
        true,
    );

    assert_eq!(
        (
            statuses(&results),
            results[1]["attempts"].clone(),
            started.len(),
            exit,
            waited
        ),
        (
            vec![
                ("exa:docs_search", "passed", None),
                ("exa:similar", "passed", None)
            ],
            json!(2),
            4,
            0,
            vec![CONFIRMATION_DELAY]
        )
    );
}

#[test]
fn an_authentication_or_parameter_failure_is_confirmed_without_a_rerun() {
    let declared = [on(
        ProviderId::Exa,
        check("exa:docs_search", LiveTier::Metered),
    )];
    for error_kind in ["auth", "parameter"] {
        let (results, started, exit, waited) = run_with(
            &declared,
            vec![failed(error_kind, "rejected")],
            long(),
            true,
        );

        assert_eq!(
            (
                results[0]["status"].as_str(),
                results[0].get("rerun"),
                results[0]["confirmed"].as_bool(),
                started.len(),
                exit,
                waited
            ),
            (Some("failed"), None, Some(true), 1, 4, Vec::new()),
            "{error_kind}"
        );
    }
}

#[test]
fn a_run_that_is_not_unattended_reruns_nothing() {
    let declared = [on(
        ProviderId::Exa,
        check("exa:docs_search", LiveTier::Metered),
    )];

    let (results, started, exit, waited) =
        run_with(&declared, vec![broke_contract()], long(), false);

    assert_eq!(
        (
            results[0].get("rerun"),
            results[0].get("confirmed"),
            started.len(),
            exit,
            waited
        ),
        (None, None, 1, 4, Vec::new())
    );
}

#[test]
fn a_browser_check_is_not_rerun_in_an_unattended_run() {
    let declared = [on(
        ProviderId::RedditBrowser,
        check("reddit_browser:search", LiveTier::Attended),
    )];

    let (results, started, exit, waited) =
        run_with(&declared, vec![broke_contract()], long(), true);

    assert_eq!(
        (results[0].get("rerun"), started.len(), exit, waited),
        (None, 1, 4, Vec::new())
    );
}

#[test]
fn a_failure_the_time_limit_leaves_no_room_to_rerun_fails_unconfirmed_without_waiting() {
    let declared = [on(
        ProviderId::Exa,
        check("exa:docs_search", LiveTier::Metered),
    )];
    // Room for the delay, but not for the check's own time limit after it.
    let deadline = Deadline::new(CONFIRMATION_DELAY + declared[0].check.time_limit / 2);

    let (results, started, exit, waited) =
        run_with(&declared, vec![broke_contract()], deadline, true);

    assert_eq!(
        (
            results[0]["status"].as_str(),
            results[0].get("rerun"),
            results[0]["confirmed"].as_bool(),
            started.len(),
            exit,
            waited
        ),
        (Some("failed"), None, Some(false), 1, 4, Vec::new())
    );
}

#[test]
fn a_check_whose_dependency_passed_only_on_its_rerun_is_not_started() {
    let dependent = LiveCheck {
        command: LiveCommand {
            arguments: &[
                LiveArgument::Text("fetch"),
                LiveArgument::Input(LiveInput::FirstItem("ref")),
            ],
            routes: RoutePin::Configured,
        },
        ..check("polymarket_api:fetch:live", LiveTier::Free).depends_on("polymarket_api:search")
    };
    let declared = [
        on(
            ProviderId::PolymarketApi,
            check("polymarket_api:search", LiveTier::Free),
        ),
        on(ProviderId::PolymarketApi, dependent),
    ];
    let found = Execution::Exited {
        code: Some(0),
        stdout: json!({"results": [{"url": "https://example.com/"}], "items": [{"ref": "polymarket:event"}]})
            .to_string()
            .into_bytes(),
    };

    let (results, started, exit, _) =
        run_with(&declared, vec![broke_contract(), found], long(), true);

    assert_eq!(
        (statuses(&results), started.len(), exit),
        (
            vec![
                ("polymarket_api:search", "passed", None),
                ("polymarket_api:fetch:live", "not_started", Some("no_input")),
            ],
            2,
            0
        )
    );
}
