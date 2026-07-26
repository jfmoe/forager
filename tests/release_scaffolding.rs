use std::fs;

#[test]
fn ci_uses_the_repository_toolchain_and_runs_all_rust_gates() {
    let workflow = fs::read_to_string(".github/workflows/ci.yml").expect("read CI workflow");

    let required_fragments = [
        "actions-rust-lang/setup-rust-toolchain@v1",
        "cargo fmt --check",
        "cargo clippy --all-targets --all-features --locked -- -D warnings",
        "cargo test --all-targets --all-features --locked",
    ];

    assert!(
        required_fragments
            .iter()
            .all(|fragment| workflow.contains(fragment)),
        "CI workflow is missing a required Rust gate"
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
