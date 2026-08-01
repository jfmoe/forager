use std::fs;
use std::process::Command;

#[test]
fn every_runnable_workflow_job_has_a_twenty_minute_timeout() {
    for workflow in [
        ".github/workflows/ci.yml",
        ".github/workflows/release.yml",
        ".github/workflows/release-plz.yml",
        ".github/workflows/release-artifact-gate.yml",
    ] {
        assert_runnable_job_timeouts(workflow);
    }
}

#[test]
fn ci_does_not_run_live_smoke_checks() {
    assert!(!ci_workflow().contains("smoke --live"));
}

#[test]
fn ci_checks_rust_formatting() {
    assert!(cargo_commands(&ci_workflow()).iter().any(|command| {
        let arguments = command.split_whitespace().collect::<Vec<_>>();
        arguments.starts_with(&["cargo", "fmt"]) && arguments.contains(&"--check")
    }));
}

#[test]
fn ci_denies_clippy_warnings() {
    assert!(cargo_commands(&ci_workflow()).iter().any(|command| {
        let arguments = command.split_whitespace().collect::<Vec<_>>();
        arguments.starts_with(&["cargo", "clippy"])
            && arguments.windows(2).any(|pair| pair == ["-D", "warnings"])
    }));
}

#[test]
fn ci_runs_the_full_unfiltered_test_suite() {
    assert!(cargo_commands(&ci_workflow()).iter().any(|command| {
        let mut arguments = command.split_whitespace();
        arguments.next() == Some("cargo")
            && arguments.next() == Some("test")
            && arguments.all(|argument| argument == "--locked")
    }));
}

#[test]
fn ci_locks_every_dependency_resolving_cargo_command() {
    assert!(
        cargo_commands(&ci_workflow())
            .iter()
            .filter(|command| !command.starts_with("cargo fmt "))
            .all(|command| command
                .split_whitespace()
                .any(|argument| argument == "--locked"))
    );
}

#[test]
fn ci_tests_on_a_windows_runner() {
    let workflow = ci_workflow();
    let jobs = workflow.split_once("\njobs:\n").expect("workflow jobs").1;

    assert!(workflow_job_bodies(jobs).iter().any(|job| {
        job.lines()
            .any(|line| line.trim() == "runs-on: windows-latest")
            && cargo_commands(job)
                .iter()
                .any(|command| command.starts_with("cargo test"))
    }));
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

fn assert_runnable_job_timeouts(path: &str) {
    let workflow = fs::read_to_string(path).expect("read workflow");
    let jobs = workflow.split_once("\njobs:\n").expect("workflow jobs").1;
    let mut job_name = None;
    let mut job_body = Vec::new();
    let mut job_count = 0;

    for line in jobs.lines() {
        if line.starts_with("  ") && !line.starts_with("    ") && line.ends_with(':') {
            if let Some(name) = job_name.take() {
                assert_job_timeout(path, name, &job_body);
            }
            job_name = Some(line.trim_end_matches(':').trim());
            job_body.clear();
            job_count += 1;
        } else {
            job_body.push(line);
        }
    }
    if let Some(name) = job_name {
        assert_job_timeout(path, name, &job_body);
    }

    assert!(job_count > 0, "{path} has no jobs");
}

fn assert_job_timeout(path: &str, job_name: &str, body: &[&str]) {
    let calls_reusable_workflow = body.iter().any(|line| line.starts_with("    uses:"));
    let has_timeout = body.iter().any(|line| line.trim() == "timeout-minutes: 20");

    if calls_reusable_workflow {
        assert!(
            !has_timeout,
            "{path} job {job_name} cannot declare timeout-minutes while calling a reusable workflow"
        );
    } else {
        assert!(
            has_timeout,
            "{path} job {job_name} must declare timeout-minutes: 20"
        );
    }
}

fn ci_workflow() -> String {
    fs::read_to_string(".github/workflows/ci.yml").expect("read CI workflow")
}

fn cargo_commands(workflow: &str) -> Vec<String> {
    let lines = workflow.lines().collect::<Vec<_>>();
    let mut commands = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index];
        let indentation = line.len() - line.trim_start().len();
        let Some(run) = line.trim().strip_prefix("- run: ") else {
            index += 1;
            continue;
        };

        let command = if matches!(run, ">-" | "|" | "|-" | ">") {
            index += 1;
            let mut parts = Vec::new();
            while index < lines.len()
                && lines[index].len() - lines[index].trim_start().len() > indentation
            {
                parts.push(lines[index].trim());
                index += 1;
            }
            parts.join(" ")
        } else {
            index += 1;
            run.to_owned()
        };

        if command.starts_with("cargo ") {
            commands.push(command);
        }
    }

    commands
}

fn workflow_job_bodies(jobs: &str) -> Vec<String> {
    let mut bodies = Vec::new();
    let mut body = Vec::new();

    for line in jobs.lines() {
        let starts_job = line.starts_with("  ") && !line.starts_with("    ") && line.ends_with(':');
        if starts_job {
            if !body.is_empty() {
                bodies.push(body.join("\n"));
                body.clear();
            }
        } else {
            body.push(line);
        }
    }
    if !body.is_empty() {
        bodies.push(body.join("\n"));
    }

    bodies
}
