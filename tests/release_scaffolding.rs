use std::fs;

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
fn dist_scaffold_is_validated_on_pull_requests() {
    let source = fs::read_to_string("dist-workspace.toml").expect("read dist config");
    let config: toml::Value = toml::from_str(&source).expect("parse dist config");
    let workflow = fs::read_to_string(".github/workflows/release.yml").expect("read dist workflow");

    assert_eq!(
        (
            config["dist"]["ci"].as_str(),
            config["dist"]["pr-run-mode"].as_str(),
            workflow.contains("pull_request:"),
            workflow.contains("cargo-dist/releases/download/v0.31.0"),
        ),
        (Some("github"), Some("plan"), true, true)
    );
}

#[test]
fn release_plz_scaffold_owns_versions_without_publishing_crates() {
    let source = fs::read_to_string("release-plz.toml").expect("read release-plz config");
    let config: toml::Value = toml::from_str(&source).expect("parse release-plz config");
    let workflow =
        fs::read_to_string(".github/workflows/release-plz.yml").expect("read release-plz workflow");

    assert_eq!(
        (
            config["workspace"]["git_only"].as_bool(),
            config["workspace"]["publish"].as_bool(),
            config["workspace"]["git_release_enable"].as_bool(),
            workflow.contains("command: release-pr"),
        ),
        (Some(true), Some(false), Some(false), true)
    );
}
