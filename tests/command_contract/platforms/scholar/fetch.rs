use std::process::Output;

use serde_json::{Value, json};

use crate::support::scholar::{config, fixture};
use crate::support::{Fixture, RunEnvironment, ok_json, payload, stderr};

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
fn a_fetch_prints_the_cluster_it_names() {
    let (output, _) = run(&[CLUSTER], &fixture("cluster.json"));
    let item = payload(&output);

    assert_eq!(
        (
            output.status.code(),
            [&item["platform"], &item["provider"], &item["ref"]],
        ),
        (
            Some(0),
            [&json!("scholar"), &json!("serpapi"), &json!(CLUSTER)]
        ),
        "stderr: {}",
        stderr(&output)
    );
}
