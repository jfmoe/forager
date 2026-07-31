use std::collections::BTreeSet;
use std::fs;
use std::process::Command;

use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct AcceptanceManifest {
    unit: Vec<TrackedTest>,
    transport_fixtures: Vec<TransportFixture>,
    offline_e2e: Vec<TrackedTest>,
}

#[derive(Deserialize)]
struct TrackedTest {
    id: String,
    requirements: Vec<String>,
    test: String,
}

#[derive(Deserialize)]
struct TransportFixture {
    id: String,
    test: String,
}

#[test]
fn acceptance_manifest_tracks_every_tier_zero_and_tier_one_contract() {
    let manifest = manifest();
    let tracked = manifest
        .unit
        .iter()
        .chain(&manifest.offline_e2e)
        .flat_map(|test| test.requirements.iter().map(String::as_str))
        .collect::<BTreeSet<_>>();
    let expected = [
        "I0-1",
        "I0-2",
        "I0-3",
        "I0-4",
        "I0-5",
        "I0-6",
        "I0-7",
        "T1-aliases",
        "T1-command-tree",
        "T1-config",
        "T1-exit-codes",
        "T1-fallback",
        "T1-journal",
        "T1-map",
        "T1-output",
        "T1-plan",
        "T1-redaction",
        "T1-routing",
        "T1-sidecar-failure",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();

    assert_eq!(tracked, expected);
    assert_unique_ids(
        manifest
            .unit
            .iter()
            .map(|test| test.id.as_str())
            .chain(
                manifest
                    .transport_fixtures
                    .iter()
                    .map(|test| test.id.as_str()),
            )
            .chain(manifest.offline_e2e.iter().map(|test| test.id.as_str())),
    );
    for test in manifest
        .unit
        .iter()
        .map(|test| test.test.as_str())
        .chain(
            manifest
                .transport_fixtures
                .iter()
                .map(|test| test.test.as_str()),
        )
        .chain(manifest.offline_e2e.iter().map(|test| test.test.as_str()))
    {
        assert_test_exists(test);
    }
}

#[test]
fn specification_live_ids_equal_the_real_binary_registry() {
    let specification = specification_live_ids();
    let isolated = tempfile::tempdir().expect("isolated environment");
    let output = Command::new(env!("CARGO_BIN_EXE_forager"))
        .args(["smoke", "--live", "--list"])
        .env_clear()
        .env("HOME", isolated.path())
        .env("XDG_CONFIG_HOME", isolated.path().join("config"))
        .env("XDG_STATE_HOME", isolated.path().join("state"))
        .output()
        .expect("run live registry");
    let payload: Value = serde_json::from_slice(&output.stdout).expect("live registry JSON");
    let registered = payload["registered_case_ids"]
        .as_array()
        .expect("registered live case IDs")
        .iter()
        .map(|id| id.as_str().expect("case ID").to_owned())
        .collect::<BTreeSet<_>>();

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(registered, specification);
}

fn manifest() -> AcceptanceManifest {
    serde_json::from_str(
        &fs::read_to_string(root().join("tests/acceptance-manifest.json"))
            .expect("acceptance coverage manifest"),
    )
    .expect("parse acceptance coverage manifest")
}

fn specification_live_ids() -> BTreeSet<String> {
    let specification = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/docs/spec/forager/05-acceptance.md"
    ));
    let mut ids = specification
        .lines()
        .filter_map(|line| {
            ["P1", "P2"]
                .into_iter()
                .find(|id| line.starts_with(&format!("- **{id}**")))
                .map(str::to_owned)
        })
        .collect::<BTreeSet<_>>();
    for line in specification.lines() {
        let Some(id) = line
            .strip_prefix("| C")
            .and_then(|rest| rest.split_once(" |"))
            .map(|(number, _)| format!("C{number}"))
        else {
            continue;
        };
        ids.insert(id);
    }
    ids
}

fn assert_unique_ids<'a>(ids: impl Iterator<Item = &'a str>) {
    let mut unique = BTreeSet::new();
    for id in ids {
        assert!(unique.insert(id), "duplicate acceptance test ID {id}");
    }
}

fn assert_test_exists(reference: &str) {
    let (path, test) = reference
        .split_once("::")
        .expect("test reference uses path::name");
    let source = fs::read_to_string(root().join(path)).expect("read tracked test source");
    assert!(
        source.contains(&format!("fn {test}(")),
        "tracked test {reference} does not exist"
    );
}

fn root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
