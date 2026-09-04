mod support;

use std::any::Any;
use std::io::{Read, Write};
use std::net::TcpStream;
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

#[test]
fn fixture_finish_rejects_an_ignored_extra_request() {
    let fixture = Fixture::start(200, "text/plain", "ok");

    send_request(&fixture.url);
    send_request(&fixture.url);
    let panic = panic::catch_unwind(AssertUnwindSafe(|| fixture.finish_all()))
        .expect_err("fixture must reject the extra request");

    assert!(
        panic_message(&panic)
            .contains("fixture received an unexpected number of requests: expected 1, received 2")
    );
}

#[test]
fn repeating_fixture_serves_until_finished_without_a_fixed_request_count() {
    let fixture = Fixture::start_repeating(Response::new(200, "text/plain", "ok"));

    send_request(&fixture.url);
    send_request(&fixture.url);
    let requests = fixture.finish_all();

    assert_eq!(requests.len(), 2);
}

fn send_request(url: &str) {
    let address = url.strip_prefix("http://").expect("fixture HTTP URL");
    let mut stream = TcpStream::connect(address).expect("connect to fixture");
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: fixture\r\nConnection: close\r\n\r\n")
        .expect("write fixture request");
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .expect("read fixture response");
}

fn panic_message(panic: &Box<dyn Any + Send>) -> &str {
    panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .expect("string panic")
}
