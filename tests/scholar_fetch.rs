mod support;

use std::collections::BTreeMap;
use std::process::Output;

use serde_json::{Value, json};

use support::{Fixture, Response, RunEnvironment};

const KEY: &str = "test-serpapi-key-a";
const CLUSTER: &str = "scholar:18208131694456651388";

/// Reads a trimmed real SerpApi response captured on 2026-10-07.
fn fixture(name: &str) -> Value {
    let path = format!(
        "{}/tests/fixtures/scholar/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).expect("read fixture")).expect("fixture")
}

fn config(url: &str) -> String {
    format!("[providers.serpapi]\nurl = \"{url}/search.json\"\nkeys = [\"{KEY}\"]\n")
}

fn fetch(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "scholar", "fetch"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

fn run(arguments: &[&str], body: &Value) -> (Output, String) {
    let fixture = Fixture::start_sequence(vec![Response::json(200, &body.to_string())]);
    let environment = RunEnvironment::new(&config(&fixture.url));
    let output = fetch(&environment, arguments);
    (output, fixture.finish())
}

fn payload(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "parse JSON stdout: {error}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn query_pairs(request: &str) -> BTreeMap<String, String> {
    let target = request.split_whitespace().nth(1).expect("request target");
    reqwest::Url::parse(&format!("http://fixture.test{target}"))
        .expect("request URL")
        .query_pairs()
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}

#[test]
fn fetch_requests_one_full_page_of_the_cluster() {
    let (output, request) = run(&[CLUSTER], &fixture("cluster.json"));

    assert_eq!(
        (output.status.code(), query_pairs(&request)),
        (
            Some(0),
            BTreeMap::from([
                ("engine".to_owned(), "google_scholar".to_owned()),
                ("hl".to_owned(), "en".to_owned()),
                ("cluster".to_owned(), "18208131694456651388".to_owned()),
                ("num".to_owned(), "20".to_owned()),
                ("api_key".to_owned(), KEY.to_owned()),
            ])
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn fetch_lists_the_cluster_versions_at_metadata_depth() {
    let (output, _) = run(&[CLUSTER], &fixture("cluster.json"));

    assert_eq!(
        payload(&output),
        json!({
            "platform": "scholar",
            "provider": "serpapi",
            "ref": "scholar:18208131694456651388",
            "url": "https://scholar.google.com/scholar?cluster=18208131694456651388",
            "depth": "metadata",
            "title": "Time series momentum$",
            "authors": ["TJ Moskowitz", "YH Ooi"],
            "published": "2012",
            "versions": [
                {
                    "title": "Time series momentum$",
                    "link": "https://www.trendfollowing.com/whitepaper/b.pdf",
                    "source": "TJ Moskowitz, YH Ooi… - Journal of Financial …, 2012 - trendfollowing.com",
                    "resources": [{
                        "title": "trendfollowing.com",
                        "file_format": "PDF",
                        "url": "https://www.trendfollowing.com/whitepaper/b.pdf"
                    }]
                },
                {
                    "title": "Replication of “Time series momentum”",
                    "link": "https://dmurav.com/replications/releases/v2026-09-06/reports/paper-f920fb6a4696.pdf",
                    "source": "TJ Moskowitz, YH Ooi, LH Pedersen - Journal of Financial Economics, 2012 - dmurav.com",
                    "resources": []
                },
                {
                    "title": "Time series momentum",
                    "link": null,
                    "source": "TJ Moskowitz, YH Ooi, LH Pedersen - Journal of Financial Economics, 2012",
                    "resources": []
                }
            ]
        })
    );
}

#[test]
fn a_cluster_url_fetches_the_cluster_it_names() {
    let (output, request) = run(
        &[
            "https://scholar.google.com/scholar?cluster=18208131694456651388&hl=en&num=20&as_sdt=0,27",
        ],
        &fixture("cluster.json"),
    );

    assert_eq!(
        (
            output.status.code(),
            payload(&output)["ref"].clone(),
            query_pairs(&request)["cluster"].clone()
        ),
        (
            Some(0),
            json!("scholar:18208131694456651388"),
            "18208131694456651388".to_owned()
        )
    );
}

#[test]
fn unrecognized_urls_and_depths_other_than_metadata_are_rejected_before_any_request() {
    for arguments in [
        &["https://scholar.google.com/scholar?cites=18208131694456651388"][..],
        &[
            "https://scholar.google.com/scholar?cluster=18208131694456651388&cites=18208131694456651388",
        ],
        &["https://scholar.google.com/scholar?cluster=1&cluster=2"],
        &["https://scholar.google.com/scholar?cluster=18446744073709551616"],
        &["https://scholar.google.com/citations?user=q9g8tuAAAAAJ&hl=en"],
        &[CLUSTER, "--depth", "snippet"],
        &[CLUSTER, "--depth", "abstract"],
        &[CLUSTER, "--depth", "full_text"],
    ] {
        let fixture = Fixture::start_canary();
        let environment = RunEnvironment::new(&config(&fixture.url));

        let output = fetch(&environment, arguments);

        assert_eq!(
            (output.status.code(), fixture.finish_all().len()),
            (Some(2), 0),
            "arguments: {arguments:?}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn a_cluster_with_more_versions_than_one_page_says_the_list_is_incomplete() {
    let mut cluster = fixture("cluster.json");
    cluster["serpapi_pagination"] = json!({
        "current": 1,
        "next": "https://serpapi.com/search.json?cluster=18208131694456651388&engine=google_scholar&hl=en&num=20&start=20"
    });

    let (output, _) = run(&[CLUSTER], &cluster);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        (
            output.status.code(),
            payload(&output)["versions"].as_array().map(Vec::len),
            stderr.contains("more versions of scholar:18208131694456651388"),
            stderr.contains("https://scholar.google.com/scholar?cluster=18208131694456651388"),
        ),
        (Some(0), Some(3), true, true),
        "stderr: {stderr}"
    );
}

#[test]
fn a_cluster_that_does_not_exist_is_an_attempt_parameter_failure() {
    let (output, _) = run(&["scholar:1"], &fixture("cl_missing.json"));
    let payload = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            &payload["error_kind"],
            &payload["message"]
        ),
        (
            Some(4),
            &json!("parameter"),
            &json!("Google Scholar has no cluster scholar:1")
        )
    );
}
