//! A fake `opencli` executable for process route tests.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

const CONTRACT: &str = "forager-ssrn/4";
pub(crate) const XHS_CONTRACT: &str = "forager-xhs/2";

const CALL_END: &str = "<<end of call>>";

/// The stdout of OpenCLI 2.x for an adapter command that returned `envelope`.
pub(crate) fn success(envelope: &Value) -> String {
    json!({"ok": true, "data": envelope}).to_string()
}

/// The stdout of OpenCLI 2.x for a failed command.
pub(crate) fn failure(code: &str, message: &str, exit: i32) -> String {
    json!({"ok": false, "error": {"code": code, "message": message, "exitCode": exit}}).to_string()
}

fn adapter_envelope(contract: &str, status: &str, data: &Value) -> String {
    success(&json!({"contract": contract, "status": status, "data": data}))
}

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

    /// Fails the way OpenCLI 2.x does: the error envelope on stdout, then exit `exit`.
    pub(crate) fn failing(code: &str, message: &str, exit: i32) -> Self {
        Self::answering(&failure(code, message, exit), "", exit)
    }

    pub(crate) fn envelope(status: &str, data: &Value) -> Self {
        Self::contract_envelope(CONTRACT, status, data)
    }

    /// Answers an envelope of another adapter contract.
    pub(crate) fn contract_envelope(contract: &str, status: &str, data: &Value) -> Self {
        Self::answering(&adapter_envelope(contract, status, data), "", 0)
    }

    /// Answers each command, selected by its second argument, with an `ok` envelope.
    pub(crate) fn by_command(pages: &[(&str, Value)]) -> Self {
        Self::contract_by_command(CONTRACT, pages)
    }

    /// Answers each command of another adapter contract with an `ok` envelope.
    pub(crate) fn contract_by_command(contract: &str, pages: &[(&str, Value)]) -> Self {
        let cases = pages.iter().fold(String::new(), |mut cases, (command, _)| {
            let _ = writeln!(cases, "  {command}) /bin/cat \"$dir/{command}.json\" ;;");
            cases
        });
        let fake = Self::with_body(&format!("case \"$2\" in\n{cases}  *) exit 1 ;;\nesac\n"));
        for (command, data) in pages {
            fs::write(
                fake.path(&format!("{command}.json")),
                adapter_envelope(contract, "ok", data),
            )
            .expect("write fake page");
        }
        fake
    }

    /// Answers the n-th call with the n-th envelope of `(status, data)`.
    pub(crate) fn sequence(envelopes: &[(&str, Value)]) -> Self {
        let fake = Self::with_body(&format!(
            "call=$(/usr/bin/grep -cxF '{CALL_END}' \"$dir/argv\")\n/bin/cat \"$dir/$call.json\"\n"
        ));
        for (index, (status, data)) in envelopes.iter().enumerate() {
            fs::write(
                fake.path(&format!("{}.json", index + 1)),
                adapter_envelope(CONTRACT, status, data),
            )
            .expect("write fake envelope");
        }
        fake
    }

    /// Writes `contents` as `file_name` and answers an `ok` envelope whose `download` points at
    /// it, like the adapter after a completed browser download.
    pub(crate) fn downloading(data: &Value, file_name: &str, contents: &[u8]) -> Self {
        let fake = Self::with_body("/bin/cat \"$dir/stdout\"\n");
        fs::write(fake.path(file_name), contents).expect("write fake download");
        let mut data = data.clone();
        data["download"] = json!({
            "state": "complete",
            "file": fake.path(file_name).display().to_string(),
        });
        fs::write(fake.path("stdout"), adapter_envelope(CONTRACT, "ok", &data))
            .expect("write fake stdout");
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
            "#!/bin/sh\ndir='{}'\nfor argument in \"$@\"; do printf '%s\\n' \"$argument\" >> \"$dir/argv\"; done\nprintf '%s\\n' '{CALL_END}' >> \"$dir/argv\"\n/usr/bin/env >> \"$dir/env\"\nprintf '%s\\n' '{CALL_END}' >> \"$dir/env\"\n{body}",
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
        self.recorded("argv")
    }

    /// Returns the environment of every call, in order, as `NAME=value` lines.
    pub(crate) fn environments(&self) -> Vec<Vec<String>> {
        self.recorded("env")
    }

    fn recorded(&self, name: &str) -> Vec<Vec<String>> {
        let Ok(recorded) = fs::read_to_string(self.path(name)) else {
            return Vec::new();
        };
        recorded
            .split_terminator(&format!("{CALL_END}\n"))
            .map(|call| call.lines().map(str::to_owned).collect())
            .collect()
    }

    pub(crate) fn config(&self, order: &str) -> String {
        self.route_config("ssrn_browser", "ssrn", order)
    }

    /// Points `route` at this fake and sets the order of `platform`.
    pub(crate) fn route_config(&self, route: &str, platform: &str, order: &str) -> String {
        format!(
            "[providers.{route}]\ncommand = {:?}\n\n[platforms.{platform}]\norder = {order}\n",
            self.executable().display().to_string()
        )
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("make fake executable");
}

// Process routes are Unix-only, so the tests that run this fake are too.
#[cfg(not(unix))]
fn make_executable(_path: &Path) {}
