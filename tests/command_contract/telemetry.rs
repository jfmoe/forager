//! Invocation Telemetry through the binary: the terminal state writes one record per retrieval
//! command, and what the record keeps is decided in the crate.

use std::fs;
use std::path::PathBuf;

use serde_json::{Value, json};

use crate::support::hn::config as hn_config;
use crate::support::samples::{sample, sample_json};
use crate::support::web_samples::jina;
use crate::support::{Fixture, Response, RunEnvironment, payload, stderr};

const TOKEN_MARKER: &str = "token-marker-key";

/// Jina alone at `url`, keyed with the token marker.
fn jina_config(url: &str) -> String {
    format!(
        "[providers.jina]\nurl = {url:?}\nkeys = [\"{TOKEN_MARKER}\"]\n\n\
         [capabilities.web_fetch]\norder = [\"jina\"]\n\n[retry]\nmax_attempts = 1\n"
    )
}

/// A page long enough to pass the content gate, each line carrying `marker`.
fn page(marker: &str) -> String {
    (1..=5)
        .map(|line| format!("Paragraph {line} of the page about {marker}, long enough to keep."))
        .collect::<Vec<_>>()
        .join("\n")
}

fn directory(environment: &RunEnvironment) -> PathBuf {
    environment.state_dir.join("forager/telemetry")
}

/// The stored text of every telemetry file, and each one parsed.
fn records(environment: &RunEnvironment) -> Vec<(String, Value)> {
    let Ok(entries) = fs::read_dir(directory(environment)) else {
        return Vec::new();
    };
    entries
        .map(|entry| {
            let text = fs::read_to_string(entry.expect("telemetry entry").path())
                .expect("read telemetry record");
            let value = serde_json::from_str(&text).expect("parse telemetry record");
            (text, value)
        })
        .collect()
}

#[test]
fn a_record_keeps_health_signals_and_none_of_the_text_a_command_handled() {
    let refusal = {
        let mut answer = sample_json("jina/error.json");
        answer["message"] = json!("refused: message-marker");
        answer["readableMessage"] = json!("refused: message-marker");
        Response::new(401, "application/json", &answer.to_string())
    };
    let jina_fixture = Fixture::start_sequence(vec![refusal, jina(&page("content-marker"))]);
    let xai = {
        let mut answer = sample_json("xai/error.json");
        answer["error"] = json!("refused: message-marker");
        Fixture::start_sequence(vec![Response::new(
            401,
            "application/json",
            &answer.to_string(),
        )])
    };
    let algolia = Fixture::start_sequence(vec![Response::json(200, &sample("hn_api/search.json"))]);
    // Both configurations end with the same retry table, which one document holds once.
    let environment = RunEnvironment::new(&format!(
        "{}\n[providers.xai]\nurl = \"{}/v1\"\nkeys = [\"{TOKEN_MARKER}\"]\nmodel = \"model-marker\"\n\n{}",
        jina_config(&jina_fixture.url).replace("[retry]\nmax_attempts = 1\n", ""),
        xai.url,
        hn_config(&algolia.url, "http://127.0.0.1:9"),
    ));

    let runs = [
        environment.run(&[
            "fetch",
            "https://reader:userinfo-marker@Example.test:8443/path-marker?query-marker=1",
        ]),
        environment.run(&["search", "a question about query-marker"]),
        environment.run(&["platform", "hn", "search", "query-marker"]),
    ];
    jina_fixture.finish_all();
    xai.finish_all();
    algolia.finish();
    let stored = records(&environment);
    let fetched = stored
        .iter()
        .map(|(_, record)| record)
        .find(|record| record["command"] == "fetch")
        .expect("a fetch record");
    let markers = [
        "userinfo-marker",
        "path-marker",
        "query-marker",
        TOKEN_MARKER,
        "content-marker",
        "message-marker",
        "127.0.0.1",
        "model-marker",
        "hn:8863",
    ];

    assert_eq!(
        (
            runs.iter()
                .map(|output| output.status.code())
                .collect::<Vec<_>>(),
            stored.len(),
            &fetched["host"],
            &fetched["exit_code"],
            &fetched["succeeded"],
            fetched["attempts"].as_array().map(Vec::len),
            markers
                .iter()
                .filter(|marker| stored.iter().any(|(text, _)| text.contains(*marker)))
                .collect::<Vec<_>>(),
        ),
        (
            vec![Some(0), Some(4), Some(0)],
            3,
            &json!("example.test"),
            &json!(0),
            &json!("jina"),
            Some(2),
            Vec::<&&str>::new(),
        ),
        "stderr: {:?}",
        runs.iter().map(stderr).collect::<Vec<_>>()
    );
}

#[test]
fn the_record_keeps_the_exit_code_the_output_copy_settled() {
    let fixture = Fixture::start_sequence(vec![jina(&page("an article"))]);
    let environment = RunEnvironment::new(&jina_config(&fixture.url));
    let unwritable = tempfile::tempdir().expect("a directory where the output file should be");

    let output = environment.run(&[
        "fetch",
        "https://example.test/article",
        "--output",
        unwritable.path().to_str().expect("UTF-8 path"),
    ]);
    fixture.finish();
    let stored = records(&environment);

    assert_eq!(
        (
            output.status.code(),
            stored
                .iter()
                .map(|(_, record)| (record["exit_code"].clone(), record["error_kind"].clone()))
                .collect::<Vec<_>>(),
        ),
        (Some(3), vec![(json!(3), Value::Null)])
    );
}

#[test]
fn telemetry_turned_off_writes_no_record() {
    let fixture = Fixture::start_sequence(vec![jina(&page("an article"))]);
    let environment = RunEnvironment::new(&jina_config(&fixture.url));

    let output = environment.run_with_env(
        &["fetch", "https://example.test/article"],
        &[("FORAGER_TELEMETRY__ENABLED", "false")],
    );
    fixture.finish();

    assert_eq!(
        (output.status.code(), directory(&environment).exists()),
        (Some(0), false),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_record_that_cannot_be_written_leaves_the_output_and_exit_code_unchanged() {
    let content = page("an article");
    let fixture = Fixture::start_sequence(vec![jina(&content)]);
    let environment = RunEnvironment::new(&jina_config(&fixture.url));
    fs::create_dir_all(environment.state_dir.join("forager")).expect("state directory");
    fs::write(directory(&environment), "").expect("a file where the directory should be");

    let output = environment.run(&["fetch", "https://example.test/article"]);
    fixture.finish();

    assert_eq!(
        (
            output.status.code(),
            payload(&output)["content"].as_str(),
            stderr(&output)
        ),
        (Some(0), Some(content.as_str()), String::new())
    );
}

#[test]
fn concurrent_invocations_each_leave_a_readable_record() {
    let environment = RunEnvironment::new("");
    let count = 4;

    let codes = std::thread::scope(|scope| {
        let runs = (0..count)
            .map(|_| scope.spawn(|| environment.run(&["map", "https://example.test"])))
            .collect::<Vec<_>>();
        runs.into_iter()
            .map(|run| run.join().expect("run finished").status.code())
            .collect::<Vec<_>>()
    });
    let stored = records(&environment);

    assert_eq!(
        (
            codes,
            stored
                .iter()
                .map(|(_, record)| (record["command"].clone(), record["error_kind"].clone()))
                .collect::<Vec<_>>(),
        ),
        (
            vec![Some(3); count],
            vec![(json!("map"), json!("config")); count]
        )
    );
}
