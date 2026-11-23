mod support;

use std::collections::BTreeMap;
use std::fs;
use std::process::Command;

use serde_json::{Map, Value};
use support::{forager_command, run_command};

const WORKFLOW_DIRECTORY: &str = ".github/workflows";

// cargo-dist generates this workflow and runs its plan job on pull requests as a dry run.
const DIST_WORKFLOW: &str = ".github/workflows/release.yml";

// Without a timeout, a hung fixture accept could hold a job until GitHub's six-hour limit (#52).
#[test]
fn every_runnable_workflow_job_has_a_twenty_minute_timeout() {
    for workflow in workflow_paths() {
        assert_runnable_job_timeouts(&workflow);
    }
}

// The `linux` and `windows` job names are what a ruleset on main is to require. A path filter
// would leave those checks pending on a pull request it skips (c8ab40c), so every pull request
// runs the whole workflow.
#[test]
fn one_workflow_runs_the_required_jobs_on_every_pull_request_and_main_push() {
    let workflow = pull_request_workflow();
    let pull_request = &workflow["on"]["pull_request"];
    let path_filters = ["paths", "paths-ignore"]
        .into_iter()
        .filter(|filter| pull_request.get(filter).is_some())
        .collect::<Vec<_>>();

    assert_eq!(
        (
            path_filters,
            string_list(&workflow["on"]["push"]["branches"]),
            workflow_jobs(&workflow)
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        ),
        (Vec::<&str>::new(), vec!["main"], vec!["linux", "windows"])
    );
}

// Live checks spend the maintainer's keys against real sites; #19 kept them out of every
// pull-request run.
#[test]
fn ci_does_not_run_live_smoke_checks() {
    assert!(
        workflow_run_commands(&pull_request_workflow())
            .iter()
            .all(|command| !command.contains("smoke --live"))
    );
}

// CI ran `cargo test`, one process with many threads, while local checks ran nextest, one process
// per test; a fixture race in the concurrent search test showed only under CI's runner. Linux now
// runs the local full check, unfiltered, through the `ci` profile.
#[test]
fn linux_runs_the_full_suite_through_the_local_runner_and_the_ci_profile() {
    let workflow = pull_request_workflow();
    assert!(
        cargo_commands_in_job(workflow_job(&workflow, "linux"))
            .iter()
            .any(|command| is_full_ci_suite_run(command))
    );
    assert!(is_full_ci_suite_run(
        "cargo nextest run --locked --all-features --profile ci --all-targets"
    ));
    for command in [
        "cargo nextest run --all-targets --all-features --locked",
        "cargo nextest run --profile ci --all-targets --all-features --locked --test search",
        "cargo nextest run --profile ci --all-targets --all-features --locked -E kind(lib)",
        "cargo nextest run --profile ci --all-targets --all-features --locked test_name",
    ] {
        assert!(!is_full_ci_suite_run(command), "command: {command}");
    }
}

// #72 made every dependency-resolving cargo command in CI build the versions Cargo.lock records.
#[test]
fn ci_locks_every_dependency_resolving_cargo_command() {
    assert!(
        cargo_commands(&pull_request_workflow())
            .iter()
            .filter(|command| !command.starts_with("cargo fmt "))
            .all(|command| command
                .split_whitespace()
                .any(|argument| argument == "--locked"))
    );
}

// The Windows modules of these targets run nowhere else. When d0be5ad narrowed the Windows job,
// the only Windows test of the read-side permission check stopped running and no test noticed.
#[test]
fn windows_runs_the_permission_and_release_artifact_tests_through_the_ci_profile() {
    let workflow = pull_request_workflow();
    let windows = workflow_job(&workflow, "windows");
    let selections = cargo_commands_in_job(windows)
        .iter()
        .filter_map(|command| ci_profile_nextest_targets(command))
        .flatten()
        .collect::<Vec<_>>();

    assert!(
        windows["runs-on"]
            .as_str()
            .is_some_and(|runner| runner.starts_with("windows")),
        "runs-on: {}",
        windows["runs-on"]
    );
    for target in ["config_permissions", "release_artifact_gate"] {
        assert!(
            selections.iter().any(|selected| selected == target),
            "{target} is not among the ci-profile selections {selections:?}"
        );
    }
}

#[test]
fn released_binary_reports_the_cargo_package_version() {
    let mut command = forager_command();
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

// Since ab0fb3b the artifact gate verifies the targets in its own file, a second list beside the
// one dist builds from; a target only dist lists would be published unverified.
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

// The release.yml that dist generates undrafts the Release before the artifact gate runs. Moving
// the undraft after the gate is a hand edit in that generated file, kept by `allow-dirty`;
// regenerating the workflow drops it, and nothing else notices before unverified artifacts are
// published.
#[test]
fn the_release_is_published_only_by_jobs_that_wait_for_the_artifact_gate() {
    let workflow = load_workflow(DIST_WORKFLOW);
    let publishing_jobs = workflow_jobs(&workflow)
        .iter()
        .filter(|(_, job)| {
            job.get("steps").is_some()
                && job_steps(job)
                    .iter()
                    .filter_map(|step| step["run"].as_str())
                    .any(|run| run.contains("gh release edit") && run.contains("--draft=false"))
        })
        .collect::<Vec<_>>();

    assert!(
        !publishing_jobs.is_empty(),
        "no job publishes the draft Release"
    );
    for (name, job) in publishing_jobs {
        assert!(
            string_list(&job["needs"]).contains(&"custom-release-artifact-gate"),
            "job {name} publishes the Release without waiting for the artifact gate"
        );
    }
}

// gh finds the repository from a checkout. The gate's draft Release downloads failed in jobs
// without one until fbf1ce1, and the job that records the gate still checks out nothing.
#[test]
fn every_release_asset_command_of_the_artifact_gate_names_the_repository() {
    let workflow = load_workflow(".github/workflows/release-artifact-gate.yml");
    let commands = workflow_run_commands(&workflow)
        .into_iter()
        .flat_map(|run| {
            run.replace("\\\n", " ")
                .replace("`\n", " ")
                .lines()
                .filter(|line| line.contains("gh release "))
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    assert!(!commands.is_empty());
    for command in commands {
        assert!(command.contains("--repo"), "command: {command}");
    }
}

// release-plz ran on every push to main until de1dda8; a release is now started by hand.
#[test]
fn release_plz_runs_only_when_manually_dispatched() {
    let workflow = load_workflow(".github/workflows/release-plz.yml");
    let triggers = workflow["on"].as_object().expect("release-plz triggers");

    assert!(
        triggers.contains_key("workflow_dispatch") && !triggers.contains_key("push"),
        "release-plz must not run on ordinary main pushes"
    );
}

// A release pull request opened with the default token starts no workflow, so its checks waited
// for the maintainer (#76); an app token lets them run.
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

fn workflow_paths() -> Vec<String> {
    let mut paths = fs::read_dir(WORKFLOW_DIRECTORY)
        .expect("read workflow directory")
        .map(|entry| entry.expect("workflow directory entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "yml"))
        .map(|path| path.to_str().expect("Unicode workflow path").to_owned())
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

/// The one workflow, apart from the dist workflow, that runs on pull requests.
fn pull_request_workflow() -> Value {
    let mut workflows = workflow_paths()
        .into_iter()
        .filter(|path| path != DIST_WORKFLOW)
        .map(|path| (load_workflow(&path), path))
        .filter(|(workflow, _)| workflow["on"].get("pull_request").is_some())
        .collect::<Vec<_>>();
    assert_eq!(
        workflows.len(),
        1,
        "pull request workflows: {:?}",
        workflows.iter().map(|(_, path)| path).collect::<Vec<_>>()
    );
    workflows.remove(0).0
}

fn cargo_commands(workflow: &Value) -> Vec<String> {
    workflow_jobs(workflow)
        .values()
        .flat_map(cargo_commands_in_job)
        .collect()
}

fn is_full_ci_suite_run(command: &str) -> bool {
    let Some(arguments) = command.strip_prefix("cargo nextest run ") else {
        return false;
    };
    let mut arguments = arguments.split_whitespace().collect::<Vec<_>>();
    arguments.sort_unstable();
    arguments
        == [
            "--all-features",
            "--all-targets",
            "--locked",
            "--profile",
            "ci",
        ]
}

/// The test targets a `cargo nextest run` through the `ci` profile selects by name.
fn ci_profile_nextest_targets(command: &str) -> Option<Vec<String>> {
    let arguments = command
        .strip_prefix("cargo nextest run ")?
        .split_whitespace()
        .collect::<Vec<_>>();
    arguments
        .windows(2)
        .any(|pair| pair == ["--profile", "ci"])
        .then(|| {
            arguments
                .windows(2)
                .filter(|pair| pair[0] == "--test")
                .map(|pair| pair[1].to_owned())
                .collect()
        })
}
