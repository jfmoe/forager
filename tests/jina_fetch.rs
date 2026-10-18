mod support;

use serde_json::Value;

use support::{Fixture, Response, RunEnvironment};

#[test]
fn jina_fetch_preserves_content_while_redacting_protected_outputs() {
    let content = format!(
        "{}\nhttps://example.test/source?token=source-secret#fragment",
        "A substantial fixture paragraph. ".repeat(20)
    );
    let body = serde_json::json!({
        "code": 200,
        "status": 20000,
        "data": {
            "title": "Transport title",
            "url": "https://example.test/source",
            "content": content,
            "warning": "Transport warning"
        }
    })
    .to_string();
    let fixture = Fixture::start(200, "application/json", &body);
    let config = format!(
        r#"
[providers.jina]
url = {url:?}
keys = ["jina-canary"]
respond_with = "readerlm-v2"
timeout = 2

[providers.tavily]
keys = []

[providers.firecrawl]
keys = []

[journal]
enabled = false
"#,
        url = fixture.url
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&[
        "fetch",
        "https://example.test/source?token=source-secret#fragment",
        "--verbose",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["provider"],
            &payload["url"],
            payload["content"].as_str(),
            &payload["provider_attempts"][0]["seam"],
        ),
        (
            Some(0),
            &Value::String("jina".into()),
            &Value::String("https://example.test/source?token=********".into()),
            Some(content.as_str()),
            &Value::String("web_fetch".into()),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains("jina-canary"));

    let request = fixture.finish();
    assert!(request.starts_with("GET /https://example.test/source?token=source-secret"));
    assert!(request.contains("authorization: Bearer jina-canary"));
    assert!(request.contains("x-return-format: markdown"));
    assert!(request.contains("accept: application/json"));
    assert!(request.contains("x-respond-with: readerlm-v2"));
    for header in [
        "x-retain-links:",
        "x-target-selector:",
        "x-remove-selector:",
    ] {
        assert!(
            !request.contains(header),
            "unexpected header {header}: {request}"
        );
    }
}

const ARTICLE: &str = "https://example.test/article";

/// A config in the default order whose Firecrawl has no key.
fn anonymous_config(
    jina_url: &str,
    jina_keys: &[&str],
    tavily_url: &str,
    tavily_keys: &[&str],
) -> String {
    format!(
        r"
[providers.jina]
url = {jina_url:?}
keys = {jina_keys:?}
timeout = 5

[providers.tavily]
url = {tavily_url:?}
keys = {tavily_keys:?}
timeout = 5

[providers.firecrawl]
keys = []

[retry]
max_attempts = 1
multiplier = 1
max_wait = 0

[journal]
enabled = false
"
    )
}

fn article(content: &str) -> String {
    serde_json::json!({"code": 200, "status": 20000, "data": {"content": content.repeat(30)}})
        .to_string()
}

/// Each Jina attempt as its error kind and whether it sent no credential, in chain order.
fn jina_attempts(payload: &Value) -> Vec<(Value, Value)> {
    payload["provider_attempts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|attempt| attempt["provider"] == "jina")
        .map(|attempt| (attempt["error_kind"].clone(), attempt["anonymous"].clone()))
        .collect()
}

fn fetch(config: &str) -> (Option<i32>, Value, String) {
    let output = RunEnvironment::new(config).run(&["fetch", ARTICLE, "--verbose"]);
    let payload = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");
    (
        output.status.code(),
        payload,
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn jina_without_keys_reads_the_page_anonymously() {
    let jina = Fixture::start(
        200,
        "application/json",
        &article("Anonymous Jina content. "),
    );
    let tavily = Fixture::start_canary();

    let (code, payload, stderr) = fetch(&anonymous_config(&jina.url, &[], &tavily.url, &[]));

    assert_eq!(
        (code, &payload["provider"], jina_attempts(&payload)),
        (
            Some(0),
            &Value::String("jina".into()),
            vec![(Value::Null, Value::Bool(true))]
        ),
        "stderr: {stderr}"
    );
    let request = jina.finish();
    assert!(
        !request.to_ascii_lowercase().contains("authorization:")
            && request.contains("x-return-format: markdown")
            && request.contains("accept: application/json"),
        "{request}"
    );
    assert_eq!(tavily.finish_all().len(), 0);
}

#[test]
fn a_key_failure_is_followed_by_one_anonymous_request() {
    for (status, kind) in [
        (401, "auth"),
        (402, "quota_exhausted"),
        (429, "rate_limited"),
    ] {
        let jina = Fixture::start_sequence(vec![
            Response::json(status, r#"{"message":"key refused"}"#),
            Response::json(200, &article("Anonymous fallback content. ")),
        ]);
        let tavily = Fixture::start_canary();

        let (code, payload, stderr) = fetch(&anonymous_config(
            &jina.url,
            &["jina-key"],
            &tavily.url,
            &["tavily-key"],
        ));

        assert_eq!(
            (code, &payload["provider"], jina_attempts(&payload)),
            (
                Some(0),
                &Value::String("jina".into()),
                vec![
                    (Value::String(kind.into()), Value::Bool(false)),
                    (Value::Null, Value::Bool(true)),
                ]
            ),
            "status {status}, stderr: {stderr}"
        );
        let requests = jina.finish_all();
        assert_eq!(
            requests
                .iter()
                .map(|request| request.contains("authorization: Bearer jina-key"))
                .collect::<Vec<_>>(),
            [true, false]
        );
        assert_eq!(tavily.finish_all().len(), 0);
    }
}

#[test]
fn a_failure_unrelated_to_the_key_sends_no_anonymous_request() {
    for (case, response, kind) in [
        (
            "network",
            Response::json(503, r#"{"message":"unavailable"}"#),
            "network",
        ),
        (
            "timeout",
            Response::json(408, r#"{"message":"too slow"}"#),
            "timeout",
        ),
        ("thin", Response::json(200, &jina_thin()), "quality"),
    ] {
        let jina = Fixture::start_sequence(vec![response]);
        let tavily = Fixture::start(200, "application/json", &tavily_article());

        let (code, payload, stderr) = fetch(&anonymous_config(
            &jina.url,
            &["jina-key"],
            &tavily.url,
            &["tavily-key"],
        ));

        assert_eq!(
            (code, &payload["provider"], jina_attempts(&payload)),
            (
                Some(0),
                &Value::String("tavily".into()),
                vec![(Value::String(kind.into()), Value::Bool(false))]
            ),
            "case {case}, stderr: {stderr}"
        );
        assert_eq!(jina.finish_all().len(), 1, "case {case}");
        tavily.finish();
    }
}

#[test]
fn an_anonymous_refusal_hands_the_page_to_the_next_provider_without_retrying() {
    // Jina's body when it blocks anonymous access to a domain, recorded on 2026-10-18.
    let blocked = serde_json::json!({
        "data": null,
        "code": 403,
        "name": "AbuseAlleviationError",
        "status": 40305,
        "message": "Anonymous access to domain x.com blocked until Sun Oct 18 2026 04:21:55 GMT+0000 (Coordinated Universal Time) due to previous abuse found on http://x.com/globalstats11: DDoS attack suspected: Too many requests",
        "readableMessage": "AbuseAlleviationError: Anonymous access to domain x.com blocked until Sun Oct 18 2026 04:21:55 GMT+0000 (Coordinated Universal Time) due to previous abuse found on http://x.com/globalstats11: DDoS attack suspected: Too many requests"
    })
    .to_string();
    for (case, response, kind, message) in [
        (
            "blocked",
            Response::json(403, &blocked),
            "parameter",
            "Anonymous access to domain x.com blocked until Sun Oct 18 2026",
        ),
        (
            "rate_limited",
            Response::json(429, r#"{"message":"anonymous limit reached"}"#),
            "rate_limited",
            "anonymous limit reached",
        ),
    ] {
        let jina = Fixture::start_sequence(vec![response]);
        let tavily = Fixture::start(200, "application/json", &tavily_article());

        // Retries are allowed, so a single request shows the refusal is not retried.
        let config = anonymous_config(&jina.url, &[], &tavily.url, &["tavily-key"])
            .replace("max_attempts = 1", "max_attempts = 3");
        let (code, payload, stderr) = fetch(&config);
        let jina_message = payload["provider_attempts"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|attempt| attempt["provider"] == "jina")
            .and_then(|attempt| attempt["message"].as_str())
            .unwrap_or_default()
            .to_owned();

        assert_eq!(
            (
                code,
                &payload["provider"],
                jina_attempts(&payload),
                jina_message.contains(message),
            ),
            (
                Some(0),
                &Value::String("tavily".into()),
                vec![(Value::String(kind.into()), Value::Bool(true))],
                true,
            ),
            "case {case}, message {jina_message}, stderr: {stderr}"
        );
        assert_eq!(jina.finish_all().len(), 1, "case {case}");
        tavily.finish();
    }
}

fn tavily_article() -> String {
    serde_json::json!({"results": [{"raw_content": "Tavily fallback content. ".repeat(30)}]})
        .to_string()
}

fn jina_thin() -> String {
    serde_json::json!({"code": 200, "status": 20000, "data": {"content": "thin"}}).to_string()
}
