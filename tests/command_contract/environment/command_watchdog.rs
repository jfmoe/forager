//! The command deadline of the shared test environment: a command that outlives it is killed
//! and the test fails with what the command printed.
#![cfg(unix)]

use std::env;
use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::process::Stdio;
use std::thread;
use std::time::{Duration, Instant};

use crate::support::{
    command_behind_closed_proxy, command_deadline, process_exists, spawn_command, wait_for_child,
};

const HELPER_MODE: &str = "FORAGER_WATCHDOG_HELPER";
const PID_FILE: &str = "FORAGER_WATCHDOG_PID_FILE";

#[test]
fn command_watchdog_kills_a_hung_child_and_reports_it() {
    let markers = tempfile::tempdir().expect("create watchdog marker directory");
    let pid_file = markers.path().join("pid");
    let executable = env::current_exe().expect("resolve watchdog executable");
    let mut command = command_behind_closed_proxy(&executable);
    command
        .args(["--exact", &helper_test_name(), "--nocapture"])
        .env(HELPER_MODE, "wait-forever")
        .env(PID_FILE, &pid_file)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = spawn_command(&mut command).expect("spawn the hung helper");
    let pid = wait_for_pid(&pid_file);

    let panic = catch_unwind(AssertUnwindSafe(|| {
        wait_for_child(child, "hung helper", Duration::ZERO);
    }))
    .expect_err("the watchdog must fail the test");
    let message = panic
        .downcast_ref::<String>()
        .map_or("", String::as_str)
        .to_owned();

    assert_eq!(
        (
            message.starts_with("test command exceeded"),
            process_exists(&pid)
        ),
        (true, false),
        "{message}"
    );
}

/// Starts, records its process ID, and never ends on its own.
#[test]
fn watchdog_helper_waits_forever() {
    if env::var(HELPER_MODE).as_deref() != Ok("wait-forever") {
        return;
    }
    let pid_file =
        env::var_os(PID_FILE).map_or_else(|| panic!("missing {PID_FILE}"), PathBuf::from);
    // Written whole and then renamed, so the test never reads part of the number.
    let partial = pid_file.with_extension("partial");
    fs::write(&partial, std::process::id().to_string()).expect("write the helper's pid");
    fs::rename(partial, pid_file).expect("publish the helper's pid");
    loop {
        thread::park();
    }
}

/// The helper's process ID, once it has started.
fn wait_for_pid(pid_file: &std::path::Path) -> String {
    let deadline = Instant::now() + command_deadline();
    loop {
        if let Ok(pid) = fs::read_to_string(pid_file) {
            return pid;
        }
        assert!(Instant::now() < deadline, "the helper did not start");
        thread::sleep(Duration::from_millis(5));
    }
}

/// The helper's name as the test harness lists it: its module path inside the binary.
fn helper_test_name() -> String {
    let (_, module) = module_path!()
        .split_once("::")
        .expect("a module of the test binary");
    format!("{module}::watchdog_helper_waits_forever")
}
