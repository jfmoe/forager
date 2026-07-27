use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Output};
use std::thread;
use std::time::Duration;

use serde_json::Value;
use sha2::{Digest, Sha256};

#[test]
fn anysearch_calls_tools_without_a_session_when_initialize_omits_the_session_header() {
    let fixture = Fixture::start(vec![
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
        &["anysearch", "search", "Kyoto travel"],
        &["anysearch-key"],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let requests = fixture.finish();

    assert_eq!(
        (
            requests.len(),
            requests[0].contains(r#""method":"initialize""#),
            requests[1].contains(r#""method":"tools/call""#),
            requests[1].contains("mcp-session-id:"),
        ),
        (2, true, true, false)
    );
}

#[test]
fn anysearch_domains_lists_sub_domains_and_parameter_contracts() {
    let fixture = Fixture::start(vec![
        initialize("domains-session"),
        Response::json(202, ""),
        Response::json(
            200,
            r#"{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"sub_domains":[{"sub_domain":"vuln","description":"Vulnerability search","parameters":{"type":"object","required":["type","value"],"properties":{"type":{"type":"string"},"value":{"type":"string"}}}}]},"content":[]}}"#,
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
    let requests = fixture.finish();

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
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn anysearch_domains_requires_a_parent_domain_before_network() {
    let fixture = Fixture::start(Vec::new());
    let output = run(&fixture, &["anysearch", "domains"], &["anysearch-key"]);
    fixture.finish();

    assert_eq!(
        (
            output.status.code(),
            output.stdout.is_empty(),
            String::from_utf8_lossy(&output.stderr).contains("parent DOMAIN"),
        ),
        (Some(2), true, true)
    );
}

#[test]
fn anysearch_search_without_a_domain_performs_vertical_discovery() {
    let fixture = Fixture::start(vec![
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
    let requests = fixture.finish();

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
fn anysearch_explicit_search_passes_domain_and_sub_domain_parameters() {
    let fixture = Fixture::start(vec![
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
    let requests = fixture.finish();

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
        let fixture = Fixture::start(Vec::new());
        let output = run(&fixture, &arguments, &["anysearch-key"]);
        fixture.finish();
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
    let fixture = Fixture::start(vec![
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
    fixture.finish();
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
    let fixture = Fixture::start(vec![
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
    let requests = fixture.finish();

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
    let fixture = Fixture::start(vec![
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
    let requests = fixture.finish();

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
    let fixture = Fixture::start(vec![Response::json(
        401,
        r#"{"error":{"message":"invalid credential"}}"#,
    )]);

    let output = run(
        &fixture,
        &["anysearch", "domains", "security"],
        &["bad-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let requests = fixture.finish();

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
    let fixture = Fixture::start(vec![
        initialize("late-session").with_delay(Duration::from_millis(1500)),
    ]);

    let output = run(
        &fixture,
        &["anysearch", "search", "timeout", "--timeout", "1"],
        &["anysearch-key"],
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    let requests = fixture.finish();

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
    let fixture = Fixture::start(vec![
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
    fixture.finish();
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

fn run(fixture: &Fixture, arguments: &[&str], keys: &[&str]) -> Output {
    let root = tempfile::tempdir().expect("create isolated root");
    let config_dir = root.path().join("config");
    let state_dir = root.path().join("state");
    fs::create_dir_all(&config_dir).expect("create config directory");
    let keys = keys
        .iter()
        .map(|key| format!("{key:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    fs::write(
        config_dir.join("config.toml"),
        format!(
            "[providers.anysearch]\nurl = {:?}\nkeys = [{keys}]\ntimeout = 2\n[journal]\nenabled = false\n",
            fixture.url
        ),
    )
    .expect("write config");

    Command::new(env!("CARGO_BIN_EXE_forager"))
        .args(arguments)
        .env_clear()
        .env("FORAGER_CONFIG_DIR", config_dir)
        .env("XDG_STATE_HOME", state_dir)
        .env("HOME", root.path())
        .output()
        .expect("run forager")
}

struct Fixture {
    url: String,
    handle: thread::JoinHandle<Vec<String>>,
}

impl Fixture {
    fn start(responses: Vec<Response>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture");
        let address = listener.local_addr().expect("fixture address");
        let handle = thread::spawn(move || {
            let mut requests = Vec::new();
            for response in responses {
                let (mut stream, _) = listener.accept().expect("accept request");
                let request = read_request(&mut stream);
                let reason = if response.status < 400 { "OK" } else { "Error" };
                let session = response
                    .session
                    .map(|value| format!("Mcp-Session-Id: {value}\r\n"))
                    .unwrap_or_default();
                thread::sleep(response.delay);
                let headers = format!(
                    "HTTP/1.1 {} {reason}\r\nContent-Type: application/json\r\n{session}Content-Length: {}\r\nConnection: close\r\n\r\n",
                    response.status,
                    response.body.len()
                );
                let _ = stream.write_all(headers.as_bytes());
                let _ = stream.write_all(response.body.as_bytes());
                requests.push(String::from_utf8(request).expect("UTF-8 request"));
            }
            requests
        });
        Self {
            url: format!("http://{address}"),
            handle,
        }
    }

    fn finish(self) -> Vec<String> {
        self.handle.join().expect("fixture thread")
    }
}

struct Response {
    status: u16,
    body: &'static str,
    session: Option<&'static str>,
    delay: Duration,
}

impl Response {
    fn json(status: u16, body: &'static str) -> Self {
        Self {
            status,
            body,
            session: None,
            delay: Duration::ZERO,
        }
    }

    fn with_session(mut self, session: &'static str) -> Self {
        self.session = Some(session);
        self
    }

    fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }
}

fn read_request(stream: &mut impl Read) -> Vec<u8> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let read = stream.read(&mut buffer).expect("read request");
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
        if let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&request[..header_end + 4]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.split_once(':').and_then(|(name, value)| {
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().expect("content length"))
                    })
                })
                .unwrap_or(0);
            if request.len() >= header_end + 4 + content_length {
                break;
            }
        }
    }
    request
}
