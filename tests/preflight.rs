mod support;

use serde_json::Value;

use support::RunEnvironment;

#[test]
fn json_preflight_failures_are_machine_readable_without_a_journal_config() {
    let invalid_config = RunEnvironment::new("invalid = [");
    let invalid_plan = tempfile::NamedTempFile::new().expect("create invalid plan");
    std::fs::write(invalid_plan.path(), "not JSON").expect("write invalid plan");
    let valid_config = RunEnvironment::new("");
    let plan_path = invalid_plan.path().to_string_lossy().into_owned();
    let cases = [
        (
            &invalid_config,
            vec!["search", "query", "--capabilities", "none"],
            3,
            "config",
        ),
        (
            &invalid_config,
            vec!["fetch", "https://example.test"],
            3,
            "config",
        ),
        (
            &valid_config,
            vec!["research", "query", "--plan", &plan_path],
            2,
            "parameter",
        ),
    ];

    for (environment, arguments, exit_code, error_kind) in cases {
        let output = environment.run(&arguments);
        let payload: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "{arguments:?} did not return JSON: {error}; stdout: {}; stderr: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });

        assert_eq!(
            (
                output.status.code(),
                &payload["error_kind"],
                payload["message"].as_str().map(str::len),
                &payload["journal_ref"],
                &payload["journal_status"],
            ),
            (
                Some(exit_code),
                &Value::String(error_kind.into()),
                payload["message"].as_str().map(str::len),
                &Value::Null,
                &Value::String("unavailable".into()),
            ),
            "arguments: {arguments:?}"
        );
        assert!(output.stderr.is_empty(), "arguments: {arguments:?}");
        assert!(
            payload["message"]
                .as_str()
                .is_some_and(|message| !message.is_empty() && message.chars().count() <= 500),
            "arguments: {arguments:?}; payload: {payload}"
        );
        assert!(
            !environment.state_dir.join("forager/journal").exists(),
            "arguments: {arguments:?} created a default journal"
        );
    }
}

#[test]
fn non_json_preflight_failures_remain_on_standard_error() {
    let invalid_config = RunEnvironment::new("invalid = [");
    let invalid_plan = tempfile::NamedTempFile::new().expect("create invalid plan");
    std::fs::write(invalid_plan.path(), "not JSON").expect("write invalid plan");
    let valid_config = RunEnvironment::new("");
    let plan_path = invalid_plan.path().to_string_lossy().into_owned();
    let cases = [
        (
            &invalid_config,
            vec!["search", "query", "--format", "markdown"],
            3,
            "config_error:",
        ),
        (
            &invalid_config,
            vec!["fetch", "https://example.test", "--format", "content"],
            3,
            "config_error:",
        ),
        (
            &valid_config,
            vec![
                "research", "query", "--plan", &plan_path, "--format", "markdown",
            ],
            2,
            "argument_error:",
        ),
    ];

    for (environment, arguments, exit_code, category) in cases {
        let output = environment.run(&arguments);

        assert_eq!(output.status.code(), Some(exit_code), "{arguments:?}");
        assert!(output.stdout.is_empty(), "arguments: {arguments:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).starts_with(category),
            "arguments: {arguments:?}; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn recordable_search_preflight_failure_is_journaled_once() {
    let environment = RunEnvironment::new("");

    let output = environment.run(&["search", "query", "--capabilities", "none"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let journal_ref = payload["journal_ref"].as_str().expect("journal reference");
    let journal: Value =
        serde_json::from_slice(&std::fs::read(journal_ref).expect("read preflight journal record"))
            .expect("parse preflight journal record");
    let journal_dir = environment.state_dir.join("forager/journal");

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            &payload["journal_status"],
            std::fs::read_dir(&journal_dir)
                .expect("read journal directory")
                .count(),
            &journal["result"]["status"],
            &journal["result"]["error_kind"],
            &journal["result"]["query"],
            journal["execution"]["provider_attempts"]
                .as_array()
                .map(Vec::len),
        ),
        (
            Some(3),
            &Value::String("config".into()),
            &Value::String("written".into()),
            1,
            &Value::String("error".into()),
            &Value::String("config".into()),
            &Value::String("query".into()),
            Some(0),
        )
    );
    assert!(output.stderr.is_empty());
}
