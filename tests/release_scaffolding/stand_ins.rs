//! Stand-ins for the programs the scheduled scripts call (`gh`, `osascript`, `forager`): each
//! records its call and then runs the shell code a test gives it, so a test sees what a script
//! asks of them and no real issue, notification or live run happens.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use super::process::{command_behind_closed_proxy, run_command};

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
        fakes.program("osascript", "");
        fakes
    }

    fn bin(&self) -> PathBuf {
        self.directory.path().join("bin")
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
        let path = std::env::join_paths(std::iter::once(self.bin()).chain(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        )))
        .expect("join PATH");
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

/// A `gh issue list` call that looks for the open issue labelled `label`, as the scripts write it.
pub(crate) fn issue_list(repository: &str, label: &str) -> String {
    format!(
        "gh issue list --repo {repository} --label {label} --state open --limit 1 --json number \
         --jq .[0].number // \"\""
    )
}
