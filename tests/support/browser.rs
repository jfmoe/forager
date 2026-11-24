//! A shell stand-in for the browser that `browser.path` points at. It speaks no CDP: it only
//! records its arguments and then exits or hangs.

use std::fs;
use std::path::{Path, PathBuf};

/// The line the stand-in script writes after the arguments of each launch.
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

    /// Links a new directory's `browser` to the shared stand-in script, which runs `body` from
    /// the same directory. One script for every test, instead of a new executable each, keeps the
    /// host from vetting a new executable on each test's first launch, which takes long enough
    /// on macOS to run a test into its limit.
    fn with_body(body: &str) -> Self {
        let directory = tempfile::tempdir().expect("create stand-in directory");
        let stand_in = Self { directory };
        fs::write(stand_in.path("body"), body).expect("write stand-in body");
        link_script(&stand_in.executable());
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

    /// A configuration that launches this stand-in and sets the Xiaohongshu order to `order`.
    pub(crate) fn xiaohongshu_config(&self, order: &str) -> String {
        format!(
            "{}\n[platforms.xiaohongshu]\norder = {order}\n",
            self.config()
        )
    }

    /// A configuration that launches this stand-in and sets the Reddit order to `order`.
    pub(crate) fn reddit_config(&self, order: &str) -> String {
        format!("{}\n[platforms.reddit]\norder = {order}\n", self.config())
    }
}

/// Marks the Browser Profile of `platform` under `state_dir` as logged in, the way a finished
/// `forager browser login` leaves it.
pub(crate) fn mark_logged_in(state_dir: &Path, platform: &str) {
    let profile = state_dir.join("forager/browser").join(platform);
    fs::create_dir_all(&profile).expect("create the Browser Profile");
    fs::write(
        profile.join("login.json"),
        "{\"logged_in_at\":\"2026-11-18T08:00:00+00:00\"}\n",
    )
    .expect("write the login marker");
}

#[cfg(unix)]
fn link_script(link: &Path) {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/browser_stand_in.sh");
    std::os::unix::fs::symlink(script, link).expect("link the stand-in script");
}

#[cfg(not(unix))]
fn link_script(_link: &Path) {}
