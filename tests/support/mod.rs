//! The shared test environment of the command contract tests: the isolated run environment of
//! the binary, the fixture server, the stored samples and the helpers built on them.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::Duration;

mod fixture_server;
mod process;

pub(crate) use fixture_server::*;
pub(crate) use process::*;

pub(crate) mod arxiv_feed;
pub(crate) mod browser;
pub(crate) mod doctor;
pub(crate) mod gemini_api;
pub(crate) mod gemini_api_samples;
pub(crate) mod hn;
pub(crate) mod hn_samples;
pub(crate) mod main_search_samples;
pub(crate) mod polymarket;
pub(crate) mod polymarket_samples;
pub(crate) mod samples;
pub(crate) mod scholar;
pub(crate) mod smoke;
pub(crate) mod ssrn_samples;
pub(crate) mod web_samples;
pub(crate) mod x;
pub(crate) mod x_samples;
pub(crate) mod youtube;
pub(crate) mod youtube_api_samples;
pub(crate) mod youtube_web;

impl Response {
    /// Holds the response back for `delay` after the request arrives.
    pub(crate) fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }
}

/// Every key of the execution object in a Search Result Journal record of schema version 1.
pub(crate) const JOURNAL_EXECUTION_KEYS: [&str; 5] = [
    "capability_gaps",
    "deadline_budget",
    "plan_summary",
    "provider_attempts",
    "terminal_attribution",
];

/// The keys of a JSON object, sorted, so a test can assert an object's complete shape.
pub(crate) fn object_keys(value: &serde_json::Value) -> Vec<&str> {
    let mut keys = value
        .as_object()
        .expect("JSON object")
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    keys.sort_unstable();
    keys
}

/// The JSON a run printed on stdout; a parse failure panics with both streams.
pub(crate) fn payload(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "parse JSON stdout: {error}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

pub(crate) fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The refs of a page's items, in order.
pub(crate) fn refs(page: &serde_json::Value) -> Vec<String> {
    page["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|item| item["ref"].as_str().expect("ref").to_owned())
        .collect()
}

/// Reads the single Search Result Journal record an environment wrote.
pub(crate) fn read_only_journal(environment: &RunEnvironment) -> serde_json::Value {
    let entry = fs::read_dir(environment.state_dir.join("forager/journal"))
        .expect("read journal")
        .next()
        .expect("journal entry")
        .expect("valid journal entry");
    serde_json::from_slice(&fs::read(entry.path()).expect("read journal record"))
        .expect("parse journal record")
}

/// The forager binary with an empty environment whose HTTP goes through a closed port.
pub(crate) fn forager_command() -> Command {
    forager_command_through(CLOSED_PROXY)
}

pub(crate) fn forager_command_through(proxy: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_forager"));
    command.env_clear();
    proxy_through(&mut command, proxy);
    #[cfg(windows)]
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }
    command
}

pub(crate) struct RunEnvironment {
    root: tempfile::TempDir,
    pub(crate) config_dir: PathBuf,
    pub(crate) state_dir: PathBuf,
}

impl RunEnvironment {
    pub(crate) fn new(config: &str) -> Self {
        let root = tempfile::tempdir().expect("create isolated root");
        let config_dir = root.path().join("config");
        let state_dir = root.path().join("state");
        fs::create_dir_all(&config_dir).expect("create config directory");
        fs::write(config_dir.join("config.toml"), config).expect("write config");
        Self {
            root,
            config_dir,
            state_dir,
        }
    }

    pub(crate) fn run(&self, arguments: &[&str]) -> Output {
        self.run_with_env(arguments, &[])
    }

    pub(crate) fn run_with_env(&self, arguments: &[&str], variables: &[(&str, &str)]) -> Output {
        let mut command = forager_command();
        command
            .args(arguments)
            .env("FORAGER_CONFIG_DIR", &self.config_dir)
            .env("XDG_STATE_HOME", &self.state_dir)
            .env("HOME", self.root.path());
        for (name, value) in variables {
            command.env(name, value);
        }
        run_command(&mut command, None)
    }
}
