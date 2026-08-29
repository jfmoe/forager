mod support;

use std::env;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

const HELPER_MODE: &str = "FORAGER_WATCHDOG_HELPER";

#[test]
fn command_watchdog_times_out_a_hung_child() {
    let mut command = Command::new(env::current_exe().expect("resolve watchdog executable"));
    command
        .args(["--exact", "watchdog_helper_waits_forever", "--nocapture"])
        .env(HELPER_MODE, "wait-forever");

    let started = Instant::now();
    let panic = catch_unwind(AssertUnwindSafe(|| {
        support::run_command_with_deadline(&mut command, None, Duration::from_millis(100));
    }));

    assert!(panic.is_err(), "hung child did not trip the watchdog");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "watchdog did not respect its direct-child deadline"
    );
}

#[test]
fn watchdog_helper_waits_forever() {
    if env::var(HELPER_MODE).as_deref() != Ok("wait-forever") {
        return;
    }
    loop {
        thread::park();
    }
}
