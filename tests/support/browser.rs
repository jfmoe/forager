//! A shell stand-in for the browser that `browser.path` points at. It speaks no CDP: it only
//! records its arguments and then exits or hangs.

use std::fs;
use std::path::{Path, PathBuf};

const CALL_END: &str = "<<end of call>>";

/// A stand-in browser executable in a temporary directory.
pub(crate) struct StandIn {
    directory: tempfile::TempDir,
}

impl StandIn {
    /// Writes `message` to stderr and exits with 1, as a browser that fails to start does.
    pub(crate) fn failing(message: &str) -> Self {
        Self::with_body(&format!("printf '%s\\n' '{message}' >&2\nexit 1\n"))
    }

    /// Starts a child that sleeps, records both process IDs, and never answers.
    pub(crate) fn hanging() -> Self {
        Self::with_body(
            "/bin/sleep 60 &\nprintf '%s\\n' \"$!\" > \"$dir/child.pid\"\nprintf '%s\\n' \"$$\" > \"$dir/leader.pid\"\nwait\n",
        )
    }

    fn with_body(body: &str) -> Self {
        let directory = tempfile::tempdir().expect("create stand-in directory");
        let stand_in = Self { directory };
        let script = format!(
            "#!/bin/sh\ndir='{}'\nfor argument in \"$@\"; do printf '%s\\n' \"$argument\" >> \"$dir/argv\"; done\nprintf '%s\\n' '{CALL_END}' >> \"$dir/argv\"\n{body}",
            stand_in.directory.path().display()
        );
        let executable = stand_in.executable();
        fs::write(&executable, script).expect("write stand-in");
        make_executable(&executable);
        stand_in
    }

    pub(crate) fn path(&self, name: &str) -> PathBuf {
        self.directory.path().join(name)
    }

    pub(crate) fn executable(&self) -> PathBuf {
        self.path("browser")
    }

    /// The arguments of every launch, in order.
    pub(crate) fn launches(&self) -> Vec<Vec<String>> {
        let Ok(recorded) = fs::read_to_string(self.path("argv")) else {
            return Vec::new();
        };
        recorded
            .split_terminator(&format!("{CALL_END}\n"))
            .map(|call| call.lines().map(str::to_owned).collect())
            .collect()
    }

    /// A configuration whose `browser.path` launches this stand-in.
    pub(crate) fn config(&self) -> String {
        format!(
            "[browser]\npath = {:?}\n",
            self.executable().display().to_string()
        )
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("make stand-in executable");
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) {}
