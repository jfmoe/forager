mod support;

use std::any::Any;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::panic::{self, AssertUnwindSafe};
use std::time::Duration;

use support::{Fixture, Response, Route};

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

#[test]
fn routed_fixture_answers_by_method_and_path_whatever_the_arrival_order() {
    let fixture = Fixture::start_routed(vec![
        Route::new("GET", "/one", Response::new(200, "text/plain", "first")),
        Route::new("POST", "/two", Response::new(503, "text/plain", "second")),
    ]);

    let second = exchange(&fixture.url, "POST", "/two");
    let first = exchange(&fixture.url, "GET", "/one");
    let requests = fixture.finish_all();

    assert_eq!(
        (
            first.lines().next(),
            first.ends_with("first"),
            second.lines().next(),
            second.ends_with("second"),
            requests
                .iter()
                .map(|request| request.lines().next().unwrap_or_default())
                .collect::<Vec<_>>(),
        ),
        (
            Some("HTTP/1.1 200 OK"),
            true,
            Some("HTTP/1.1 503 Error"),
            true,
            vec!["GET /one HTTP/1.1", "POST /two HTTP/1.1"],
        )
    );
}

#[test]
fn routed_fixture_finish_names_a_request_that_matches_no_route() {
    let fixture = Fixture::start_routed(vec![Route::new(
        "GET",
        "/one",
        Response::new(200, "text/plain", "ok"),
    )]);

    exchange(&fixture.url, "GET", "/one");
    let unrouted = exchange(&fixture.url, "POST", "/one?page=2");
    let panic = panic::catch_unwind(AssertUnwindSafe(|| fixture.finish_all()))
        .expect_err("fixture must reject the unrouted request");

    assert_eq!(
        (unrouted.lines().next(), panic_message(&panic)),
        (
            Some("HTTP/1.1 404 Error"),
            r#"routed fixture received requests that match no route: ["POST /one?page=2 HTTP/1.1"]"#
        )
    );
}

#[test]
fn routed_fixture_finish_names_a_route_that_was_never_requested() {
    let fixture = Fixture::start_routed(vec![
        Route::new("GET", "/one", Response::new(200, "text/plain", "ok")),
        Route::new("GET", "/two", Response::new(200, "text/plain", "ok")),
    ]);

    exchange(&fixture.url, "GET", "/one");
    let panic = panic::catch_unwind(AssertUnwindSafe(|| fixture.finish_all()))
        .expect_err("fixture must report the unrequested route");

    assert_eq!(
        panic_message(&panic),
        r#"routed fixture routes were never requested: ["GET /two"]"#
    );
}

#[test]
fn routed_fixture_answers_routes_sharing_a_method_and_path_in_the_order_given() {
    let fixture = Fixture::start_routed(vec![
        Route::new("GET", "/one", Response::new(200, "text/plain", "first")),
        Route::new("GET", "/one", Response::new(200, "text/plain", "second")),
    ]);

    let earlier = exchange(&fixture.url, "GET", "/one");
    let later = exchange(&fixture.url, "GET", "/one");
    fixture.finish_all();

    assert_eq!(
        (earlier.ends_with("first"), later.ends_with("second")),
        (true, true)
    );
}

#[test]
fn routed_fixture_matches_a_path_whatever_its_query() {
    let fixture = Fixture::start_routed(vec![Route::new(
        "GET",
        "/one",
        Response::new(200, "text/plain", "ok"),
    )]);

    let response = exchange(&fixture.url, "GET", "/one?page=1");
    let requests = fixture.finish_all();

    assert_eq!(
        (
            response.lines().next(),
            requests[0].lines().next().unwrap_or_default()
        ),
        (Some("HTTP/1.1 200 OK"), "GET /one?page=1 HTTP/1.1")
    );
}

fn send_request(url: &str) {
    exchange(url, "GET", "/");
}

fn exchange(url: &str, method: &str, path: &str) -> String {
    let address = url.strip_prefix("http://").expect("fixture HTTP URL");
    let mut stream = TcpStream::connect(address).expect("connect to fixture");
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: fixture\r\nConnection: close\r\n\r\n"
    )
    .expect("write fixture request");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("read fixture response");
    response
}

fn panic_message(panic: &Box<dyn Any + Send>) -> &str {
    panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .expect("string panic")
}
