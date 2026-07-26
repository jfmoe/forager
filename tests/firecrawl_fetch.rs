mod support;

use serde_json::Value;

use support::{Fixture, RunEnvironment};

#[test]
fn firecrawl_fetch_decodes_scrape_results_through_the_real_http_stack() {
    let content = "Firecrawl fixture content. ".repeat(30);
    let body = serde_json::json!({
        "success": true,
        "data": {"markdown": content}
    })
    .to_string();
    let fixture = Fixture::start(200, "application/json", &body);
    let config = format!(
        r#"
[providers.jina]
keys = []

[providers.tavily]
keys = []

[providers.firecrawl]
url = {url:?}
keys = ["firecrawl-key"]
timeout = 2

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
            &Value::String("firecrawl".into()),
            Some(content.as_str())
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let request = fixture.finish();
    assert!(request.starts_with("POST /scrape "));
    assert!(request.contains("authorization: Bearer firecrawl-key"));
    assert!(request.contains("\"url\":\"https://example.test/article\""));
    assert!(request.contains("\"formats\":[\"markdown\"]"));
}
