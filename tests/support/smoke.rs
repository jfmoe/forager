//! The isolated environment of a `forager smoke` run and readers of its report.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

use super::run_command;

// Platform routes need no credentials, and the X route shares the main search key, so live
// smoke would run their cases unless the platform orders disable them.
pub(crate) const DISABLED_PLATFORMS: &str = "[platforms.arxiv]\norder = []\n[platforms.ssrn]\norder = []\n[platforms.x]\norder = []\n[platforms.hn]\norder = []\n";

pub(crate) struct SmokeEnvironment {
    _root: tempfile::TempDir,
    pub(crate) config_dir: PathBuf,
    pub(crate) state_dir: PathBuf,
    pub(crate) home_dir: PathBuf,
    pub(crate) journal_dir: PathBuf,
}

impl SmokeEnvironment {
    pub(crate) fn new(config: impl FnOnce(&Path) -> String) -> Self {
        let root = tempfile::tempdir().expect("create isolated root");
        let config_home = root.path().join("xdg-config");
        let config_dir = config_home.join("forager");
        let state_dir = root.path().join("xdg-state");
        let home_dir = root.path().join("home");
        let journal_dir = root.path().join("journal");
        fs::create_dir_all(&config_dir).expect("create config directory");
        fs::create_dir_all(&home_dir).expect("create home directory");
        forager::config::ensure_private_directory(&config_dir)
            .expect("restrict config directory permissions");
        let mut config_file = forager::config::create_private_file(&config_dir.join("config.toml"))
            .expect("create private config file");
        config_file
            .write_all(config(&journal_dir).as_bytes())
            .expect("write config");
        Self {
            _root: root,
            config_dir,
            state_dir,
            home_dir,
            journal_dir,
        }
    }

    pub(crate) fn run(&self, arguments: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_forager"));
        command
            .args(arguments)
            .env_clear()
            .env(
                "XDG_CONFIG_HOME",
                self.config_dir.parent().expect("XDG config home"),
            )
            .env("XDG_STATE_HOME", &self.state_dir)
            .env("HOME", &self.home_dir)
            .env("NO_PROXY", "127.0.0.1,localhost");
        #[cfg(windows)]
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", system_root);
        }
        run_command(&mut command, None)
    }
}

/// Returns the live case `id` of a smoke report.
pub(crate) fn case<'a>(payload: &'a Value, id: &str) -> &'a Value {
    payload["cases"]
        .as_array()
        .expect("live cases")
        .iter()
        .find(|case| case["id"] == id)
        .expect("registered live case")
}
