mod support;

use serde_json::Value;

use support::{Fixture, RunEnvironment};

#[test]
fn jina_fetch_returns_redacted_content_through_the_real_http_stack() {
    let content = format!(
        "{}\nhttps://example.test/source?token=source-secret#fragment",
        "A substantial fixture paragraph. ".repeat(20)
    );
    let fixture = Fixture::start(200, "text/markdown", &content);
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
            payload["content"]
                .as_str()
                .is_some_and(|value| value.len() > 200),
            &payload["provider_attempts"][0]["seam"],
        ),
        (
            Some(0),
            &Value::String("jina".into()),
            &Value::String("https://example.test/source?token=********".into()),
            true,
            &Value::String("web_fetch".into()),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!combined.contains("jina-canary"));
    assert!(!combined.contains("source-secret"));

    let request = fixture.finish();
    assert!(request.starts_with("GET /https://example.test/source?token=source-secret"));
    assert!(request.contains("authorization: Bearer jina-canary"));
    assert!(request.contains("x-return-format: markdown"));
    assert!(request.contains("x-respond-with: readerlm-v2"));
}
