mod support;

use serde_json::Value;

use support::samples::sample;
use support::{Fixture, RunEnvironment};

#[test]
fn map_sends_tavily_options_and_normalizes_site_results() {
    let fixture = Fixture::start(200, "application/json", &sample("tavily/map.json"));
    let environment = RunEnvironment::new(&map_config(&fixture.url, &["tavily-key"]));

    let output = environment.run(&[
        "map",
        "https://docs.example.test",
        "--instructions",
        "Only public API documentation",
        "--max-depth",
        "2",
        "--max-breadth",
        "10",
        "--limit",
        "25",
    ]);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse JSON stdout");

    assert_eq!(
        (
            output.status.code(),
            &payload["provider"],
            &payload["url"],
            &payload["base_url"],
            &payload["results"],
            &payload["response_time"],
        ),
        (
            Some(0),
            &Value::String("tavily".into()),
            &Value::String("https://docs.example.test".into()),
            &Value::String("https://docs.example.test".into()),
            &serde_json::json!([
                "https://docs.example.test/api?token=********",
                "https://docs.example.test/guides"
            ]),
            &Value::from(0.42),
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let request = fixture.finish();
    assert!(request.starts_with("POST /map "), "{request}");
    assert!(
        request.contains("authorization: Bearer tavily-key"),
        "{request}"
    );
    assert!(
        request.contains("\"url\":\"https://docs.example.test\""),
        "{request}"
    );
    assert!(
        request.contains("\"instructions\":\"Only public API documentation\""),
        "{request}"
    );
    assert!(request.contains("\"max_depth\":2"), "{request}");
    assert!(request.contains("\"max_breadth\":10"), "{request}");
    assert!(request.contains("\"limit\":25"), "{request}");
    assert!(request.contains("\"timeout\":150"), "{request}");
}

fn map_config(tavily_url: &str, tavily_keys: &[&str]) -> String {
    format!(
        r"
[providers.tavily]
url = {tavily_url:?}
keys = {tavily_keys:?}
timeout = 30

[retry]
max_attempts = 1
multiplier = 1
max_wait = 0

[journal]
enabled = false
"
    )
}
