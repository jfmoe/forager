//! The `ssrn_browser` route against a fake `opencli` executable. The fake records its argv and
//! answers with a preset envelope and exit code; these tests prove the transport and the route
//! adapter, not the JavaScript adapter, which only the live smoke exercises.

mod support;

use std::fs;
use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use support::opencli::FakeOpenCli;
use support::{Fixture, Response, RunEnvironment};

const SESSION_FLAGS: [&str; 8] = [
    "-f",
    "json",
    "--window",
    "background",
    "--site-session",
    "ephemeral",
    "--keep-tab",
    "false",
];
fn payload(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "parse JSON stdout: {error}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// Splits an argv into the route arguments and the transport's `--timeout` value, and checks
/// that the session flags end it.
fn route_arguments(call: &[String]) -> (Vec<String>, u64) {
    let flags = &call[call.len() - SESSION_FLAGS.len()..];
    assert_eq!(flags, SESSION_FLAGS, "argv {call:?}");
    let rest = &call[..call.len() - SESSION_FLAGS.len()];
    let [arguments @ .., flag, timeout] = rest else {
        panic!("argv {call:?} lacks --timeout");
    };
    assert_eq!(flag, "--timeout", "argv {call:?}");
    (
        arguments.to_vec(),
        timeout.parse().expect("integer --timeout"),
    )
}

fn result(id: u64) -> Value {
    json!({
        "url": format!("https://papers.ssrn.com/sol3/papers.cfm?abstract_id={id}"),
        "title": format!("  Paper   {id} "),
        "authors": ["Gary  Antonacci"],
        "details": "Number of pages: 37 • Posted: 20 Apr 2012",
        "snippets": ["Dual  momentum", "absolute momentum"]
    })
}

fn results_page(native_page: u64, first: u64, last: u64, total: u64) -> Value {
    let suffix = if native_page == 1 {
        String::new()
    } else {
        format!("&page={native_page}")
    };
    json!({
        "url": format!("https://papers.ssrn.com/searchresults.cfm?term=dual+momentum{suffix}"),
        "term": "dual momentum",
        "current_page": native_page.to_string(),
        "range": format!("Displaying results {first} to {last} of {total}"),
        "next_page": last < total,
        "results": (first..=last).map(result).collect::<Vec<_>>()
    })
}

fn paper_page(id: &str, abstract_paragraphs: &[&str]) -> Value {
    json!({
        "url": format!("https://papers.ssrn.com/sol3/papers.cfm?abstract_id={id}"),
        "canonical_url": format!("https://papers.ssrn.com/sol3/papers.cfm?abstract_id={id}"),
        "doi": format!("10.2139/ssrn.{id}"),
        "title": "Risk Premia Harvesting Through Dual Momentum",
        "authors": ["Gary Antonacci"],
        "abstract_paragraphs": abstract_paragraphs,
        "notes": ["37 Pages", "Posted: 19 Apr 2012", "Last revised: 23 May 2017"],
        "date_written": "Date Written: October 1, 2016"
    })
}

fn browser_only(fake: &FakeOpenCli) -> RunEnvironment {
    RunEnvironment::new(&fake.config("[\"ssrn_browser\"]"))
}

#[test]
fn search_runs_the_adapter_search_command_with_the_session_flags() {
    let fake = FakeOpenCli::envelope("ok", &results_page(1, 1, 50, 10_000));
    let environment = browser_only(&fake);

    let output = environment.run(&[
        "platform",
        "ssrn",
        "search",
        "dual momentum",
        "--limit",
        "2",
    ]);
    let calls = fake.calls();
    let (arguments, timeout) = route_arguments(&calls[0]);

    assert_eq!(
        (output.status.code(), calls.len(), arguments),
        (
            Some(0),
            1,
            ["ssrn", "search", "--query", "dual momentum", "--page", "1"]
                .map(str::to_owned)
                .to_vec()
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        (1..=85).contains(&timeout),
        "the command timeout leaves the cleanup reserve: {timeout}"
    );
}

#[test]
fn search_maps_page_facts_to_snippet_items() {
    let fake = FakeOpenCli::envelope("ok", &results_page(1, 1, 50, 10_000));
    let environment = browser_only(&fake);

    let output = environment.run(&[
        "platform",
        "ssrn",
        "search",
        "dual momentum",
        "--limit",
        "1",
    ]);
    let payload = payload(&output);

    assert_eq!(
        (&payload["provider"], &payload["items"]),
        (
            &json!("ssrn_browser"),
            &json!([{
                "ref": "ssrn:1",
                "url": "https://papers.ssrn.com/sol3/papers.cfm?abstract_id=1",
                "depth": "snippet",
                "title": "Paper 1",
                "authors": ["Gary Antonacci"],
                "published": "2012-04-20",
                "abstract": null,
                "snippet": "Dual momentum … absolute momentum",
                "doi": null,
                "crossref_type": null,
                "crossref_created": null,
                "posted": "20 Apr 2012",
                "last_revised": null,
                "date_written": null
            }])
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_cursor_resumes_at_the_consumed_position_on_the_same_native_page() {
    let fake = FakeOpenCli::envelope("ok", &results_page(1, 1, 50, 10_000));
    let environment = browser_only(&fake);
    let first = environment.run(&[
        "platform",
        "ssrn",
        "search",
        "dual momentum",
        "--limit",
        "2",
    ]);
    let cursor = payload(&first)["next_cursor"]
        .as_str()
        .expect("next cursor")
        .to_owned();

    let second = environment.run(&["platform", "ssrn", "search", "--cursor", &cursor]);
    let refs = payload(&second)["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|item| item["ref"].as_str().expect("ref").to_owned())
        .collect::<Vec<_>>();
    let (arguments, _) = route_arguments(&fake.calls()[1]);

    assert_eq!(
        (
            cursor.starts_with("v1.ssrn_browser."),
            refs,
            arguments[5].clone()
        ),
        (
            true,
            vec!["ssrn:3".to_owned(), "ssrn:4".to_owned()],
            "1".to_owned()
        )
    );
}

#[test]
fn the_site_notice_of_no_results_is_a_legitimate_empty_page() {
    let fake = FakeOpenCli::envelope(
        "no_results",
        &json!({
            "url": "https://papers.ssrn.com/searchresults.cfm?term=zzqx",
            "term": "zzqx"
        }),
    );
    let environment = browser_only(&fake);

    let output = environment.run(&["platform", "ssrn", "search", "zzqx"]);
    let payload = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &payload["items"],
            &payload["next_cursor"]
        ),
        (Some(0), &json!([]), &Value::Null)
    );
}

#[test]
fn an_unrecognized_results_page_fails_instead_of_returning_no_items() {
    let fake = FakeOpenCli::envelope(
        "ok",
        &json!({
            "url": "https://papers.ssrn.com/searchresults.cfm?term=dual+momentum",
            "term": "dual momentum",
            "results": []
        }),
    );
    let environment = browser_only(&fake);

    let output = environment.run(&["platform", "ssrn", "search", "dual momentum"]);

    assert_eq!(
        (output.status.code(), payload(&output)["error_kind"].clone()),
        (Some(4), json!("runtime"))
    );
}

/// Runs a browser-only search against a fake that exits with `code`, and returns the error kind
/// and message.
fn failed_search(stdout: &str, stderr: &str, code: i32) -> (Value, String) {
    let fake = FakeOpenCli::answering(stdout, stderr, code);
    let environment = browser_only(&fake);
    let output = environment.run(&["platform", "ssrn", "search", "dual momentum", "--verbose"]);
    let payload = payload(&output);
    assert_eq!(
        payload["provider_attempts"][0]["transport"],
        json!("process"),
        "{payload}"
    );
    (
        payload["error_kind"].clone(),
        payload["message"].as_str().unwrap_or_default().to_owned(),
    )
}

fn error_envelope(code: &str, message: &str, exit: i32) -> String {
    format!("ok: false\nerror:\n  code: {code}\n  message: {message}\n  exitCode: {exit}\n")
}

#[test]
fn opencli_exit_codes_map_to_error_kinds() {
    let kinds = [
        (
            error_envelope("EMPTY_RESULT", "ssrn/search returned no data", 66),
            66,
        ),
        (
            error_envelope("BROWSER_CONNECT", "Browser Bridge is not connected", 69),
            69,
        ),
        (
            error_envelope("TIMEOUT", "ssrn/search timed out after 40s", 75),
            75,
        ),
        (
            error_envelope(
                "AUTH_REQUIRED",
                "SSRN security verification did not clear",
                77,
            ),
            77,
        ),
        (error_envelope("COMMAND_EXEC", "boom", 1), 1),
    ]
    .map(|(stderr, code)| failed_search("", &stderr, code).0);

    assert_eq!(
        kinds,
        [
            json!("runtime"),
            json!("network"),
            json!("timeout"),
            json!("auth"),
            json!("runtime")
        ]
    );
}

#[test]
fn an_adapter_load_failure_is_a_runtime_failure_with_an_install_hint() {
    let (kind, message) = failed_search(
        "",
        &error_envelope("ADAPTER_LOAD", "cannot load ssrn/search.js", 69),
        69,
    );

    assert_eq!(
        (
            kind,
            message.contains(
                "copy the `opencli/ssrn` directory of the forager skill to `~/.opencli/clis/ssrn`"
            )
        ),
        (json!("runtime"), true),
        "{message}"
    );
}

#[test]
fn another_contract_version_is_a_runtime_failure_with_an_install_hint() {
    let stdout = json!({"contract": "forager-ssrn/0", "status": "ok", "data": {}}).to_string();

    let (kind, message) = failed_search(&stdout, "", 0);

    assert_eq!(
        (
            kind,
            message.contains("forager-ssrn/0"),
            message.contains("install or update")
        ),
        (json!("runtime"), true, true),
        "{message}"
    );
}

#[test]
fn a_missing_executable_is_a_runtime_failure() {
    let environment = RunEnvironment::new(
        "[providers.ssrn_browser]\ncommand = \"/nonexistent/opencli\"\n\n[platforms.ssrn]\norder = [\"ssrn_browser\"]\n",
    );

    let output = environment.run(&["platform", "ssrn", "search", "dual momentum"]);

    assert_eq!(
        (output.status.code(), payload(&output)["error_kind"].clone()),
        (Some(4), json!("runtime"))
    );
}

fn process_exists(pid: &str) -> bool {
    Command::new("/bin/kill")
        .args(["-0", pid.trim()])
        .output()
        .is_ok_and(|output| output.status.success())
}

#[test]
fn a_command_past_its_working_deadline_is_killed_with_its_process_group() {
    let fake = FakeOpenCli::hanging();
    let environment = RunEnvironment::new(&fake.config("[\"ssrn_browser\"]").replace(
        "[providers.ssrn_browser]\n",
        "[providers.ssrn_browser]\ntimeout = 7\n",
    ));
    let started = Instant::now();

    let output = environment.run(&["platform", "ssrn", "search", "dual momentum"]);
    let elapsed = started.elapsed();
    let pids = ["leader.pid", "child.pid"]
        .map(|name| fs::read_to_string(fake.path(name)).expect("recorded pid"));
    let deadline = Instant::now() + Duration::from_secs(3);
    while pids.iter().any(|pid| process_exists(pid)) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }

    assert_eq!(
        (
            output.status.code(),
            payload(&output)["error_kind"].clone(),
            pids.iter().any(|pid| process_exists(pid)),
        ),
        (Some(4), json!("timeout"), false)
    );
    assert!(
        elapsed < Duration::from_secs(6),
        "the working deadline stops the command before the attempt deadline: {elapsed:?}"
    );
}

#[test]
fn fetch_reads_the_paper_page_with_its_abstract() {
    let fake = FakeOpenCli::envelope(
        "ok",
        &paper_page("2042750", &["First  part.", " ", "Second."]),
    );
    let environment = browser_only(&fake);

    let output = environment.run(&[
        "platform",
        "ssrn",
        "fetch",
        "ssrn:2042750",
        "--depth",
        "abstract",
    ]);
    let (arguments, _) = route_arguments(&fake.calls()[0]);

    assert_eq!(
        (arguments, payload(&output)),
        (
            ["ssrn", "paper", "--id", "2042750"]
                .map(str::to_owned)
                .to_vec(),
            json!({
                "platform": "ssrn",
                "provider": "ssrn_browser",
                "ref": "ssrn:2042750",
                "url": "https://papers.ssrn.com/sol3/papers.cfm?abstract_id=2042750",
                "depth": "abstract",
                "title": "Risk Premia Harvesting Through Dual Momentum",
                "authors": ["Gary Antonacci"],
                "published": "2012-04-19",
                "abstract": "First part.\n\nSecond.",
                "snippet": null,
                "doi": null,
                "crossref_type": null,
                "crossref_created": null,
                "posted": "19 Apr 2012",
                "last_revised": "23 May 2017",
                "date_written": "October 1, 2016"
            })
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_page_for_another_paper_is_a_runtime_failure() {
    let fake = FakeOpenCli::envelope("ok", &paper_page("1", &["Abstract."]));
    let environment = browser_only(&fake);

    let output = environment.run(&["platform", "ssrn", "fetch", "ssrn:2042750"]);
    let payload = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            &payload["message"]
        ),
        (
            Some(4),
            &json!("runtime"),
            &json!("the SSRN page shows ssrn:1 for ssrn:2042750")
        )
    );
}

#[test]
fn an_unavailable_paper_is_a_parameter_failure_with_the_site_notice() {
    let fake = FakeOpenCli::envelope(
        "no_results",
        &json!({
            "url": "https://papers.ssrn.com/sol3/papers.cfm?abstract_id=999999999",
            "notice": "This paper is under review or has been removed from SSRN."
        }),
    );
    let environment = browser_only(&fake);

    let output = environment.run(&["platform", "ssrn", "fetch", "ssrn:999999999"]);
    let payload = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            &payload["message"]
        ),
        (
            Some(4),
            &json!("parameter"),
            &json!(
                "SSRN paper not available: ssrn:999999999 (This paper is under review or has been removed from SSRN.)"
            )
        )
    );
}

#[test]
fn a_missing_crossref_abstract_falls_through_to_the_browser() {
    let crossref = Fixture::start_sequence(vec![Response::json(
        200,
        &json!({
            "status": "ok",
            "message-type": "work",
            "message": {"DOI": "10.2139/ssrn.2042750", "title": ["Risk Premia"]}
        })
        .to_string(),
    )]);
    let fake = FakeOpenCli::envelope("ok", &paper_page("2042750", &["From the page."]));
    let environment = RunEnvironment::new(&format!(
        "[providers.ssrn_crossref]\nurl = {:?}\n\n{}",
        crossref.url,
        fake.config("[\"ssrn_crossref\", \"ssrn_browser\"]")
    ));

    let output = environment.run(&[
        "platform",
        "ssrn",
        "fetch",
        "ssrn:2042750",
        "--depth",
        "abstract",
        "--verbose",
    ]);
    let payload = payload(&output);
    let attempts = payload["provider_attempts"]
        .as_array()
        .expect("attempts")
        .iter()
        .map(|attempt| (attempt["provider"].clone(), attempt["error_kind"].clone()))
        .collect::<Vec<_>>();

    assert_eq!(
        (&payload["provider"], &payload["abstract"], attempts),
        (
            &json!("ssrn_browser"),
            &json!("From the page."),
            vec![
                (json!("ssrn_crossref"), json!("quality")),
                (json!("ssrn_browser"), Value::Null)
            ]
        )
    );
    assert_eq!(crossref.finish_all().len(), 1);
}

#[test]
fn full_text_skips_the_browser_route_before_it_runs() {
    let fake = FakeOpenCli::envelope("ok", &paper_page("2042750", &["Abstract."]));
    let environment = browser_only(&fake);

    let output = environment.run(&[
        "platform",
        "ssrn",
        "fetch",
        "ssrn:2042750",
        "--depth",
        "full_text",
    ]);

    assert_eq!((output.status.code(), fake.calls().len()), (Some(2), 0));
}

#[test]
fn the_default_order_never_runs_the_browser_route() {
    let crossref = Fixture::start_sequence(vec![Response::json(
        200,
        &json!({
            "status": "ok",
            "message-type": "work-list",
            "message": {"total-results": 0, "items": []}
        })
        .to_string(),
    )]);
    let fake = FakeOpenCli::envelope("ok", &results_page(1, 1, 50, 100));
    let environment = RunEnvironment::new(&format!(
        "[providers.ssrn_crossref]\nurl = {:?}\n\n[providers.ssrn_browser]\ncommand = {:?}\n",
        crossref.url,
        fake.executable().display().to_string()
    ));

    let output = environment.run(&["platform", "ssrn", "search", "dual momentum"]);

    assert_eq!(
        (
            output.status.code(),
            payload(&output)["provider"].clone(),
            fake.calls().len()
        ),
        (Some(0), json!("ssrn_crossref"), 0)
    );
    crossref.finish();
}
