mod support;

use std::collections::BTreeSet;
use std::process::Output;
use std::time::Duration;

use serde_json::Value;
use support::{Fixture, Response, RunEnvironment};

#[test]
fn context7_calls_tools_without_a_session_or_legacy_source_header() {
    let fixture = Fixture::start_sequence(vec![
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{}}}"#,
        ),
        library_result("sessionless"),
    ]);

    let output = run(&fixture, &["context7", "library", "rust"], &["only-key"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let requests = fixture.finish_all();

    assert_eq!(
        (
            output.status.code(),
            &payload["results"][0]["id"],
            requests.len(),
            requests[0].contains(r#""method":"initialize""#),
            requests[1].contains(r#""method":"tools/call""#),
            requests
                .iter()
                .any(|request| request.contains("mcp-session-id:")),
            requests
                .iter()
                .any(|request| request.contains("x-context7-source:")),
        ),
        (
            Some(0),
            &Value::String("/fixture/sessionless".into()),
            2,
            true,
            true,
            false,
            false,
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn context7_library_initializes_the_mcp_session_and_normalizes_json_results() {
    let fixture = Fixture::start_sequence(vec![
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{},"serverInfo":{"name":"context7","version":"1"}}}"#,
        )
        .with_session("session-1"),
        Response::json(202, ""),
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"results":[{"id":"/rust-lang/rust","title":"Rust","description":"The Rust language","trustScore":9.9,"benchmarkScore":88,"totalSnippets":1234,"stars":100000}]},"content":[]}}"#,
        ),
    ]);

    let output = run(
        &fixture,
        &["context7", "library", "rust", "async drop", "--verbose"],
        &["context-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let requests = fixture.finish_all();
    assert_eq!(
        payload["provider_attempts"].as_array().map(Vec::len),
        Some(1),
        "three MCP handshakes belong to one logical attempt"
    );

    assert_eq!(
        (
            output.status.code(),
            &payload["provider"],
            &payload["query"],
            &payload["results"][0]["id"],
            &payload["results"][0]["title"],
            &payload["results"][0]["trust_score"],
            requests.len(),
            requests[0].contains(r#""method":"initialize""#),
            requests[1].contains(r#""method":"notifications/initialized""#),
            requests[2].contains(r#""name":"resolve-library-id""#),
            requests[2].contains("mcp-session-id: session-1"),
            requests
                .iter()
                .all(|request| request.contains("authorization: Bearer context-key")),
        ),
        (
            Some(0),
            &Value::String("context7".into()),
            &Value::String("rust async drop".into()),
            &Value::String("/rust-lang/rust".into()),
            &Value::String("Rust".into()),
            &Value::from(9.9),
            3,
            true,
            true,
            true,
            true,
            true,
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn context7_docs_default_json_contains_only_the_readable_payload() {
    let fixture = Fixture::start_sequence(vec![
        Response::sse(
            200,
            "data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{}}}\n\n",
        )
        .with_session("session-sse"),
        Response::json(204, ""),
        Response::sse(
            200,
            "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"structuredContent\":{\"content\":\"Async drop documentation\",\"codeSnippets\":[{\"code\":\"async fn drop() {}\"}],\"infoSnippets\":[{\"text\":\"Drop is synchronous\"}]},\"content\":[{\"type\":\"text\",\"text\":\"fallback text\"}]}}\n\n",
        ),
    ]);

    let output = run(
        &fixture,
        &["context7", "docs", "/rust-lang/rust", "async drop"],
        &["context-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let requests = fixture.finish_all();

    assert_eq!(
        (
            output.status.code(),
            payload
                .as_object()
                .expect("Context7 docs object")
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>(),
            &payload["content"],
            requests[2].contains(r#""name":"query-docs""#),
        ),
        (
            Some(0),
            BTreeSet::from(["content", "library_id", "provider", "query"]),
            &Value::String("Async drop documentation".into()),
            true,
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn context7_docs_treats_redirect_language_as_document_content() {
    let fixture = Fixture::start_sequence(vec![
        initialize("redirect-language-docs"),
        Response::json(202, ""),
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"Redirect requests from /old to /new."}]}}"#,
        ),
    ]);

    let output = run(
        &fixture,
        &["context7", "docs", "/example/router", "redirects"],
        &["only-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (output.status.code(), &payload["content"]),
        (
            Some(0),
            &Value::String("Redirect requests from /old to /new.".into()),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.finish_all();
}

#[test]
fn context7_docs_turns_structured_content_only_into_readable_content() {
    let fixture = Fixture::start_sequence(vec![
        initialize("structured-docs"),
        Response::json(202, ""),
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"results":[{"content":"Async runtime documentation","source":"Tokio"}]},"content":[]}}"#,
        ),
    ]);

    let output = run(
        &fixture,
        &["context7", "docs", "/tokio-rs/tokio", "runtime"],
        &["only-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let content: Value = serde_json::from_str(payload["content"].as_str().expect("content string"))
        .expect("structured content JSON");

    assert_eq!(
        (output.status.code(), content),
        (
            Some(0),
            serde_json::json!({
                "results": [{
                    "content": "Async runtime documentation",
                    "source": "Tokio"
                }]
            }),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.finish_all();
}

#[test]
fn context7_docs_verbose_json_adds_only_provider_attempts() {
    let fixture = Fixture::start_sequence(vec![
        initialize("verbose-docs"),
        Response::json(202, ""),
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"Readable MCP text"}]}}"#,
        ),
    ]);

    let output = run(
        &fixture,
        &[
            "context7",
            "docs",
            "/rust-lang/rust",
            "async drop",
            "--verbose",
        ],
        &["context-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            payload
                .as_object()
                .expect("Context7 docs object")
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>(),
            payload["provider_attempts"].as_array().map(Vec::len),
        ),
        (
            Some(0),
            BTreeSet::from([
                "content",
                "library_id",
                "provider",
                "provider_attempts",
                "query",
            ]),
            Some(1),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.finish_all();
}

#[test]
fn context7_rejects_an_mcp_body_that_exceeds_four_mibibytes() {
    let text = format!(
        "- Title: Rust\n- Context7-compatible library ID: /rust-lang/rust\n- Description: {}",
        "x".repeat(4 * 1024 * 1024)
    );
    let result = format!(
        r#"{{"jsonrpc":"2.0","id":2,"result":{{"content":[{{"type":"text","text":{}"#,
        serde_json::to_string(&text).expect("encode MCP text")
    );
    let fixture = Fixture::start_sequence(vec![
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{}}}"#,
        )
        .with_session("truncated-session"),
        Response::json(202, ""),
        Response::json(200, &result),
    ]);

    let output = run(
        &fixture,
        &["context7", "library", "rust", "async drop"],
        &["context-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            &payload["message"],
            payload.get("results"),
        ),
        (
            Some(4),
            &Value::String("runtime".into()),
            &Value::String("response exceeded 4 MiB".into()),
            None,
        )
    );
    fixture.finish_all();
}

#[test]
fn context7_authentication_failure_has_a_stable_transport_exit() {
    let fixture = Fixture::start_sequence(vec![Response::json(
        401,
        r#"{"error":{"message":"invalid credential"}}"#,
    )]);

    let output = run(&fixture, &["context7", "library", "rust"], &["bad-key"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let requests = fixture.finish_all();

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            payload["attempts"]["total"].as_u64(),
            requests.len(),
        ),
        (Some(4), &Value::String("auth".into()), Some(1), 1),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn context7_retries_a_503_within_the_same_deadline() {
    let fixture = Fixture::start_sequence(vec![
        Response::json(503, r#"{"error":{"message":"unavailable"}}"#),
        initialize("retry-session"),
        Response::json(202, ""),
        library_result("2"),
    ]);

    let output = run(
        &fixture,
        &["context7", "library", "rust", "--verbose"],
        &["only-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let requests = fixture.finish_all();

    assert_eq!(
        (
            output.status.code(),
            &payload["results"][0]["id"],
            &payload["provider_attempts"][0]["error_kind"],
            requests.len(),
        ),
        (
            Some(0),
            &Value::String("/fixture/2".into()),
            &Value::String("network".into()),
            4,
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn context7_classifies_json_rpc_rate_limit_before_rotating_credentials() {
    let fixture = Fixture::start_sequence(vec![
        initialize("first-session"),
        Response::json(202, ""),
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32000,"message":"rate limit exceeded"}}"#,
        ),
        initialize("second-session"),
        Response::json(202, ""),
        library_result("rotated"),
    ]);

    let output = run(
        &fixture,
        &["context7", "library", "rust", "--verbose"],
        &["first-key", "second-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let requests = fixture.finish_all();

    assert_eq!(
        (
            output.status.code(),
            &payload["provider_attempts"][0]["error_kind"],
            requests[0].contains("authorization: Bearer first-key"),
            requests[3].contains("authorization: Bearer second-key"),
            &payload["results"][0]["id"],
        ),
        (
            Some(0),
            &Value::String("rate_limited".into()),
            true,
            true,
            &Value::String("/fixture/rotated".into()),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn context7_renews_an_expired_session_before_returning() {
    let fixture = Fixture::start_sequence(vec![
        initialize("expired-session"),
        Response::json(202, ""),
        Response::json(404, "MCP session expired"),
        initialize("renewed-session"),
        Response::json(202, ""),
        library_result("renewed"),
    ]);

    let output = run(&fixture, &["context7", "library", "rust"], &["only-key"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let requests = fixture.finish_all();

    assert_eq!(
        (
            output.status.code(),
            &payload["results"][0]["id"],
            requests.len(),
            requests[5].contains("mcp-session-id: renewed-session"),
        ),
        (Some(0), &Value::String("/fixture/renewed".into()), 6, true,),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn context7_reports_a_library_redirect_without_retrying() {
    let fixture = Fixture::start_sequence(vec![
        initialize("redirect-session"),
        Response::json(202, ""),
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"redirectedLibraryId":"/rust-lang/book"},"content":[]}}"#,
        ),
    ]);

    let output = run(
        &fixture,
        &["context7", "docs", "/rust-lang/rust", "book"],
        &["only-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let requests = fixture.finish_all();

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            &payload["redirected_library_id"],
            requests.len(),
        ),
        (
            Some(4),
            &Value::String("runtime".into()),
            &Value::String("/rust-lang/book".into()),
            3,
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn context7_redacts_urls_in_a_library_redirect() {
    let fixture = Fixture::start_sequence(vec![
        initialize("redacted-redirect-session"),
        Response::json(202, ""),
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"redirectedLibraryId":"/https://user:password@example.test/private?token=redirect-secret"},"content":[]}}"#,
        ),
    ]);

    let output = run(
        &fixture,
        &["context7", "docs", "/rust-lang/rust", "book"],
        &["only-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["message"],
            &payload["redirected_library_id"],
        ),
        (
            Some(4),
            &Value::String(
                "Context7 library ID was redirected to /https://example.test/private?token=********"
                    .into(),
            ),
            &Value::String("/https://example.test/private?token=********".into()),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.finish_all();
}

#[test]
fn context7_unknown_tool_error_is_runtime_and_is_not_retried() {
    let fixture = Fixture::start_sequence(vec![
        initialize("error-session"),
        Response::json(202, ""),
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":2,"result":{"isError":true,"content":[{"type":"text","text":"Unknown tool error"}]}}"#,
        ),
    ]);

    let output = run(&fixture, &["context7", "library", "rust"], &["only-key"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let requests = fixture.finish_all();

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            payload["attempts"]["total"].as_u64(),
            requests.len(),
        ),
        (Some(4), &Value::String("runtime".into()), Some(1), 3),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn context7_library_treats_an_empty_result_set_as_success() {
    let fixture = Fixture::start_sequence(vec![
        initialize("empty-session"),
        Response::json(202, ""),
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"results":[]},"content":[]}}"#,
        ),
    ]);

    let output = run(&fixture, &["context7", "library", "missing"], &["only-key"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (output.status.code(), &payload["results"], &payload["total"]),
        (Some(0), &serde_json::json!([]), &Value::from(0)),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.finish_all();
}

#[test]
fn context7_session_renewal_remains_inside_the_command_deadline() {
    let fixture = Fixture::start_sequence(vec![
        initialize("expired-session"),
        Response::json(202, ""),
        Response::json(404, "MCP session expired"),
        initialize("late-session").with_delay(Duration::from_millis(1500)),
    ]);

    let output = run(
        &fixture,
        &["context7", "library", "rust", "--timeout", "1"],
        &["only-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let requests = fixture.finish_all();

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            payload["attempts"]["total"].as_u64(),
            requests.len(),
        ),
        (Some(4), &Value::String("timeout".into()), Some(1), 4),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn context7_library_normalizes_plain_text_candidates() {
    let fixture = Fixture::start_sequence(vec![
        initialize("text-session"),
        Response::json(202, ""),
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"- Title: Tokio\n- Context7-compatible library ID: /tokio-rs/tokio\n- Description: Async runtime\n- Code Snippets: 1,234\n- Trust Score: 9.7\n- Benchmark Score: 91"}]}}"#,
        ),
    ]);

    let output = run(&fixture, &["context7", "library", "tokio"], &["only-key"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["results"][0]["id"],
            &payload["results"][0]["total_snippets"],
            &payload["results"][0]["trust_score"],
        ),
        (
            Some(0),
            &Value::String("/tokio-rs/tokio".into()),
            &Value::from(1234),
            &Value::from(9.7),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.finish_all();
}

#[test]
fn context7_docs_content_format_prints_only_the_body() {
    let fixture = Fixture::start_sequence(vec![
        initialize("content-session"),
        Response::json(202, ""),
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"content":"Exact documentation body"},"content":[]}}"#,
        ),
    ]);

    let output = run(
        &fixture,
        &[
            "context7",
            "docs",
            "/tokio-rs/tokio",
            "runtime",
            "--format",
            "content",
        ],
        &["only-key"],
    );

    assert_eq!(
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).trim(),
        ),
        (Some(0), "Exact documentation body"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.finish_all();
}

#[test]
fn context7_renews_a_session_that_expires_during_initialized_notification() {
    let fixture = Fixture::start_sequence(vec![
        initialize("notification-expired"),
        Response::json(404, "MCP session expired"),
        initialize("notification-renewed"),
        Response::json(202, ""),
        library_result("renewed"),
    ]);

    let output = run(&fixture, &["context7", "library", "rust"], &["only-key"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let requests = fixture.finish_all();

    assert_eq!(
        (
            output.status.code(),
            &payload["results"][0]["id"],
            requests.len(),
            requests[4].contains("mcp-session-id: notification-renewed"),
        ),
        (Some(0), &Value::String("/fixture/renewed".into()), 5, true,),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn context7_classifies_initialize_json_rpc_errors_before_requiring_a_session_header() {
    let fixture = Fixture::start_sequence(vec![
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"rate limit exceeded"}}"#,
        ),
        initialize("rotated-initialize"),
        Response::json(202, ""),
        library_result("rotated"),
    ]);

    let output = run(
        &fixture,
        &["context7", "library", "rust", "--verbose"],
        &["first-key", "second-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let requests = fixture.finish_all();

    assert_eq!(
        (
            output.status.code(),
            &payload["provider_attempts"][0]["error_kind"],
            requests[0].contains("authorization: Bearer first-key"),
            requests[1].contains("authorization: Bearer second-key"),
        ),
        (Some(0), &Value::String("rate_limited".into()), true, true,),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn context7_response_body_reading_obeys_the_command_deadline() {
    let fixture = Fixture::start_sequence(vec![
        initialize("slow-body").with_body_delay(Duration::from_millis(1500)),
    ]);

    let output = run(
        &fixture,
        &["context7", "library", "rust", "--timeout", "1"],
        &["only-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let requests = fixture.finish_all();

    assert_eq!(
        (output.status.code(), &payload["error_kind"], requests.len(),),
        (Some(4), &Value::String("timeout".into()), 1),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn initialize(session: &'static str) -> Response {
    Response::json(
        200,
        r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{}}}"#,
    )
    .with_session(session)
}

fn library_result(suffix: &'static str) -> Response {
    let body = match suffix {
        "2" => {
            r#"{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"results":[{"id":"/fixture/2","title":"Fixture"}]},"content":[]}}"#
        }
        "rotated" => {
            r#"{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"results":[{"id":"/fixture/rotated","title":"Fixture"}]},"content":[]}}"#
        }
        "renewed" => {
            r#"{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"results":[{"id":"/fixture/renewed","title":"Fixture"}]},"content":[]}}"#
        }
        "sessionless" => {
            r#"{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"results":[{"id":"/fixture/sessionless","title":"Fixture"}]},"content":[]}}"#
        }
        _ => unreachable!("test fixture suffix is known"),
    };
    Response::json(200, body)
}

fn run(fixture: &Fixture, arguments: &[&str], keys: &[&str]) -> Output {
    RunEnvironment::new(&format!(
        "[providers.context7]\nurl = {:?}\nkeys = {keys:?}\ntimeout = 2\n[journal]\nenabled = false\n",
        fixture.url
    ))
    .run(arguments)
}
