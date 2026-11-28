//! Stand-ins for the programs the scheduled scripts call (`gh`, `osascript`, `forager`): each
//! records its call and then runs the shell code a test gives it, so a test sees what a script
//! asks of them and no real issue, notification or live run happens. A script finds nothing else
//! on its `PATH` but the host's copies of the few tools it uses, so a program the host has
//! installed, such as a real `forager`, never stands in for a missing one.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use super::process::{command_behind_closed_proxy, run_command};

/// The host tools the scripts use besides the stand-ins and the shell's builtins. The scripts read
/// and write their JSON with `jq`, and a stand-in for it would prove nothing about the filters
/// they pass it, so these tests need the host's.
const HOST_TOOLS: [&str; 4] = ["dirname", "jq", "mkdir", "mv"];

/// A directory of stand-in programs and the log of their calls.
pub(crate) struct Fakes {
    directory: tempfile::TempDir,
    /// The shell code each stand-in runs after recording its call, by program.
    behaviours: Vec<(String, String)>,
}

impl Fakes {
    pub(crate) fn new() -> Self {
        let mut fakes = Self {
            directory: tempfile::tempdir().expect("create the stand-in directory"),
            behaviours: Vec::new(),
        };
        fs::create_dir(fakes.bin()).expect("create the stand-in bin directory");
        fs::create_dir(fakes.tools()).expect("create the host tool directory");
        for tool in HOST_TOOLS {
            std::os::unix::fs::symlink(host_program(tool), fakes.tools().join(tool))
                .expect("link a host tool");
        }
        fakes.program("osascript", "");
        fakes
    }

    fn bin(&self) -> PathBuf {
        self.directory.path().join("bin")
    }

    fn tools(&self) -> PathBuf {
        self.directory.path().join("tools")
    }

    /// Takes the host tool `name` off the scripts' `PATH`.
    pub(crate) fn without_tool(&self, name: &str) {
        fs::remove_file(self.tools().join(name)).expect("remove a host tool");
    }

    pub(crate) fn home(&self) -> PathBuf {
        self.directory.path().join("home")
    }

    pub(crate) fn state(&self) -> PathBuf {
        self.directory.path().join("state")
    }

    /// A stand-in for `name` that records its arguments and then runs `behaviour`.
    pub(crate) fn program(&mut self, name: &str, behaviour: &str) {
        let link = self.bin().join(name);
        if !link.exists() {
            std::os::unix::fs::symlink(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/release_scaffolding/stand-in"),
                link,
            )
            .expect("link a stand-in");
        }
        self.behaviours.retain(|(program, _)| program != name);
        self.behaviours
            .push((name.to_owned(), behaviour.to_owned()));
    }

    /// Runs the script at `script`, under the crate root, with `arguments` and `environment`.
    pub(crate) fn run(
        &self,
        script: &str,
        arguments: &[&str],
        environment: &[(&str, &str)],
    ) -> Output {
        let path = std::env::join_paths([self.bin(), self.tools()]).expect("join PATH");
        // launchd starts the scripts with /bin/bash, which macOS keeps at version 3.2.
        let mut command = command_behind_closed_proxy("/bin/bash");
        command
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join(script))
            .args(arguments)
            .env("PATH", path)
            .env("HOME", self.home())
            .env("XDG_STATE_HOME", self.state())
            .env("FAKE_LOG", self.directory.path().join("calls"))
            .envs(
                self.behaviours
                    .iter()
                    .map(|(program, behaviour)| (format!("FAKE_{program}"), behaviour)),
            )
            .envs(environment.iter().copied());
        run_command(&mut command, None)
    }

    /// Each recorded call, with a notification shown as its title and message.
    pub(crate) fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.directory.path().join("calls"))
            .unwrap_or_default()
            .split('\u{1e}')
            .filter(|record| !record.is_empty())
            .map(|record| {
                let call = record
                    .strip_suffix('\0')
                    .unwrap_or(record)
                    .split('\0')
                    .collect::<Vec<_>>();
                if call[0] == "osascript" {
                    format!("notify {} | {}", call[call.len() - 2], call[call.len() - 1])
                } else {
                    call.join(" ")
                }
            })
            .collect()
    }
}

/// Where the host's `PATH` finds the program `name`.
fn host_program(name: &str) -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|directory| directory.join(name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("the scheduled job tests need `{name}` on PATH"))
}

/// A `gh issue list` call that looks for the open issue labelled `label`, as the scripts write it.
pub(crate) fn issue_list(repository: &str, label: &str) -> String {
    format!(
        "gh issue list --repo {repository} --label {label} --state open --limit 1 --json number \
         --jq .[0].number // \"\""
    )
}
