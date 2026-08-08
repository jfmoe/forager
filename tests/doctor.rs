mod support;

use serde_json::Value;

use std::time::Duration;

use support::{Fixture, Response, RunEnvironment};

#[test]
fn shallow_doctor_reports_all_registry_providers_and_reuses_the_config_list_view() {
    let fixture = Fixture::start_sequence(reachable_responses(8));
    let environment = RunEnvironment::new(&shallow_config(&format!(
        "{}?token=url-secret",
        fixture.url
    )));

    let doctor = environment.run(&["doctor"]);
    let config_list = environment.run(&["config", "list"]);
    let payload: Value = serde_json::from_slice(&doctor.stdout).expect("parse doctor JSON");
    let config: Value = serde_json::from_slice(&config_list.stdout).expect("parse config JSON");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&doctor.stdout),
        String::from_utf8_lossy(&doctor.stderr)
    );

    assert_eq!(
        (
            doctor.status.code(),
            &payload["mode"],
            &payload["ok"],
            payload["providers"].as_array().map(Vec::len),
            &payload["config"],
            &payload["providers"][0]["provider"],
            &payload["providers"][0]["configured"],
            &payload["providers"][0]["key_count"],
            &payload["providers"][0]["source"],
            &payload["providers"][0]["reachable"],
        ),
        (
            Some(0),
            &Value::String("shallow".into()),
            &Value::Bool(true),
            Some(8),
            &config,
            &Value::String("xai".into()),
            &Value::Bool(true),
            &Value::from(1),
            &Value::String("file".into()),
            &Value::Bool(true),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&doctor.stderr)
    );
    for secret in [
        "xai-secret",
        "openai-secret",
        "exa-secret",
        "tavily-secret",
        "firecrawl-secret",
        "jina-secret",
        "context7-secret",
        "anysearch-secret",
        "url-secret",
    ] {
        assert!(!combined.contains(secret), "doctor leaked {secret}");
    }
    assert!(
        payload["permission_warnings"]
            .as_array()
            .is_some_and(|warnings| !warnings.is_empty())
    );
    assert_eq!(fixture.finish_all().len(), 8);
}

#[test]
fn doctor_provider_rejects_values_outside_the_compiled_registry() {
    let environment = RunEnvironment::new("");

    let output = environment.run(&["doctor", "--provider", "classifier"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("invalid provider"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn xai_deep_doctor_executes_one_responses_sse_probe() {
    let fixture = Fixture::start(
        200,
        "text/event-stream",
        "data: {\"type\":\"response.completed\",\"response\":{\"output\":[{\"content\":[{\"type\":\"output_text\",\"text\":\"ok\",\"annotations\":[]}]}]}}\n\n",
    );
    let environment = RunEnvironment::new(&format!(
        "[providers.xai]\nurl = {:?}\nkeys = [\"xai-key\"]\n",
        fixture.url
    ));

    let output = environment.run(&["doctor", "--provider", "xai"]);
    assert_deep_success(&output, "xai", &[("responses", "sse")]);
    let request = fixture.finish();
    assert!(request.starts_with("POST /responses "), "{request}");
    let request = request_json(&request);
    assert_eq!(
        request["input"],
        Value::String("Reply with exactly: ok".into())
    );
    assert_eq!(request["tools"], serde_json::json!([]));
    assert!(request.get("instructions").is_none());
}

#[test]
fn openai_compatible_deep_doctor_validates_non_stream_and_stream_shapes() {
    let fixture = Fixture::start_sequence(vec![
        Response::new(
            200,
            "application/json",
            r#"{"choices":[{"message":{"content":"ok"}}]}"#,
        ),
        Response::new(
            200,
            "text/event-stream",
            "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]\n\n",
        ),
    ]);
    let environment = RunEnvironment::new(&format!(
        "[providers.openai_compatible]\nurl = {:?}\nkeys = [\"openai-key\"]\nmodel = \"fixture-model\"\n",
        fixture.url
    ));

    let output = environment.run(&["doctor", "--provider", "openai_compatible"]);
    assert_deep_success(
        &output,
        "openai_compatible",
        &[("non_stream", "http"), ("stream", "sse")],
    );
    let requests = fixture.finish_all();
    let requests = requests
        .iter()
        .map(|request| request_json(request))
        .collect::<Vec<_>>();
    assert_eq!(requests[0]["stream"], Value::Bool(false));
    assert_eq!(requests[1]["stream"], Value::Bool(true));
    for request in requests {
        assert_eq!(
            request["messages"],
            serde_json::json!([{
                "role": "user",
                "content": "Reply with exactly: ok"
            }])
        );
    }
}

#[test]
fn openai_compatible_deep_doctor_rejects_stream_fallback_as_a_shape_failure() {
    let fixture = Fixture::start_sequence(vec![
        Response::new(
            200,
            "application/json",
            r#"{"choices":[{"message":{"content":"ok"}}]}"#,
        ),
        Response::new(200, "text/event-stream", "data: not-json\n\n"),
        Response::new(
            200,
            "application/json",
            r#"{"choices":[{"message":{"content":"fallback"}}]}"#,
        ),
    ]);
    let environment = RunEnvironment::new(&format!(
        "[providers.openai_compatible]\nurl = {:?}\nkeys = [\"openai-key\"]\nmodel = \"fixture-model\"\n",
        fixture.url
    ));

    let output = environment.run(&["doctor", "--provider", "openai_compatible"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            &payload["checks"][0]["ok"],
            &payload["checks"][1]["ok"],
        ),
        (
            Some(4),
            &Value::String("runtime".into()),
            &Value::Bool(true),
            &Value::Bool(false),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fixture.finish_all().len(), 3);
}

#[test]
fn exa_deep_doctor_executes_the_registry_search_probe() {
    let fixture = Fixture::start(200, "application/json", r#"{"results":[]}"#);
    let environment = RunEnvironment::new(&format!(
        "[providers.exa]\nurl = {:?}\nkeys = [\"exa-key\"]\n",
        fixture.url
    ));

    let output = environment.run(&["doctor", "--provider", "exa"]);
    assert_deep_success(&output, "exa", &[("search", "http")]);
    let request = fixture.finish();
    assert!(request.starts_with("POST /search "), "{request}");
}

#[test]
fn tavily_deep_doctor_executes_the_registry_search_probe() {
    let fixture = Fixture::start(
        200,
        "application/json",
        r#"{"results":[{"title":"ok","url":"https://example.test"}]}"#,
    );
    let environment = RunEnvironment::new(&format!(
        "[providers.tavily]\nurl = {:?}\nkeys = [\"tavily-key\"]\n",
        fixture.url
    ));

    let output = environment.run(&["doctor", "--provider", "tavily"]);
    assert_deep_success(&output, "tavily", &[("search", "http")]);
    let request = fixture.finish();
    assert!(request.starts_with("POST /search "), "{request}");
}

#[test]
fn firecrawl_deep_doctor_executes_the_registry_search_probe() {
    let fixture = Fixture::start(
        200,
        "application/json",
        r#"{"success":true,"data":{"web":[{"title":"ok","url":"https://example.test"}]}}"#,
    );
    let environment = RunEnvironment::new(&format!(
        "[providers.firecrawl]\nurl = {:?}\nkeys = [\"firecrawl-key\"]\n",
        fixture.url
    ));

    let output = environment.run(&["doctor", "--provider", "firecrawl"]);
    assert_deep_success(&output, "firecrawl", &[("search", "http")]);
    let request = fixture.finish();
    assert!(request.starts_with("POST /search "), "{request}");
}

#[test]
fn jina_deep_doctor_executes_the_registry_fetch_probe() {
    let fixture = Fixture::start(
        200,
        "application/json",
        &serde_json::json!({"data": {"content": "rich ".repeat(60)}}).to_string(),
    );
    let environment = RunEnvironment::new(&format!(
        "[providers.jina]\nurl = {:?}\nkeys = [\"jina-key\"]\n",
        fixture.url
    ));

    let output = environment.run(&["doctor", "--provider", "jina"]);
    assert_deep_success(&output, "jina", &[("fetch", "http")]);
    let request = fixture.finish();
    assert!(
        request.starts_with("GET /https://example.com/ "),
        "{request}"
    );
}

#[test]
fn context7_deep_doctor_executes_the_registry_library_probe() {
    let fixture = Fixture::start_sequence(vec![
        mcp_initialize("context7-session"),
        Response::new(202, "application/json", ""),
        Response::new(
            200,
            "application/json",
            r#"{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"results":[]},"content":[]}}"#,
        ),
    ]);
    let environment = RunEnvironment::new(&format!(
        "[providers.context7]\nurl = {:?}\nkeys = [\"context7-key\"]\n",
        fixture.url
    ));

    let output = environment.run(&["doctor", "--provider", "context7"]);
    assert_deep_success(&output, "context7", &[("library", "mcp")]);
    let requests = fixture.finish_all();
    assert!(requests[2].contains(r#""name":"resolve-library-id""#));
}

#[test]
fn anysearch_deep_doctor_executes_the_registry_domains_probe() {
    let fixture = Fixture::start_sequence(vec![
        mcp_initialize("anysearch-session"),
        Response::new(202, "application/json", ""),
        Response::new(
            200,
            "application/json",
            r#"{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"sub_domains":[]},"content":[]}}"#,
        ),
    ]);
    let environment = RunEnvironment::new(&format!(
        "[providers.anysearch]\nurl = {:?}\nkeys = [\"anysearch-key\"]\n",
        fixture.url
    ));

    let output = environment.run(&["doctor", "--provider", "anysearch"]);
    assert_deep_success(&output, "anysearch", &[("domains", "mcp")]);
    let requests = fixture.finish_all();
    assert!(requests[2].contains(r#""name":"get_sub_domains""#));
}

#[test]
fn deep_doctor_reports_missing_credentials_as_a_json_config_failure() {
    let environment = RunEnvironment::new("");

    let output = environment.run(&["doctor", "--provider", "exa"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");

    assert_eq!(
        (
            output.status.code(),
            &payload["ok"],
            &payload["configured"],
            &payload["error_kind"],
        ),
        (
            Some(3),
            &Value::Bool(false),
            &Value::Bool(false),
            &Value::String("config".into()),
        )
    );
}

#[test]
fn deep_doctor_applies_one_hard_deadline_and_redacts_failed_output() {
    let mut delayed = Response::new(
        401,
        "application/json",
        r#"{"message":"bad timeout-key at http://user:pass@example.test/?token=url-secret"}"#,
    );
    delayed.delay = Duration::from_secs(2);
    let fixture = Fixture::start_sequence(vec![delayed]);
    let environment = RunEnvironment::new(&format!(
        "[providers.exa]\nurl = {:?}\nkeys = [\"timeout-key\"]\n",
        fixture.url
    ));

    let output = environment.run(&["doctor", "--provider", "exa", "--timeout", "1"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(
        (output.status.code(), &payload["ok"], &payload["error_kind"]),
        (
            Some(4),
            &Value::Bool(false),
            &Value::String("timeout".into())
        )
    );
    for secret in ["timeout-key", "url-secret", "user:pass"] {
        assert!(!combined.contains(secret), "doctor leaked {secret}");
    }
    fixture.finish();
}

#[test]
fn deep_doctor_reports_authentication_failure_without_leaking_provider_values() {
    let fixture = Fixture::start(
        401,
        "application/json",
        r#"{"message":"bad auth-key at https://user:pass@example.test/?token=url-secret"}"#,
    );
    let environment = RunEnvironment::new(&format!(
        "[providers.exa]\nurl = {:?}\nkeys = [\"auth-key\"]\n",
        fixture.url
    ));

    let output = environment.run(&["doctor", "--provider", "exa"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(
        (output.status.code(), &payload["ok"], &payload["error_kind"]),
        (Some(4), &Value::Bool(false), &Value::String("auth".into()))
    );
    for secret in ["auth-key", "url-secret", "user:pass"] {
        assert!(!combined.contains(secret), "doctor leaked {secret}");
    }
    fixture.finish();
}

#[test]
fn doctor_markdown_preserves_the_json_status_and_effective_configuration() {
    let fixture = Fixture::start_sequence(reachable_responses(16));
    let environment = RunEnvironment::new(&shallow_config(&fixture.url));

    let json_output = environment.run(&["doctor"]);
    let markdown_output = environment.run(&["doctor", "--format", "markdown"]);
    let payload: Value = serde_json::from_slice(&json_output.stdout).expect("parse doctor JSON");
    let markdown = String::from_utf8(markdown_output.stdout).expect("UTF-8 markdown");

    assert_eq!(
        (json_output.status.code(), markdown_output.status.code()),
        (Some(0), Some(0))
    );
    assert!(markdown.contains("# forager doctor"));
    assert!(markdown.contains("exa: configured=true, key_count=1, source=file"));
    assert!(markdown.contains("## Effective configuration"));
    assert!(markdown.contains(r#""source": "file""#));
    assert!(!markdown.contains("exa-secret"));
    assert_eq!(payload["providers"].as_array().map(Vec::len), Some(8));
    assert_eq!(fixture.finish_all().len(), 16);
}

#[test]
fn shallow_doctor_reports_a_well_formed_but_dead_endpoint_as_unreachable() {
    let fixture = Fixture::start_sequence(reachable_responses(14));
    let config = shallow_config(&fixture.url).replace(
        &format!("[providers.xai]\nurl = {:?}", fixture.url),
        "[providers.xai]\nurl = \"http://127.0.0.1:9\"",
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["doctor", "--timeout", "2"]);
    let markdown_output = environment.run(&["doctor", "--timeout", "2", "--format", "markdown"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");
    let markdown = String::from_utf8(markdown_output.stdout).expect("UTF-8 markdown");

    assert_eq!(
        (
            output.status.code(),
            &payload["ok"],
            &payload["providers"][0]["provider"],
            &payload["providers"][0]["reachable"],
            markdown_output.status.code(),
        ),
        (
            Some(4),
            &Value::Bool(false),
            &Value::String("xai".into()),
            &Value::Bool(false),
            Some(4),
        )
    );
    assert!(markdown.contains("ok: false"), "{markdown}");
    assert_eq!(fixture.finish_all().len(), 14);
}

#[test]
fn shallow_doctor_ignores_an_unconfigured_unreachable_provider() {
    let fixture = Fixture::start_sequence(reachable_responses(7));
    let config = shallow_config(&fixture.url).replace(
        &format!(
            "[providers.xai]\nurl = {:?}\nkeys = [\"xai-secret\"]",
            fixture.url
        ),
        "[providers.xai]\nurl = \"http://127.0.0.1:9\"\nkeys = []",
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["doctor", "--timeout", "2"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");

    assert_eq!(
        (
            output.status.code(),
            &payload["ok"],
            &payload["providers"][0]["configured"],
            &payload["providers"][0]["reachable"],
        ),
        (
            Some(0),
            &Value::Bool(true),
            &Value::Bool(false),
            &Value::Bool(false),
        )
    );
    assert_eq!(fixture.finish_all().len(), 7);
}

#[test]
fn shallow_doctor_treats_zero_configured_providers_as_healthy() {
    let fixture = Fixture::start_sequence(reachable_responses(8));
    let config = [
        "xai-secret",
        "openai-secret",
        "exa-secret",
        "tavily-secret",
        "firecrawl-secret",
        "jina-secret",
        "context7-secret",
        "anysearch-secret",
    ]
    .into_iter()
    .fold(shallow_config(&fixture.url), |config, secret| {
        config.replace(&format!("keys = [\"{secret}\"]"), "keys = []")
    });
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["doctor"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");

    assert_eq!(
        (
            output.status.code(),
            &payload["ok"],
            payload["providers"]
                .as_array()
                .expect("providers")
                .iter()
                .filter(|provider| provider["configured"] == Value::Bool(true))
                .count(),
        ),
        (Some(0), &Value::Bool(true), 0)
    );
    assert_eq!(fixture.finish_all().len(), 8);
}

#[test]
fn doctor_rejects_bad_configuration_before_any_probe() {
    let environment = RunEnvironment::new("[providers.exa]\nunknown = true\n");

    let output = environment.run(&["doctor", "--provider", "exa"]);

    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unknown"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_deep_success(
    output: &std::process::Output,
    provider: &str,
    expected_checks: &[(&str, &str)],
) {
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");
    let checks = payload["checks"]
        .as_array()
        .expect("doctor checks array")
        .iter()
        .map(|check| {
            (
                check["name"].as_str().expect("check name"),
                check["transport"].as_str().expect("check transport"),
                check["ok"].as_bool().expect("check status"),
            )
        })
        .collect::<Vec<_>>();
    let expected = expected_checks
        .iter()
        .map(|(name, transport)| (*name, *transport, true))
        .collect::<Vec<_>>();
    assert_eq!(
        (
            output.status.code(),
            &payload["mode"],
            &payload["ok"],
            &payload["provider"],
            checks,
        ),
        (
            Some(0),
            &Value::String("deep".into()),
            &Value::Bool(true),
            &Value::String(provider.into()),
            expected,
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn request_json(request: &str) -> Value {
    let body = request
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .expect("request body");
    serde_json::from_str(body).expect("parse request JSON")
}

fn mcp_initialize(session: &str) -> Response {
    Response::new(
        200,
        "application/json",
        r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{},"serverInfo":{"name":"fixture","version":"1"}}}"#,
    )
    .with_header("mcp-session-id", session)
}

fn reachable_responses(count: usize) -> Vec<Response> {
    (0..count)
        .map(|_| Response::new(401, "application/json", ""))
        .collect()
}

fn shallow_config(url: &str) -> String {
    format!(
        r#"
[providers.xai]
url = {url:?}
keys = ["xai-secret"]

[providers.openai_compatible]
url = {url:?}
keys = ["openai-secret"]

[providers.exa]
url = {url:?}
keys = ["exa-secret"]

[providers.tavily]
url = {url:?}
keys = ["tavily-secret"]

[providers.firecrawl]
url = {url:?}
keys = ["firecrawl-secret"]

[providers.jina]
url = {url:?}
keys = ["jina-secret"]

[providers.context7]
url = {url:?}
keys = ["context7-secret"]

[providers.anysearch]
url = {url:?}
keys = ["anysearch-secret"]
"#
    )
}
