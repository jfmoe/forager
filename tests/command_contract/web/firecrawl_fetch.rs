use serde_json::{Value, json};

use crate::support::samples::sample;
use crate::support::{Fixture, RunEnvironment, header, request_json};

#[test]
fn a_firecrawl_fetch_prints_the_markdown_scraped_with_the_configured_key() {
    let content = "Firecrawl fixture content. ".repeat(30);
    let mut answer: Value =
        serde_json::from_str(&sample("firecrawl/scrape.json")).expect("stored scrape answer");
    answer["data"]["markdown"] = json!(content);
    let body = answer.to_string();
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
    assert_eq!(
        (
            header(&request, "authorization"),
            &request_json(&request)["url"],
        ),
        (
            Some("Bearer firecrawl-key".to_owned()),
            &json!("https://example.test/article"),
        ),
        "{request}"
    );
}
