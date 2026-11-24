use std::process::Output;

use serde_json::{Value, json};

use crate::support::scholar::{config, fixture};
use crate::support::{Fixture, RunEnvironment, ok_json, payload};

const KEY: &str = "test-serpapi-key-a";
const CLUSTER: &str = "scholar:18208131694456651388";

fn fetch(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "scholar", "fetch"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

fn run(arguments: &[&str], body: &Value) -> (Output, String) {
    let fixture = Fixture::start_sequence(vec![ok_json(body)]);
    let environment = RunEnvironment::new(&config(&fixture.url, &[KEY]));
    let output = fetch(&environment, arguments);
    (output, fixture.finish())
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
