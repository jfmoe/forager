mod support;

use std::any::Any;
use std::panic::{self, AssertUnwindSafe};
use std::time::Duration;

use support::{Fixture, Response};

#[test]
fn fixture_timeout_reports_expected_and_received_requests() {
    let fixture = Fixture::start_sequence_with_deadline(
        vec![Response::new(200, "text/plain", "ok")],
        Duration::from_millis(50),
    );

    let panic = panic::catch_unwind(AssertUnwindSafe(|| fixture.finish_all()))
        .expect_err("fixture must time out");

    assert_eq!(
        panic_message(&panic),
        "fixture timed out waiting for requests: expected 1, received 0"
    );
}

#[test]
fn parallel_fixture_timeout_reports_expected_and_received_requests() {
    let fixture = Fixture::start_parallel_sequence_with_deadline(
        vec![Response::new(200, "text/plain", "ok")],
        Duration::from_millis(50),
    );

    let panic = panic::catch_unwind(AssertUnwindSafe(|| fixture.finish_all()))
        .expect_err("parallel fixture must time out");

    assert_eq!(
        panic_message(&panic),
        "fixture timed out waiting for requests: expected 1, received 0"
    );
}

fn panic_message(panic: &Box<dyn Any + Send>) -> &str {
    panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .expect("string panic")
}
