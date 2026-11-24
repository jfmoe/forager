use std::env;
use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

const HELPER_MODE: &str = "FORAGER_WATCHDOG_HELPER";
const STARTED_MARKER: &str = "FORAGER_WATCHDOG_STARTED_MARKER";
const SURVIVED_MARKER: &str = "FORAGER_WATCHDOG_SURVIVED_MARKER";

#[test]
fn command_watchdog_times_out_a_hung_child() {
    let markers = tempfile::tempdir().expect("create watchdog marker directory");
    let started_marker = markers.path().join("started");
    let survived_marker = markers.path().join("survived");
    let executable = env::current_exe().expect("resolve watchdog executable");
    let mut command = crate::support::command_behind_closed_proxy(&executable);
    command
        .args(["--exact", &helper_test_name(), "--nocapture"])
        .env(HELPER_MODE, "wait-forever")
        .env(STARTED_MARKER, &started_marker)
        .env(SURVIVED_MARKER, &survived_marker);

    let started = Instant::now();
    let panic = catch_unwind(AssertUnwindSafe(|| {
        crate::support::run_command_with_deadline(&mut command, None, Duration::from_secs(2));
    }));

    assert!(panic.is_err(), "hung child did not trip the watchdog");
    assert!(
        started_marker.exists(),
        "helper did not start before timeout"
    );
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "watchdog did not respect its direct-child deadline"
    );
    thread::sleep(Duration::from_secs(3));
    assert!(
        !survived_marker.exists(),
        "helper survived the watchdog timeout"
    );
}

#[test]
fn watchdog_helper_writes_survival_marker() {
    if env::var(HELPER_MODE).as_deref() != Ok("wait-forever") {
        return;
    }
    fs::write(marker_path(STARTED_MARKER), b"started").expect("write started marker");
    thread::sleep(Duration::from_secs(4));
    fs::write(marker_path(SURVIVED_MARKER), b"survived").expect("write survived marker");
}

/// The helper's name as the test harness lists it: its module path inside the binary.
fn helper_test_name() -> String {
    let (_, module) = module_path!()
        .split_once("::")
        .expect("a module of the test binary");
    format!("{module}::watchdog_helper_writes_survival_marker")
}

fn marker_path(variable: &str) -> PathBuf {
    env::var_os(variable).map_or_else(|| panic!("missing {variable}"), PathBuf::from)
}
