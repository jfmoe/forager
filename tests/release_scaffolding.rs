use std::fs;
use std::process::Command;

#[test]
fn ci_uses_the_repository_toolchain_and_runs_every_pull_request_gate() {
    let workflow = fs::read_to_string(".github/workflows/ci.yml").expect("read CI workflow");

    let required_fragments = [
        "actions-rust-lang/setup-rust-toolchain@v1",
        "quality:",
        "cargo fmt --check",
        "cargo check --all-targets --all-features --locked",
        "cargo clippy --all-targets --all-features --locked -- -D warnings",
        "unit:",
        "cargo test --lib --bin forager --all-features --locked",
        "transport-fixtures:",
        "--test anysearch",
        "--test tavily_fetch",
        "offline-e2e:",
        "cargo test --tests --all-features --locked",
        "coverage-tracking:",
        "cargo test --test acceptance_coverage --locked",
        "cargo test --lib provider_fixture_projection_matches_transport_manifest --locked",
        "windows-permissions:",
        "cargo test --test config_permissions --test smoke --locked",
    ];

    assert!(
        required_fragments
            .iter()
            .all(|fragment| workflow.contains(fragment)),
        "CI workflow is missing a required Rust gate"
    );
    assert!(
        !workflow.contains("smoke --live"),
        "live credential checks must not run in pull request CI"
    );
}

#[test]
fn released_binary_reports_the_cargo_package_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_forager"))
        .arg("--version")
        .output()
        .expect("run forager");

    assert_eq!(
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        ),
        (Some(0), format!("forager {}", env!("CARGO_PKG_VERSION"))),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn dist_dispatches_draft_releases_through_the_artifact_gate() {
    let source = fs::read_to_string("dist-workspace.toml").expect("read dist config");
    let config: toml::Value = toml::from_str(&source).expect("parse dist config");
    let workflow = fs::read_to_string(".github/workflows/release.yml").expect("read dist workflow");

    assert_eq!(
        (
            config["dist"]["ci"].as_str(),
            config["dist"]["pr-run-mode"].as_str(),
            config["dist"]["checksum"].as_str(),
            config["dist"]["dispatch-releases"].as_bool(),
            config["dist"]["create-release"].as_bool(),
            config["dist"]["github-release"].as_str(),
            config["dist"]["allow-dirty"].as_array(),
            config["dist"]["publish-jobs"].as_array(),
            config["dist"]["github-custom-job-permissions"]["release-artifact-gate"]["contents"]
                .as_str(),
            workflow.contains("pull_request:"),
            workflow.contains("cargo-dist/releases/download/v0.31.0"),
        ),
        (
            Some("github"),
            Some("plan"),
            Some("sha256"),
            Some(true),
            Some(false),
            Some("host"),
            Some(&vec![toml::Value::String("ci".into())]),
            Some(&vec![toml::Value::String("./release-artifact-gate".into())]),
            Some("write"),
            true,
            true,
        )
    );
    assert!(
        workflow.contains("custom-release-artifact-gate")
            && workflow.contains("- custom-release-artifact-gate"),
        "dist announce must wait for the artifact gate"
    );
    let upload = workflow
        .find("gh release upload")
        .expect("draft asset upload");
    let gate = workflow
        .find("\n  custom-release-artifact-gate:")
        .expect("artifact gate job");
    let publish = workflow
        .rfind("gh release edit")
        .expect("verified Release publish");
    assert!(
        upload < gate && gate < publish && workflow.contains("--draft=false"),
        "the draft Release must be published only after the artifact gate"
    );
}

#[test]
fn dist_supports_the_declared_release_targets() {
    let source = fs::read_to_string("dist-workspace.toml").expect("read dist config");
    let config: toml::Value = toml::from_str(&source).expect("parse dist config");
    let targets = config["dist"]["targets"]
        .as_array()
        .expect("dist targets")
        .iter()
        .map(|target| target.as_str().expect("target string"))
        .collect::<Vec<_>>();

    assert_eq!(
        targets,
        [
            "aarch64-apple-darwin",
            "aarch64-unknown-linux-gnu",
            "x86_64-apple-darwin",
            "x86_64-unknown-linux-gnu",
            "x86_64-pc-windows-msvc",
        ]
    );
}

#[test]
fn release_gate_installs_and_verifies_every_draft_release_asset() {
    let gate = fs::read_to_string(".github/workflows/release-artifact-gate.yml")
        .expect("read release artifact gate");

    for required_fragment in [
        "aarch64-apple-darwin",
        "aarch64-unknown-linux-gnu",
        "x86_64-apple-darwin",
        "x86_64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
        "gh release download",
        "--repo \"$GITHUB_REPOSITORY\"",
        "checksum_count",
        "sha256sum --check",
        "test -x",
        "uname -m",
        "command -v forager",
        "forager --version",
        "forager doctor",
        "Machine",
        "release-gate.json",
        "artifacts-release-gate",
    ] {
        assert!(
            gate.contains(required_fragment),
            "release artifact gate is missing {required_fragment}"
        );
    }
}

#[test]
fn release_plz_owns_version_tags_and_dispatches_dist_without_package_wrappers() {
    let source = fs::read_to_string("release-plz.toml").expect("read release-plz config");
    let config: toml::Value = toml::from_str(&source).expect("parse release-plz config");
    let workflow =
        fs::read_to_string(".github/workflows/release-plz.yml").expect("read release-plz workflow");

    assert_eq!(
        (
            config["workspace"]["git_only"].as_bool(),
            config["workspace"]["publish"].as_bool(),
            config["workspace"]["git_release_enable"].as_bool(),
            config["workspace"]["git_tag_enable"].as_bool(),
            workflow.contains("command: release-pr"),
            workflow.contains("command: release"),
            workflow.contains("actions: write"),
            workflow.contains("gh release create"),
            workflow.contains("--draft"),
            workflow.contains("gh workflow run release.yml --field"),
        ),
        (
            Some(true),
            Some(false),
            Some(false),
            Some(true),
            true,
            true,
            true,
            true,
            true,
            true,
        )
    );
    assert!(
        !format!("{source}\n{workflow}").contains("npm"),
        "release flow must not depend on npm"
    );
}

#[test]
fn release_plz_runs_only_when_manually_dispatched() {
    let workflow =
        fs::read_to_string(".github/workflows/release-plz.yml").expect("read release-plz workflow");

    assert!(
        workflow.contains("on:\n  workflow_dispatch:\n") && !workflow.contains("\n  push:"),
        "release-plz must not run on ordinary main pushes"
    );
}
