mod support;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::process::Output;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use support::{Fixture, Response, RunEnvironment};

fn feed(total: u64, ids: &[&str]) -> String {
    let mut entries = String::new();
    for id in ids {
        let _ = write!(
            entries,
            r#"<entry>
    <id>http://arxiv.org/abs/{id}</id>
    <title>Paper {id}</title>
    <summary>Abstract of {id}.</summary>
    <published>2024-01-02T18:59:59Z</published>
    <updated>2024-01-03T00:00:00Z</updated>
    <author><name>Ada Lovelace</name></author>
    <link href="https://arxiv.org/pdf/{id}" rel="related" type="application/pdf" title="pdf"/>
    <arxiv:primary_category term="cs.AI"/>
    <category term="cs.AI" scheme="http://arxiv.org/schemas/atom"/>
  </entry>"#
        );
    }
    format!(
        r#"<?xml version='1.0' encoding='UTF-8'?>
<feed xmlns:opensearch="http://a9.com/-/spec/opensearch/1.1/" xmlns:arxiv="http://arxiv.org/schemas/atom" xmlns="http://www.w3.org/2005/Atom">
  <opensearch:totalResults>{total}</opensearch:totalResults>
  {entries}
</feed>"#
    )
}

fn atom(status: u16, body: &str) -> Response {
    Response::new(status, "application/atom+xml", body)
}

fn config(url: &str) -> String {
    format!("[providers.arxiv_api]\nurl = \"{url}/api/query\"\n")
}

fn search(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "arxiv", "search"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

fn payload(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "parse JSON stdout: {error}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn query_pairs(request: &str) -> BTreeMap<String, String> {
    let target = request.split_whitespace().nth(1).expect("request target");
    reqwest::Url::parse(&format!("http://fixture.test{target}"))
        .expect("request URL")
        .query_pairs()
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}

#[test]
fn search_returns_abstract_depth_items_with_refs_and_metadata() {
    let fixture = Fixture::start_sequence(vec![atom(200, &feed(1, &["2401.01234v2"]))]);
    let environment = RunEnvironment::new(&config(&fixture.url));

    let output = search(&environment, &["electron"]);
    fixture.finish();

    assert_eq!(
        (output.status.code(), payload(&output)),
        (
            Some(0),
            json!({
                "platform": "arxiv",
                "provider": "arxiv_api",
                "items": [{
                    "ref": "arxiv:2401.01234v2",
                    "url": "https://arxiv.org/abs/2401.01234v2",
                    "depth": "abstract",
                    "title": "Paper 2401.01234v2",
                    "authors": ["Ada Lovelace"],
                    "published": "2024-01-02T18:59:59Z",
                    "abstract": "Abstract of 2401.01234v2.",
                    "updated": "2024-01-03T00:00:00Z",
                    "primary_category": "cs.AI",
                    "categories": ["cs.AI"],
                    "doi": null,
                    "journal_ref": null,
                    "comment": null,
                    "pdf_url": "https://arxiv.org/pdf/2401.01234v2"
                }],
                "next_cursor": null
            })
        )
    );
}

/// Runs one page in its own environment and returns the page and the received query.
fn page(arguments: &[&str], response: &str) -> (Value, BTreeMap<String, String>) {
    let fixture = Fixture::start_sequence(vec![atom(200, response)]);
    let environment = RunEnvironment::new(&config(&fixture.url));
    let output = search(&environment, arguments);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    (payload(&output), query_pairs(&fixture.finish()))
}

#[test]
fn cursors_page_through_results_and_restore_the_original_request() {
    let arguments = [
        r#""neutrino mass" hierarchy"#,
        "--category",
        "hep-ph",
        "--sort",
        "updated",
        "--limit",
        "2",
    ];
    let (first, first_query) = page(&arguments, &feed(5, &["2401.00001v1", "2401.00002v1"]));
    let first_cursor = first["next_cursor"].as_str().expect("first page cursor");
    let (middle, middle_query) = page(
        &["--cursor", first_cursor],
        &feed(5, &["2401.00003v1", "2401.00004v1"]),
    );
    let middle_cursor = middle["next_cursor"].as_str().expect("middle page cursor");
    let (last, last_query) = page(&["--cursor", middle_cursor], &feed(5, &["2401.00005v1"]));
    let restored = |query: &BTreeMap<String, String>| {
        let mut query = query.clone();
        query.remove("start");
        query
    };

    assert_eq!(
        (
            first_cursor.starts_with("v1.arxiv_api."),
            [&first_query, &middle_query, &last_query].map(|query| query["start"].clone()),
            restored(&middle_query) == restored(&first_query),
            restored(&last_query) == restored(&first_query),
            &last["next_cursor"],
            last["items"].as_array().map(Vec::len),
        ),
        (
            true,
            ["0".to_owned(), "2".to_owned(), "4".to_owned()],
            true,
            true,
            &Value::Null,
            Some(1),
        )
    );
}

fn assert_preflight_exit(config: &str, arguments: &[&str], exit_code: i32, message: &str) {
    let fixture = Fixture::start_canary();
    let environment = RunEnvironment::new(&config.replace("{url}", &fixture.url));

    let output = search(&environment, arguments);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        (
            output.status.code(),
            stderr.contains(message),
            fixture.finish_all().len()
        ),
        (Some(exit_code), true, 0),
        "arguments: {arguments:?}\nstderr: {stderr}"
    );
}

const CURSOR: &str = "v1.arxiv_api.eyJxdWVyeSI6IngiLCJsaW1pdCI6Miwib3B0aW9ucyI6eyJwbGF0Zm9ybSI6ImFyeGl2In0sInBhZ2UiOiIyIn0";

#[test]
fn a_cursor_rejects_an_explicit_query_option_or_limit() {
    for arguments in [
        &["x", "--cursor", CURSOR][..],
        &["--cursor", CURSOR, "--category", "cs.AI"],
        &["--cursor", CURSOR, "--sort", "relevance"],
        &["--cursor", CURSOR, "--limit", "10"],
    ] {
        assert_preflight_exit(
            "[providers.arxiv_api]\nurl = \"{url}\"\n",
            arguments,
            2,
            "cannot be used with",
        );
    }
}

#[test]
fn a_cursor_accepts_general_flags() {
    let fixture = Fixture::start_sequence(vec![atom(200, &feed(0, &[]))]);
    let environment = RunEnvironment::new(&config(&fixture.url));

    let output = search(
        &environment,
        &[
            "--cursor",
            CURSOR,
            "--timeout",
            "30",
            "--format",
            "markdown",
        ],
    );
    let query = query_pairs(&fixture.finish());

    assert_eq!(
        (
            output.status.code(),
            query["start"].as_str(),
            query["max_results"].as_str()
        ),
        (Some(0), "2", "2"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn an_undecodable_cursor_is_an_argument_error() {
    assert_preflight_exit(
        "[providers.arxiv_api]\nurl = \"{url}\"\n",
        &["--cursor", "v1.arxiv_api.not-a-payload!"],
        2,
        "cursor cannot be decoded",
    );
}

#[test]
fn a_cursor_whose_route_left_the_order_is_an_argument_error() {
    assert_preflight_exit(
        "[providers.arxiv_api]\nurl = \"{url}\"\n[platforms.arxiv]\norder = []\n",
        &["--cursor", CURSOR],
        2,
        "no longer available",
    );
}

#[test]
fn an_empty_platform_order_is_a_configuration_error() {
    assert_preflight_exit(
        "[providers.arxiv_api]\nurl = \"{url}\"\n[platforms.arxiv]\norder = []\n",
        &["x"],
        3,
        "platforms.arxiv.order has no configured route for arxiv search",
    );
}

#[test]
fn a_route_of_another_platform_in_the_order_is_a_configuration_error() {
    assert_preflight_exit(
        "[providers.arxiv_api]\nurl = \"{url}\"\n[platforms.arxiv]\norder = [\"tavily\"]\n",
        &["x"],
        3,
        "platforms.arxiv.order",
    );
}

/// Records a reservation a minute ahead, as if another process had just reserved the window.
fn reserve_arxiv_window(environment: &RunEnvironment) {
    let reserved_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("wall clock")
        .as_millis()
        + Duration::from_mins(1).as_millis();
    let directory = environment.state_dir.join("forager");
    fs::create_dir_all(&directory).expect("create state directory");
    fs::write(
        directory.join("rate_limit_state.json"),
        json!({
            "schema_version": 1,
            "routes": {"arxiv_api": {"reserved_at_ms": reserved_at_ms}}
        })
        .to_string(),
    )
    .expect("write rate limit state");
}

#[test]
fn a_request_window_beyond_the_deadline_times_out_without_sending() {
    let fixture = Fixture::start_canary();
    let environment = RunEnvironment::new(&config(&fixture.url));
    reserve_arxiv_window(&environment);

    let output = search(&environment, &["x", "--timeout", "1"]);
    let payload = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            fixture.finish_all().len()
        ),
        (Some(4), &Value::String("timeout".into()), 0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
