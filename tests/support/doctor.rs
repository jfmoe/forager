//! Shared configuration and assertions for `forager doctor` process tests.

use serde_json::Value;

use super::Response;

pub(crate) fn assert_deep_success(
    output: &std::process::Output,
    provider: &str,
    expected_checks: &[(&str, &str)],
) {
    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse doctor JSON");
    let checks = payload["checks"]
        .as_array()
        .expect("doctor checks array")
        .iter()
        .map(|check| {
            (
                check["name"].as_str().expect("check name"),
                check["transport"].as_str().expect("check transport"),
                check["ok"].as_bool().expect("check status"),
            )
        })
        .collect::<Vec<_>>();
    let expected = expected_checks
        .iter()
        .map(|(name, transport)| (*name, *transport, true))
        .collect::<Vec<_>>();
    assert_eq!(
        (
            output.status.code(),
            &payload["mode"],
            &payload["ok"],
            &payload["provider"],
            checks,
        ),
        (
            Some(0),
            &Value::String("deep".into()),
            &Value::Bool(true),
            &Value::String(provider.into()),
            expected,
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Shallow doctor counts any HTTP answer as reachable, so an unauthenticated 401 will do.
pub(crate) fn reachable_responses(count: usize) -> Vec<Response> {
    (0..count)
        .map(|_| Response::new(401, "application/json", ""))
        .collect()
}

/// Points the ten HTTP providers other than SerpApi at `url`, with a key wherever one is needed.
pub(crate) fn shallow_config(url: &str) -> String {
    format!(
        r#"
[providers.xai]
url = {url:?}
keys = ["xai-secret"]

[providers.openai_compatible]
url = {url:?}
keys = ["openai-secret"]

[providers.exa]
url = {url:?}
keys = ["exa-secret"]

[providers.tavily]
url = {url:?}
keys = ["tavily-secret"]

[providers.firecrawl]
url = {url:?}
keys = ["firecrawl-secret"]

[providers.jina]
url = {url:?}
keys = ["jina-secret"]

[providers.context7]
url = {url:?}
keys = ["context7-secret"]

[providers.anysearch]
url = {url:?}
keys = ["anysearch-secret"]

[providers.arxiv_api]
url = {url:?}

[providers.ssrn_crossref]
url = {url:?}
"#
    )
}
