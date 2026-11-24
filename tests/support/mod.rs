//! The shared test environment of the command contract tests: the isolated run environment of
//! the binary, the fixture server, the stored samples and the helpers built on them.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::mpsc;

mod fixture_server;
mod process;

pub(crate) use fixture_server::*;
pub(crate) use process::*;

pub(crate) mod arxiv_api_samples;
pub(crate) mod browser;
pub(crate) mod doctor;
pub(crate) mod exa_samples;
pub(crate) mod gemini_api;
pub(crate) mod gemini_api_samples;
pub(crate) mod hn;
pub(crate) mod hn_api_samples;
pub(crate) mod main_search_samples;
pub(crate) mod polymarket;
pub(crate) mod polymarket_api_samples;
pub(crate) mod samples;
pub(crate) mod scholar;
pub(crate) mod ssrn_crossref_samples;
pub(crate) mod web_samples;
pub(crate) mod x;
pub(crate) mod youtube;
pub(crate) mod youtube_api_samples;
pub(crate) mod youtube_web;

/// A fixture whose one response waits until the test releases it, for a test that acts while
/// the request is in flight.
pub(crate) struct HeldFixture {
    pub(crate) url: String,
    fixture: Fixture,
    arrived: mpsc::Receiver<()>,
    release: mpsc::Sender<()>,
}

impl Fixture {
    pub(crate) fn start_held(response: Response) -> HeldFixture {
        let (arrived_tx, arrived) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let fixture = Self::serve(
            fixture_server::fixture_listener("held fixture"),
            Some(1),
            move |listener, stopped| {
                let mut stream = fixture_server::accept_request(&listener, &stopped, 1, 0);
                let request = fixture_server::read_request(&mut stream);
                let _ = arrived_tx.send(());
                let _ = released.recv();
                fixture_server::write_response(stream, &response);
                fixture_server::observe_extra_requests(&listener, &stopped, vec![request])
            },
        );
        HeldFixture {
            url: fixture.url.clone(),
            fixture,
            arrived,
            release,
        }
    }
}

impl HeldFixture {
    /// Waits until the request is in flight, failing within the command deadline.
    pub(crate) fn wait_for_request(&self) {
        self.arrived
            .recv_timeout(command_deadline())
            .expect("the held fixture's request did not arrive");
    }

    /// Releases the held response and returns the fixture to finish.
    pub(crate) fn release(self) -> Fixture {
        let _ = self.release.send(());
        self.fixture
    }
}

/// Whether a process with the ID `pid` still exists.
#[cfg(unix)]
pub(crate) fn process_exists(pid: &str) -> bool {
    Command::new("/bin/kill")
        .args(["-0", pid.trim()])
        .output()
        .is_ok_and(|output| output.status.success())
}

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

/// The forager binary isolated to `home`, reading its configuration from `config_dir` and
/// keeping its state in `state_dir`.
pub(crate) fn isolated_command(home: &Path, config_dir: &Path, state_dir: &Path) -> Command {
    let mut command = forager_command();
    command
        .env("FORAGER_CONFIG_DIR", config_dir)
        .env("XDG_STATE_HOME", state_dir)
        .env("HOME", home);
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
        let mut command = isolated_command(self.root.path(), &self.config_dir, &self.state_dir);
        command.args(arguments);
        for (name, value) in variables {
            command.env(name, value);
        }
        run_command(&mut command, None)
    }
}
