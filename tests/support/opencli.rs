//! A fake `opencli` executable for process route tests.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

const CONTRACT: &str = "forager-ssrn/1";

const CALL_END: &str = "<<end of call>>";

/// A fake `opencli` in a temporary directory.
pub(crate) struct FakeOpenCli {
    directory: tempfile::TempDir,
}

impl FakeOpenCli {
    /// Prints `stdout` and `stderr`, then exits with `code`.
    pub(crate) fn answering(stdout: &str, stderr: &str, code: i32) -> Self {
        let fake = Self::with_body(&format!(
            "/bin/cat \"$dir/stdout\"\n/bin/cat \"$dir/stderr\" >&2\nexit {code}\n"
        ));
        fs::write(fake.path("stdout"), stdout).expect("write fake stdout");
        fs::write(fake.path("stderr"), stderr).expect("write fake stderr");
        fake
    }

    pub(crate) fn envelope(status: &str, data: &Value) -> Self {
        Self::answering(
            &json!({"contract": CONTRACT, "status": status, "data": data}).to_string(),
            "",
            0,
        )
    }

    /// Answers each command, selected by its second argument, with an `ok` envelope.
    pub(crate) fn by_command(pages: &[(&str, Value)]) -> Self {
        let cases = pages.iter().fold(String::new(), |mut cases, (command, _)| {
            let _ = writeln!(cases, "  {command}) /bin/cat \"$dir/{command}.json\" ;;");
            cases
        });
        let fake = Self::with_body(&format!("case \"$2\" in\n{cases}  *) exit 1 ;;\nesac\n"));
        for (command, data) in pages {
            fs::write(
                fake.path(&format!("{command}.json")),
                json!({"contract": CONTRACT, "status": "ok", "data": data}).to_string(),
            )
            .expect("write fake page");
        }
        fake
    }

    /// Starts a child that sleeps, records both process IDs, and never exits by itself.
    pub(crate) fn hanging() -> Self {
        Self::with_body(
            "/bin/sleep 60 &\nprintf '%s\\n' \"$!\" > \"$dir/child.pid\"\nprintf '%s\\n' \"$$\" > \"$dir/leader.pid\"\nwait\n",
        )
    }

    fn with_body(body: &str) -> Self {
        let directory = tempfile::tempdir().expect("create fake opencli directory");
        let fake = Self { directory };
        let script = format!(
            "#!/bin/sh\ndir='{}'\nfor argument in \"$@\"; do printf '%s\\n' \"$argument\" >> \"$dir/argv\"; done\nprintf '%s\\n' '{CALL_END}' >> \"$dir/argv\"\n{body}",
            fake.directory.path().display()
        );
        let executable = fake.executable();
        fs::write(&executable, script).expect("write fake opencli");
        make_executable(&executable);
        fake
    }

    pub(crate) fn path(&self, name: &str) -> PathBuf {
        self.directory.path().join(name)
    }

    pub(crate) fn executable(&self) -> PathBuf {
        self.path("opencli")
    }

    /// Returns the argv of every call, in order.
    pub(crate) fn calls(&self) -> Vec<Vec<String>> {
        let Ok(recorded) = fs::read_to_string(self.path("argv")) else {
            return Vec::new();
        };
        recorded
            .split_terminator(&format!("{CALL_END}\n"))
            .map(|call| call.lines().map(str::to_owned).collect())
            .collect()
    }

    pub(crate) fn config(&self, order: &str) -> String {
        format!(
            "[providers.ssrn_browser]\ncommand = {:?}\n\n[platforms.ssrn]\norder = {order}\n",
            self.executable().display().to_string()
        )
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("make fake executable");
}
