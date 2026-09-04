mod support;

use std::collections::BTreeMap;
use std::fs;
use std::process::Command;

use serde_json::{Map, Value};
use support::run_command;

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
    assert!(
        workflow_run_commands(&ci_workflow())
            .iter()
            .all(|command| !command.contains("smoke --live"))
    );
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
    assert!(
        cargo_commands(&ci_workflow())
            .iter()
            .any(|command| is_full_cargo_test_command(command))
    );
    for command in [
        "cargo test --locked",
        "cargo test --all-targets --all-features --locked --no-fail-fast -- --quiet",
    ] {
        assert!(is_full_cargo_test_command(command), "command: {command}");
    }
    for command in [
        "cargo test --locked --package forager",
        "cargo test --locked -p forager",
        "cargo test --locked --test search",
        "cargo test --locked test_name",
    ] {
        assert!(!is_full_cargo_test_command(command), "command: {command}");
    }
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

    assert!(workflow_jobs(&workflow).values().any(|job| {
        job["runs-on"].as_str() == Some("windows-latest")
            && cargo_commands_in_job(job)
                .iter()
                .any(|command| command.starts_with("cargo test"))
    }));
}

#[test]
fn windows_ci_denies_clippy_warnings() {
    let workflow = ci_workflow();

    assert!(workflow_jobs(&workflow).values().any(|job| {
        job["runs-on"].as_str() == Some("windows-latest")
            && cargo_commands_in_job(job).iter().any(|command| {
                let arguments = command.split_whitespace().collect::<Vec<_>>();
                arguments.starts_with(&["cargo", "clippy"])
                    && arguments.contains(&"--all-targets")
                    && arguments.contains(&"--locked")
                    && arguments.windows(2).any(|pair| pair == ["-D", "warnings"])
            })
    }));
}

#[test]
fn windows_ci_runs_release_artifact_fixtures() {
    let workflow = ci_workflow();

    assert!(workflow_jobs(&workflow).values().any(|job| {
        job["runs-on"].as_str() == Some("windows-latest")
            && cargo_commands_in_job(job)
                .iter()
                .any(|command| command.contains("--test release_artifact_gate"))
    }));
}

#[test]
fn released_binary_reports_the_cargo_package_version() {
    let mut command = Command::new(env!("CARGO_BIN_EXE_forager"));
    command.arg("--version");
    let output = run_command(&mut command, None);

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
    let workflow = load_workflow(".github/workflows/release.yml");
    let dist_version = config["dist"]["cargo-dist-version"]
        .as_str()
        .expect("cargo-dist version");
    let expected_dist_download = format!("cargo-dist/releases/download/v{dist_version}");

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
        )
    );

    assert!(workflow["on"].as_object().is_some_and(|triggers| {
        triggers.contains_key("pull_request") && triggers.contains_key("workflow_dispatch")
    }));
    assert!(
        named_step(workflow_job(&workflow, "plan"), "Install dist")["run"]
            .as_str()
            .is_some_and(|run| run.contains(&expected_dist_download))
    );

    let gate = workflow_job(&workflow, "custom-release-artifact-gate");
    assert_eq!(string_list(&gate["needs"]), ["plan", "host"]);
    assert_eq!(
        gate["uses"].as_str(),
        Some("./.github/workflows/release-artifact-gate.yml")
    );
    assert_eq!(
        gate["with"]["plan"].as_str(),
        Some("${{ needs.plan.outputs.val }}")
    );
    assert_eq!(gate["permissions"]["contents"].as_str(), Some("write"));

    let announce = workflow_job(&workflow, "announce");
    assert!(
        string_list(&announce["needs"]).contains(&"custom-release-artifact-gate"),
        "dist announce must wait for the artifact gate"
    );
    assert!(
        named_step(
            workflow_job(&workflow, "host"),
            "Upload draft GitHub Release assets"
        )["run"]
            .as_str()
            .is_some_and(|run| run.contains("gh release upload"))
    );
    assert!(
        named_step(announce, "Publish verified GitHub Release")["run"]
            .as_str()
            .is_some_and(|run| run.contains("gh release edit") && run.contains("--draft=false"))
    );
}

#[test]
fn release_target_allowlist_matches_dist() {
    let source = fs::read_to_string("dist-workspace.toml").expect("read dist config");
    let config: toml::Value = toml::from_str(&source).expect("parse dist config");
    let dist_targets = config["dist"]["targets"]
        .as_array()
        .expect("dist targets")
        .iter()
        .map(|target| target.as_str().expect("target string"))
        .collect::<Vec<_>>();
    let allowlist: Value = serde_json::from_str(
        &fs::read_to_string(".github/release-targets.json").expect("read release targets"),
    )
    .expect("parse release targets");
    let gate_targets = ["unix", "windows"]
        .into_iter()
        .flat_map(|platform| {
            allowlist[platform]
                .as_array()
                .expect("platform release targets")
        })
        .map(|entry| entry["target"].as_str().expect("release target name"))
        .collect::<Vec<_>>();

    assert_eq!(dist_targets, gate_targets);
}

#[test]
fn release_target_preparation_is_fail_closed() {
    let directory = tempfile::tempdir().expect("create release target test directory");
    let output_path = directory.path().join("github-output");
    let mut command = Command::new("bash");
    command.args([
        ".github/scripts/prepare-release-targets.sh",
        ".github/release-targets.json",
    ]);
    command.arg(&output_path);
    let output = run_command(&mut command, None);
    assert_eq!(output.status.code(), Some(0), "{output:?}");

    let prepared = fs::read_to_string(&output_path).expect("read prepared release targets");
    let fields = prepared
        .lines()
        .map(|line| line.split_once('=').expect("release target output"))
        .collect::<BTreeMap<_, _>>();
    let targets: Value = serde_json::from_str(fields["targets"]).expect("prepared targets");
    let unix: Value = serde_json::from_str(fields["unix-matrix"]).expect("prepared Unix matrix");
    let windows: Value =
        serde_json::from_str(fields["windows-matrix"]).expect("prepared Windows matrix");
    let combined = unix["include"]
        .as_array()
        .expect("Unix matrix include")
        .iter()
        .chain(
            windows["include"]
                .as_array()
                .expect("Windows matrix include"),
        )
        .cloned()
        .collect::<Vec<_>>();
    assert!(!combined.is_empty());
    assert_eq!(targets, Value::Array(combined));

    let empty_source = directory.path().join("empty-targets.json");
    fs::write(&empty_source, r#"{"unix":[],"windows":[]}"#).expect("write empty release targets");
    let empty_output = directory.path().join("empty-output");
    let mut command = Command::new("bash");
    command
        .arg(".github/scripts/prepare-release-targets.sh")
        .arg(empty_source)
        .arg(empty_output);
    let output = run_command(&mut command, None);
    assert_ne!(output.status.code(), Some(0), "{output:?}");
}

#[test]
fn release_gate_declares_the_artifact_verification_wiring() {
    let workflow = load_workflow(".github/workflows/release-artifact-gate.yml");

    assert_release_target_job(&workflow);
    assert_release_checksum_job(&workflow);
    assert_release_unix_job(&workflow);
    assert_release_windows_job(&workflow);
    assert_release_record_job(&workflow);
}

fn assert_release_target_job(workflow: &Value) {
    let prepare = workflow_job(workflow, "prepare-targets");
    assert_eq!(
        (
            prepare["outputs"]["targets"].as_str(),
            prepare["outputs"]["unix-matrix"].as_str(),
            prepare["outputs"]["windows-matrix"].as_str(),
        ),
        (
            Some("${{ steps.targets.outputs.targets }}"),
            Some("${{ steps.targets.outputs.unix-matrix }}"),
            Some("${{ steps.targets.outputs.windows-matrix }}"),
        )
    );
    let prepare_run = named_step_by_id(prepare, "targets")["run"]
        .as_str()
        .expect("prepare target command");
    assert_eq!(
        prepare_run,
        "bash .github/scripts/prepare-release-targets.sh .github/release-targets.json \"$GITHUB_OUTPUT\""
    );
}

fn assert_release_checksum_job(workflow: &Value) {
    let checksums = workflow_job(workflow, "verify-checksums");
    assert_job_checks_out_scripts(checksums);
    assert_eq!(string_list(&checksums["needs"]), ["prepare-targets"]);
    let download_step = named_step(checksums, "Download draft Release assets");
    assert_eq!(
        (
            download_step["env"]["PLAN"].as_str(),
            download_step["env"]["TARGETS"].as_str(),
        ),
        (
            Some("${{ inputs.plan }}"),
            Some("${{ needs.prepare-targets.outputs.targets }}"),
        )
    );
    let download = download_step["run"]
        .as_str()
        .expect("checksum download command");
    assert!(
        download.contains("gh release download")
            && download.contains("$GITHUB_REPOSITORY")
            && download.contains("$TARGETS")
    );
    let checksum_step = named_step(checksums, "Verify archive checksums");
    assert_eq!(
        checksum_step["env"]["TARGETS"].as_str(),
        Some("${{ needs.prepare-targets.outputs.targets }}")
    );
    assert_eq!(
        checksum_step["run"].as_str(),
        Some("bash .github/scripts/verify-release-checksums.sh artifacts")
    );
}

fn assert_release_unix_job(workflow: &Value) {
    let unix = workflow_job(workflow, "verify-unix");
    assert_job_checks_out_scripts(unix);
    assert_eq!(
        string_list(&unix["needs"]),
        ["prepare-targets", "verify-checksums"]
    );
    assert_eq!(
        unix["strategy"]["matrix"].as_str(),
        Some("${{ fromJSON(needs.prepare-targets.outputs.unix-matrix) }}")
    );
    let unix_download = named_step(unix, "Download draft Release asset");
    assert_eq!(
        (
            unix_download["env"]["PLAN"].as_str(),
            unix_download["env"]["TARGET"].as_str(),
            unix_download["env"]["ARCHIVE"].as_str(),
        ),
        (
            Some("${{ inputs.plan }}"),
            Some("${{ matrix.target }}"),
            Some("${{ matrix.archive }}"),
        )
    );
    let unix_verify_step = named_step(unix, "Verify installed binary");
    assert_eq!(
        (
            unix_verify_step["shell"].as_str(),
            unix_verify_step["run"].as_str(),
            unix_verify_step["env"]["TARGET"].as_str(),
            unix_verify_step["env"]["ARCHIVE"].as_str(),
            unix_verify_step["env"]["HOST_ARCH"].as_str(),
            unix_verify_step["env"]["FILE_ARCH"].as_str(),
        ),
        (
            Some("bash"),
            Some("bash .github/scripts/verify-release-unix.sh"),
            Some("${{ matrix.target }}"),
            Some("${{ matrix.archive }}"),
            Some("${{ matrix.host_arch }}"),
            Some("${{ matrix.file_arch }}"),
        )
    );
}

fn assert_release_windows_job(workflow: &Value) {
    let windows = workflow_job(workflow, "verify-windows");
    assert_job_checks_out_scripts(windows);
    assert_eq!(
        string_list(&windows["needs"]),
        ["prepare-targets", "verify-checksums"]
    );
    assert_eq!(
        windows["strategy"]["matrix"].as_str(),
        Some("${{ fromJSON(needs.prepare-targets.outputs.windows-matrix) }}")
    );
    let windows_download = named_step(windows, "Download draft Release asset");
    assert_eq!(
        (
            windows_download["env"]["PLAN"].as_str(),
            windows_download["env"]["TARGET"].as_str(),
            windows_download["env"]["ARCHIVE"].as_str(),
        ),
        (
            Some("${{ inputs.plan }}"),
            Some("${{ matrix.target }}"),
            Some("${{ matrix.archive }}"),
        )
    );
    let windows_verify_step = named_step(windows, "Verify installed binary");
    assert_eq!(
        (
            windows_verify_step["shell"].as_str(),
            windows_verify_step["run"].as_str(),
            windows_verify_step["env"]["TARGET"].as_str(),
            windows_verify_step["env"]["ARCHIVE"].as_str(),
            windows_verify_step["env"]["EXPECTED_MACHINE"].as_str(),
        ),
        (
            Some("pwsh"),
            Some("./.github/scripts/verify-release-windows.ps1"),
            Some("${{ matrix.target }}"),
            Some("${{ matrix.archive }}"),
            Some("${{ matrix.machine }}"),
        )
    );
}

fn assert_job_checks_out_scripts(job: &Value) {
    assert!(job_steps(job).iter().any(|step| {
        step["uses"].as_str() == Some("actions/checkout@v6")
            && step["with"]["persist-credentials"].as_bool() == Some(false)
    }));
}

fn assert_release_record_job(workflow: &Value) {
    let record = workflow_job(workflow, "record-gate");
    assert_eq!(
        string_list(&record["needs"]),
        [
            "prepare-targets",
            "verify-checksums",
            "verify-unix",
            "verify-windows",
        ]
    );
    let record_step = named_step(record, "Record release gate");
    assert_eq!(
        (
            record_step["env"]["PLAN"].as_str(),
            record_step["env"]["TARGETS"].as_str(),
        ),
        (
            Some("${{ inputs.plan }}"),
            Some("${{ needs.prepare-targets.outputs.targets }}"),
        )
    );
    let record_run = record_step["run"].as_str().expect("record gate command");
    assert!(
        record_run.contains("--argjson targets \"$TARGETS\"")
            && record_run.contains("verified_targets: [$targets[].target]")
            && record_run.contains("release-gate.json")
    );
    assert!(job_steps(record).iter().any(|step| {
        step["uses"].as_str() == Some("actions/upload-artifact@v6")
            && step["with"]["name"].as_str() == Some("artifacts-release-gate")
    }));
}

#[test]
fn release_plz_owns_version_tags_and_dispatches_dist_without_package_wrappers() {
    let source = fs::read_to_string("release-plz.toml").expect("read release-plz config");
    let config: toml::Value = toml::from_str(&source).expect("parse release-plz config");
    let workflow_source =
        fs::read_to_string(".github/workflows/release-plz.yml").expect("read release-plz workflow");
    let workflow: Value =
        serde_yaml_ng::from_str(&workflow_source).expect("parse release-plz workflow");
    let release = workflow_job(&workflow, "release");
    let dispatch = named_step(release, "Dispatch dist for released tags")["run"]
        .as_str()
        .expect("release dispatch command");

    assert_eq!(
        (
            config["workspace"]["git_only"].as_bool(),
            config["workspace"]["publish"].as_bool(),
            config["workspace"]["git_release_enable"].as_bool(),
            config["workspace"]["git_tag_enable"].as_bool(),
            workflow_job(&workflow, "release-pr")["steps"]
                .as_array()
                .is_some_and(|steps| steps
                    .iter()
                    .any(|step| { step["with"]["command"].as_str() == Some("release-pr") })),
            job_steps(release)
                .iter()
                .any(|step| step["with"]["command"].as_str() == Some("release")),
            release["permissions"]["actions"].as_str() == Some("write"),
            dispatch.contains("gh release create"),
            dispatch.contains("--draft"),
            dispatch.contains("gh workflow run release.yml --field"),
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
}

#[test]
fn release_plz_runs_only_when_manually_dispatched() {
    let workflow = load_workflow(".github/workflows/release-plz.yml");
    let triggers = workflow["on"].as_object().expect("release-plz triggers");

    assert!(
        triggers.contains_key("workflow_dispatch") && !triggers.contains_key("push"),
        "release-plz must not run on ordinary main pushes"
    );
}

#[test]
fn release_plz_commands_use_github_app_tokens() {
    let workflow = load_workflow(".github/workflows/release-plz.yml");
    let release_plz_jobs = workflow_jobs(&workflow)
        .values()
        .filter(|job| {
            job_steps(job).iter().any(|step| {
                step["uses"]
                    .as_str()
                    .is_some_and(|uses| uses.starts_with("release-plz/action@"))
            })
        })
        .collect::<Vec<_>>();

    assert_eq!(release_plz_jobs.len(), 2);
    for job in release_plz_jobs {
        let token = named_step_by_id(job, "app-token");
        assert_eq!(
            token["uses"].as_str(),
            Some("actions/create-github-app-token@v2")
        );
        assert_eq!(
            token["with"]["app-id"].as_str(),
            Some("${{ secrets.APP_ID }}")
        );
        assert_eq!(
            token["with"]["private-key"].as_str(),
            Some("${{ secrets.APP_PRIVATE_KEY }}")
        );
        assert!(job_steps(job).iter().any(|step| {
            step["uses"]
                .as_str()
                .is_some_and(|uses| uses.starts_with("release-plz/action@"))
                && step["env"]["GITHUB_TOKEN"].as_str()
                    == Some("${{ steps.app-token.outputs.token }}")
        }));
    }
}

fn assert_runnable_job_timeouts(path: &str) {
    let workflow = load_workflow(path);
    let jobs = workflow_jobs(&workflow);
    assert!(!jobs.is_empty(), "{path} has no jobs");
    for (job_name, job) in jobs {
        if job.get("uses").is_some() {
            assert!(
                job.get("timeout-minutes").is_none(),
                "{path} job {job_name} cannot declare timeout-minutes while calling a reusable workflow"
            );
        } else {
            assert_eq!(
                job["timeout-minutes"].as_u64(),
                Some(20),
                "{path} job {job_name} must declare timeout-minutes: 20"
            );
        }
    }
}

fn load_workflow(path: &str) -> Value {
    let source = fs::read_to_string(path).expect("read workflow");
    serde_yaml_ng::from_str(&source).expect("parse workflow")
}

fn workflow_jobs(workflow: &Value) -> &Map<String, Value> {
    workflow["jobs"].as_object().expect("workflow jobs")
}

fn workflow_job<'a>(workflow: &'a Value, name: &str) -> &'a Value {
    workflow_jobs(workflow).get(name).expect("workflow job")
}

fn job_steps(job: &Value) -> &[Value] {
    job["steps"].as_array().expect("workflow job steps")
}

fn named_step<'a>(job: &'a Value, name: &str) -> &'a Value {
    job_steps(job)
        .iter()
        .find(|step| step["name"].as_str() == Some(name))
        .expect("named workflow step")
}

fn named_step_by_id<'a>(job: &'a Value, id: &str) -> &'a Value {
    job_steps(job)
        .iter()
        .find(|step| step["id"].as_str() == Some(id))
        .expect("workflow step id")
}

fn string_list(value: &Value) -> Vec<&str> {
    if let Some(value) = value.as_str() {
        vec![value]
    } else {
        value
            .as_array()
            .expect("string list")
            .iter()
            .map(|value| value.as_str().expect("string list item"))
            .collect()
    }
}

fn workflow_run_commands(workflow: &Value) -> Vec<&str> {
    workflow_jobs(workflow)
        .values()
        .flat_map(job_steps)
        .filter_map(|step| step["run"].as_str())
        .collect()
}

fn cargo_commands_in_job(job: &Value) -> Vec<String> {
    job_steps(job)
        .iter()
        .filter_map(|step| step["run"].as_str())
        .flat_map(str::lines)
        .map(str::trim)
        .filter(|command| command.starts_with("cargo "))
        .map(str::to_owned)
        .collect()
}

fn ci_workflow() -> Value {
    load_workflow(".github/workflows/ci.yml")
}

fn cargo_commands(workflow: &Value) -> Vec<String> {
    workflow_jobs(workflow)
        .values()
        .flat_map(cargo_commands_in_job)
        .collect()
}

fn is_full_cargo_test_command(command: &str) -> bool {
    let arguments = command.split_whitespace().collect::<Vec<_>>();
    if !arguments.starts_with(&["cargo", "test"]) || !arguments.contains(&"--locked") {
        return false;
    }
    let mut test_binary_arguments = false;
    arguments[2..].iter().all(|argument| {
        if *argument == "--" {
            test_binary_arguments = true;
            true
        } else if test_binary_arguments {
            *argument == "--quiet"
        } else {
            matches!(
                *argument,
                "--locked" | "--all-targets" | "--all-features" | "--no-fail-fast" | "--quiet"
            )
        }
    })
}
