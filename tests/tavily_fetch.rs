mod support;

use serde_json::Value;

use support::{Fixture, RunEnvironment, request_json};

#[test]
fn tavily_fetch_decodes_extract_results_through_the_real_http_stack() {
    let content = "Tavily fixture content. ".repeat(30);
    let body = serde_json::json!({
        "results": [{"url": "https://example.test/article", "raw_content": content}]
    })
    .to_string();
    let fixture = Fixture::start(200, "application/json", &body);
    let config = format!(
        r#"
[providers.jina]
keys = []

[providers.tavily]
url = {url:?}
keys = ["tavily-key"]
timeout = 2

[providers.firecrawl]
keys = []

[journal]
enabled = false
"#,
        url = fixture.url
    );
    let environment = RunEnvironment::new(&config);

    let output = environment.run(&["fetch", "https://example.test/article"]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["provider"],
            payload["content"].as_str(),
        ),
        (
            Some(0),
            &Value::String("tavily".into()),
            Some(content.as_str())
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let request = fixture.finish();
    assert!(request.starts_with("POST /extract "));
    assert!(request.contains("authorization: Bearer tavily-key"));
    assert_eq!(
        request_json(&request),
        serde_json::json!({
            "urls": ["https://example.test/article"],
            "format": "markdown",
            "extract_depth": "basic"
        })
    );
}
