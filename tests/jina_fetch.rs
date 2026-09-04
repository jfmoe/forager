mod support;

use serde_json::Value;

use support::{Fixture, RunEnvironment};

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
