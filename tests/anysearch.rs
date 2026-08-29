mod support;

use std::fs;
use std::process::Output;
use std::time::Duration;

use serde_json::Value;
use sha2::{Digest, Sha256};
use support::{Fixture, Response, RunEnvironment};

const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

#[test]
fn anysearch_calls_tools_without_a_session_when_initialize_omits_the_session_header() {
    let fixture = Fixture::start_sequence(vec![
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{}}}"#,
        ),
        Response::json(
            200,
            r####"{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"### 1. Kyoto guide\n- **URL**: https://example.test/kyoto\nTravel guide"}]}}"####,
        ),
    ]);

    let output = run(
        &fixture,
        &["anysearch", "search", "Kyoto travel", "--verbose"],
        &["anysearch-key"],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let requests = fixture.finish_all();

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    assert_eq!(
        (
            payload["provider_attempts"].as_array().map(Vec::len),
            requests.len(),
            requests[0].contains(r#""method":"initialize""#),
            requests[1].contains(r#""method":"tools/call""#),
            requests[1].contains("mcp-session-id:"),
            requests
                .iter()
                .all(|request| request.contains("x-anysearch-client: mcp/1.0.0")),
            requests
                .iter()
                .any(|request| request.contains("x-context7-source:")),
        ),
        (Some(1), 2, true, true, false, true, false)
    );
}

#[test]
fn anysearch_rejects_string_content_as_a_runtime_error() {
    let fixture = Fixture::start_sequence(vec![
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{}}}"#,
        ),
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":2,"result":{"content":"non-standard text"}}"#,
        ),
    ]);

    let output = run(
        &fixture,
        &["anysearch", "search", "string content"],
        &["anysearch-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let requests = fixture.finish_all();

    assert_eq!(
        (output.status.code(), &payload["error_kind"], requests.len(),),
        (Some(4), &Value::String("runtime".into()), 2),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn anysearch_rejects_an_sse_body_that_exceeds_four_mibibytes() {
    let completed = r####"{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"### 1. Complete candidate\n- **URL**: https://example.test/complete"}]}}"####;
    let body = format!(
        "data: {completed}\n\ndata: {}\n\n",
        "x".repeat(MAX_RESPONSE_BYTES)
    );
    let fixture = Fixture::start_sequence(vec![
        initialize("oversized-session"),
        Response::json(202, ""),
        Response::new(200, "text/event-stream", &body),
    ]);

    let output = run(
        &fixture,
        &["anysearch", "search", "oversized response"],
        &["anysearch-key"],
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
fn anysearch_accepts_a_complete_mcp_body_at_four_mibibytes() {
    let result = r####"{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"### 1. Boundary candidate\n- **URL**: https://example.test/boundary"}]}}"####;
    let body = format!("{result}{}", " ".repeat(MAX_RESPONSE_BYTES - result.len()));
    let fixture = Fixture::start_sequence(vec![
        initialize("boundary-session"),
        Response::json(202, ""),
        Response::json(200, &body),
    ]);

    let output = run(
        &fixture,
        &["anysearch", "search", "boundary response"],
        &["anysearch-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (output.status.code(), &payload["results"][0]["url"]),
        (
            Some(0),
            &Value::String("https://example.test/boundary".into()),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.finish_all();
}

#[test]
fn anysearch_domains_lists_sub_domains_and_parameter_contracts() {
    let fixture = Fixture::start_sequence(vec![
        initialize("domains-session"),
        Response::json(202, ""),
        Response::json(
            200,
            r####"{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"sub_domains":[{"sub_domain":"vuln","description":"Vulnerability search","parameters":{"type":"object","required":["type","value"],"properties":{"type":{"type":"string"},"value":{"type":"string"}}}}]},"content":[{"type":"text","text":"### security.search\nThis Markdown fallback must not replace structuredContent."}]}}"####,
        ),
    ]);

    let output = run(
        &fixture,
        &["anysearch", "domains", "security"],
        &["anysearch-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let requests = fixture.finish_all();

    assert_eq!(
        (
            output.status.code(),
            &payload["provider"],
            &payload["operation"],
            &payload["domain"],
            &payload["results"][0]["sub_domain"],
            &payload["results"][0]["parameter_schema"]["required"],
            requests[2].contains(r#""name":"get_sub_domains""#),
            requests[2].contains(r#""domain":"security""#),
            requests[1].contains("x-anysearch-client: mcp/1.0.0"),
        ),
        (
            Some(0),
            &Value::String("anysearch".into()),
            &Value::String("domain_discovery".into()),
            &Value::String("security".into()),
            &Value::String("vuln".into()),
            &serde_json::json!(["type", "value"]),
            true,
            true,
            true,
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn anysearch_domains_decodes_live_markdown_contracts() {
    let fixture = Fixture::start_sequence(vec![
        initialize("domains-session"),
        Response::json(202, ""),
        Response::json(
            200,
            r###"{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"## academic Domain Capabilities (2 available)\n\n### academic.search\nCross-discipline paper search by keyword and author\n\n**Parameters:**\n- `year_from` (required): Publication year start (inclusive), four digits.\n  Accepted from 1900 onward.\n- `open_access`: Whether to return only open access publications.\n\n### academic.dataset\nResearch datasets and scientific software\n\n**Parameters:**\n- `year_to`: Publication year upper bound (inclusive)."}]}}"###,
        ),
    ]);

    let output = run(
        &fixture,
        &["anysearch", "domains", "academic"],
        &["anysearch-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    fixture.finish_all();

    assert_eq!(
        (
            output.status.code(),
            &payload["total"],
            &payload["results"][0]["domain"],
            &payload["results"][0]["sub_domain"],
            &payload["results"][0]["description"],
            &payload["results"][0]["parameter_schema"]["properties"]["year_from"]["description"],
            &payload["results"][0]["parameter_schema"]["properties"]["year_from"],
            &payload["results"][0]["parameter_schema"]["required"],
            &payload["results"][1]["sub_domain"],
            &payload["results"][1]["parameter_schema"]["required"],
        ),
        (
            Some(0),
            &Value::Number(2.into()),
            &Value::String("academic".into()),
            &Value::String("search".into()),
            &Value::String("Cross-discipline paper search by keyword and author".into()),
            &Value::String(
                "Publication year start (inclusive), four digits.\nAccepted from 1900 onward."
                    .into()
            ),
            &serde_json::json!({
                "description": "Publication year start (inclusive), four digits.\nAccepted from 1900 onward."
            }),
            &serde_json::json!(["year_from"]),
            &Value::String("dataset".into()),
            &serde_json::json!([]),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn anysearch_domains_requires_a_parent_domain_before_network() {
    let fixture = Fixture::start_sequence(Vec::new());
    let output = run(&fixture, &["anysearch", "domains"], &["anysearch-key"]);
    fixture.finish_all();

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        (
            output.status.code(),
            output.stdout.is_empty(),
            stderr.contains("required arguments"),
            stderr.contains("<DOMAIN>"),
        ),
        (Some(2), true, true, true),
        "stderr: {stderr}"
    );
}

#[test]
fn anysearch_search_without_a_domain_performs_vertical_discovery() {
    let fixture = Fixture::start_sequence(vec![
        initialize("discovery-session"),
        Response::json(202, ""),
        Response::json(
            200,
            r####"{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"### 1. Kyoto guide\n- **URL**: https://example.test/kyoto\nTravel guide"}]}}"####,
        ),
    ]);

    let output = run(
        &fixture,
        &["anysearch", "search", "Kyoto travel", "--max-results", "2"],
        &["anysearch-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let requests = fixture.finish_all();

    assert_eq!(
        (
            &payload["operation"],
            &payload["results"][0]["url"],
            payload.get("domain"),
            requests[2].contains(r#""name":"search""#),
            requests[2].contains(r#""query":"Kyoto travel""#)
                && requests[2].contains(r#""max_results":2"#),
            requests[2].contains(r#""domain""#),
        ),
        (
            &Value::String("vertical_discovery".into()),
            &Value::String("https://example.test/kyoto".into()),
            None,
            true,
            true,
            false,
        )
    );
}

#[test]
fn anysearch_search_decodes_flexible_numbered_markdown_blocks() {
    let (output, payload) = run_search_text(
        "### 1. First result\n- **URL**: https://example.test/first\nFirst summary\n\n### \t2.\tSecond result\n-  **URL**:\t https://example.test/second\nSecond summary",
        "flexible markdown",
    );

    assert_eq!(
        (output.status.code(), &payload["results"]),
        (
            Some(0),
            &serde_json::json!([
                {
                    "title": "First result",
                    "url": "https://example.test/first",
                    "description": "First summary"
                },
                {
                    "title": "Second result",
                    "url": "https://example.test/second",
                    "description": "Second summary"
                }
            ]),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn anysearch_search_excludes_markdown_metadata_from_descriptions() {
    let (output, payload) = run_search_text(
        "### 1. Result\n## Metadata\n- **URL**: https://example.test/result\nResult summary",
        "metadata",
    );

    assert_eq!(
        (output.status.code(), &payload["results"][0]),
        (
            Some(0),
            &serde_json::json!({
                "title": "Result",
                "url": "https://example.test/result",
                "description": "Result summary"
            })
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn anysearch_search_limits_descriptions_to_300_unicode_characters() {
    let text = format!("### 1. Result\n{}", "界".repeat(301));
    let (output, payload) = run_search_text(&text, "long description");

    assert_eq!(
        (
            output.status.code(),
            payload["results"][0]["description"]
                .as_str()
                .map(str::chars)
                .map(Iterator::count),
        ),
        (Some(0), Some(300)),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn anysearch_search_validates_labeled_urls() {
    let fixture = Fixture::start_sequence(vec![
        initialize("labeled-url-session"),
        Response::json(202, ""),
        Response::json(
            200,
            r####"{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"### 1. Invalid scheme\n- **URL**: ftp://example.test/file\nFirst summary\n### 2. Missing host\n- **URL**: https://\nSecond summary\n### 3. Valid URL\n- **URL**: https://example.test/valid\nThird summary"}]}}"####,
        ),
    ]);

    let output = run(
        &fixture,
        &["anysearch", "search", "labeled URLs"],
        &["anysearch-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    fixture.finish_all();

    assert_eq!(
        (output.status.code(), &payload["results"]),
        (
            Some(0),
            &serde_json::json!([
                {
                    "title": "Invalid scheme",
                    "url": "",
                    "description": "First summary"
                },
                {
                    "title": "Missing host",
                    "url": "",
                    "description": "Second summary"
                },
                {
                    "title": "Valid URL",
                    "url": "https://example.test/valid",
                    "description": "Third summary"
                }
            ])
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn anysearch_search_extracts_unique_bare_urls_without_numbered_headings() {
    let fixture = Fixture::start_sequence(vec![
        initialize("bare-url-session"),
        Response::json(202, ""),
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"Raw https://example.test/first then [linked](https://example.test/linked) and ftp://example.test/file.\nRepeat https://example.test/first before http://example.test/second\nReject https:// and http://["}]}}"#,
        ),
    ]);

    let output = run(
        &fixture,
        &["anysearch", "search", "bare URLs"],
        &["anysearch-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    fixture.finish_all();

    assert_eq!(
        (output.status.code(), &payload["results"]),
        (
            Some(0),
            &serde_json::json!([
                {
                    "title": "https://example.test/first",
                    "url": "https://example.test/first",
                    "description": ""
                },
                {
                    "title": "http://example.test/second",
                    "url": "http://example.test/second",
                    "description": ""
                }
            ])
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn anysearch_search_ignores_formatted_and_nested_urls() {
    let fixture = Fixture::start_sequence(vec![
        initialize("formatted-url-session"),
        Response::json(202, ""),
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"[inline](https://example.test/inline)\n[reference]: https://example.test/reference\n<https://example.test/autolink>\n<details>\nhttps://example.test/details\n<details>\nhttps://example.test/nested-details\n</details>\nhttps://example.test/outer-details\n</details>\nRaw https://example.test/source?next=https://example.test/nested"}]}}"#,
        ),
    ]);

    let output = run(
        &fixture,
        &["anysearch", "search", "formatted URLs"],
        &["anysearch-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    fixture.finish_all();

    assert_eq!(
        (output.status.code(), &payload["results"]),
        (
            Some(0),
            &serde_json::json!([{
                "title": "https://example.test/source?next=https://example.test/nested",
                "url": "https://example.test/source?next=https://example.test/nested",
                "description": ""
            }])
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn anysearch_search_preserves_pure_structured_content_without_a_url() {
    let fixture = Fixture::start_sequence(vec![
        initialize("structured-session"),
        Response::json(202, ""),
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"identifier":"CVE-2024-3094","severity":"critical"},"content":[]}}"#,
        ),
    ]);

    let output = run(
        &fixture,
        &["anysearch", "search", "structured result"],
        &["anysearch-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    fixture.finish_all();

    assert_eq!(
        (output.status.code(), &payload["results"]),
        (
            Some(0),
            &serde_json::json!([{
                "title": "vertical search structured result",
                "url": "",
                "description": "",
                "evidence_type": "structured"
            }])
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn anysearch_explicit_search_passes_domain_and_sub_domain_parameters() {
    let fixture = Fixture::start_sequence(vec![
        initialize("search-session"),
        Response::json(202, ""),
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"identifier":"CVE-2024-3094","severity":"critical"},"content":[{"type":"text","text":"CVE-2024-3094 severity: critical"}]}}"#,
        ),
    ]);

    let output = run(
        &fixture,
        &[
            "anysearch",
            "search",
            "xz vulnerability",
            "--domain",
            "security",
            "--sub-domain",
            "vuln",
            "--sub-domain-params",
            r#"{"type":"cve","value":"CVE-2024-3094"}"#,
        ],
        &["anysearch-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let requests = fixture.finish_all();

    assert_eq!(
        (
            &payload["operation"],
            &payload["domain"],
            &payload["sub_domain"],
            &payload["domain_status"],
            &payload["schema_validation"]["status"],
            &payload["results"][0]["evidence_type"],
            &payload["results"][0]["url"],
            requests[2].contains(r#""domain":"security""#),
            requests[2].contains(r#""sub_domain":"vuln""#),
            requests[2].contains(r#""sub_domain_params":{"type":"cve","value":"CVE-2024-3094"}"#,),
        ),
        (
            &Value::String("vertical_search".into()),
            &Value::String("security".into()),
            &Value::String("vuln".into()),
            &Value::String("discovered_unverified".into()),
            &Value::String("unavailable".into()),
            &Value::String("structured".into()),
            &Value::String(String::new()),
            true,
            true,
            true,
        )
    );
}

#[test]
fn anysearch_search_rejects_invalid_parameter_contracts_before_network() {
    for (arguments, message) in [
        (
            vec!["anysearch", "search", "query", "--domain", "security"],
            "--sub-domain",
        ),
        (
            vec![
                "anysearch",
                "search",
                "query",
                "--domain",
                "security.vuln",
                "--sub-domain",
                "vuln",
            ],
            "dotted domain shorthand",
        ),
        (
            vec![
                "anysearch",
                "search",
                "query",
                "--domain",
                "security",
                "--sub-domain",
                "cve",
            ],
            "--sub-domain vuln",
        ),
        (
            vec!["anysearch", "search", "query", "--sub-domain-params", "[]"],
            "JSON object",
        ),
        (
            vec![
                "anysearch",
                "search",
                "query",
                "--domain",
                "security",
                "--sub-domain",
                "vuln",
                "--sub-domain-params",
                r#"{"query":"must-not-leak"}"#,
            ],
            "reserved fields",
        ),
    ] {
        let fixture = Fixture::start_sequence(Vec::new());
        let output = run(&fixture, &arguments, &["anysearch-key"]);
        fixture.finish_all();
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert_eq!(
            (
                output.status.code(),
                output.stdout.is_empty(),
                stderr.contains(message),
                stderr.contains("must-not-leak"),
            ),
            (Some(2), true, true, false),
            "stderr: {stderr}"
        );
    }
}

#[test]
fn anysearch_tool_errors_redact_request_values() {
    let fixture = Fixture::start_sequence(vec![
        initialize("redaction-session"),
        Response::json(202, ""),
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":2,"result":{"isError":true,"content":[{"type":"text","text":"secret-query secret-param anysearch-key"}]}}"#,
        ),
    ]);

    let output = run(
        &fixture,
        &[
            "anysearch",
            "search",
            "secret-query",
            "--domain",
            "security",
            "--sub-domain",
            "vuln",
            "--sub-domain-params",
            r#"{"token":"secret-param"}"#,
        ],
        &["anysearch-key"],
    );
    fixture.finish_all();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(output.status.code(), Some(4));
    assert!(!combined.contains("secret-query"));
    assert!(!combined.contains("secret-param"));
    assert!(!combined.contains("anysearch-key"));
}

#[test]
fn anysearch_classifies_upstream_parameter_errors_without_retrying() {
    let fixture = Fixture::start_sequence(vec![
        initialize("parameter-session"),
        Response::json(202, ""),
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32602,"message":"type and value are required"}}"#,
        ),
    ]);

    let output = run(
        &fixture,
        &[
            "anysearch",
            "search",
            "missing params",
            "--domain",
            "security",
            "--sub-domain",
            "vuln",
        ],
        &["anysearch-key"],
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
        (Some(4), &Value::String("parameter".into()), Some(1), 3)
    );
}

#[test]
fn anysearch_rotates_after_result_is_error_reports_exhausted_quota() {
    let fixture = Fixture::start_sequence(vec![
        initialize("quota-session"),
        Response::json(202, ""),
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":2,"result":{"isError":true,"content":[{"type":"text","text":"credits exhausted"}]}}"#,
        ),
        initialize("rotated-session"),
        Response::json(202, ""),
        Response::json(200, r#"{"jsonrpc":"2.0","id":2,"result":{"content":[]}}"#),
    ]);

    let output = run(
        &fixture,
        &["anysearch", "search", "rotation", "--verbose"],
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
        ),
        (
            Some(0),
            &Value::String("quota_exhausted".into()),
            true,
            true,
        )
    );
}

#[test]
fn anysearch_authentication_failure_has_a_stable_transport_exit() {
    let fixture = Fixture::start_sequence(vec![Response::json(
        401,
        r#"{"error":{"message":"invalid credential"}}"#,
    )]);

    let output = run(
        &fixture,
        &["anysearch", "domains", "security"],
        &["bad-key"],
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
        (Some(4), &Value::String("auth".into()), Some(1), 1)
    );
}

#[test]
fn anysearch_obeys_the_command_deadline() {
    let fixture = Fixture::start_sequence(vec![
        initialize("late-session").with_delay(Duration::from_millis(1500)),
    ]);

    let output = run(
        &fixture,
        &["anysearch", "search", "timeout", "--timeout", "1"],
        &["anysearch-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let requests = fixture.finish_all();

    assert_eq!(
        (output.status.code(), &payload["error_kind"], requests.len()),
        (Some(4), &Value::String("timeout".into()), 1)
    );
}

#[test]
fn anysearch_candidate_fixtures_match_the_versioned_unverified_manifest() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest_path = root.join("assets/anysearch/verified-domain-manifest.json");
    let manifest: Value = serde_json::from_slice(&fs::read(manifest_path).expect("read manifest"))
        .expect("parse manifest");
    let assessments = manifest["candidate_assessments"]
        .as_array()
        .expect("candidate assessments");

    assert_eq!(manifest["verified_domains"], serde_json::json!([]));
    assert_eq!(assessments.len(), 4);
    for assessment in assessments {
        assert_eq!(assessment["status"], "discovered_unverified");
        let fixture_path = root.join(
            assessment["evidence"]["fixture"]
                .as_str()
                .expect("fixture path"),
        );
        let fixture: Value =
            serde_json::from_slice(&fs::read(fixture_path).expect("read candidate fixture"))
                .expect("parse candidate fixture");
        let canonical =
            serde_json::to_string(&fixture["parameter_schema"]).expect("canonical schema");
        let fingerprint = format!("sha256:{:x}", Sha256::digest(canonical.as_bytes()));

        assert_eq!(assessment["schema_fingerprint"], fingerprint);
        assert_eq!(fixture["provenance"], "synthetic_mock");
        assert_eq!(assessment["acceptance_date"], Value::Null);
    }
}

#[test]
fn anysearch_vertical_discovery_cannot_modify_or_promote_the_manifest() {
    let manifest_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets/anysearch/verified-domain-manifest.json");
    let before = fs::read(&manifest_path).expect("read manifest before discovery");
    let fixture = Fixture::start_sequence(vec![
        initialize("isolation-session"),
        Response::json(202, ""),
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"domain":"security","sub_domain":"vuln"},"content":[{"type":"text","text":"candidate"}]}}"#,
        ),
    ]);

    let output = run(
        &fixture,
        &["anysearch", "search", "find a vulnerability"],
        &["anysearch-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    fixture.finish_all();
    let after = fs::read(manifest_path).expect("read manifest after discovery");

    assert_eq!(
        (
            output.status.code(),
            &payload["operation"],
            payload.get("domain"),
            payload.get("domain_status"),
            before == after,
        ),
        (
            Some(0),
            &Value::String("vertical_discovery".into()),
            None,
            None,
            true,
        )
    );
}

fn initialize(session: &'static str) -> Response {
    Response::json(
        200,
        r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{}}}"#,
    )
    .with_session(session)
}

fn run_search_text(text: &str, query: &str) -> (Output, Value) {
    let tool_result = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "result": {"content": [{"type": "text", "text": text}]}
    });
    let fixture = Fixture::start_sequence(vec![
        initialize("search-result-session"),
        Response::json(202, ""),
        Response::json(200, &tool_result.to_string()),
    ]);
    let output = run(
        &fixture,
        &["anysearch", "search", query],
        &["anysearch-key"],
    );
    let payload = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    fixture.finish_all();
    (output, payload)
}

fn run(fixture: &Fixture, arguments: &[&str], keys: &[&str]) -> Output {
    RunEnvironment::new(&format!(
        "[providers.anysearch]\nurl = {:?}\nkeys = {keys:?}\ntimeout = 2\n[journal]\nenabled = false\n",
        fixture.url
    ))
    .run(arguments)
}
