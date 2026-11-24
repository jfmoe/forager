use std::error::Error;
use std::fmt::Debug;
use std::future::{self, Ready};
use std::marker::PhantomData;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};

use reqwest::Client;
use tower_layer::Layer;
use tower_service::Service;

use super::{BoxError, refused_target};
use crate::fixture_server::block_on;
use crate::net::build_client;

/// Pins how the guard reads what reqwest's connector actually receives, so a reqwest upgrade
/// that changes that request's `Debug` form fails here by name.
#[test]
fn the_guard_reads_the_target_of_the_request_the_connector_receives() {
    let targets = connector_requests(&[
        "http://localhost:8080/path",
        "http://127.0.0.1:8080/path",
        "http://[::1]:8080/path",
        "http://forager-guard.example/path",
        "http://192.0.2.1:8080/path",
    ]);

    assert_eq!(
        targets
            .iter()
            .map(|target| refused_target(target))
            .collect::<Vec<_>>(),
        [
            None,
            None,
            None,
            Some("forager-guard.example".to_owned()),
            Some("192.0.2.1".to_owned()),
        ],
        "connector requests: {targets:#?}"
    );
}

/// The `Debug` form of each connector request a client without the guard makes for `urls`.
fn connector_requests(urls: &[&str]) -> Vec<String> {
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let client = Client::builder()
        .no_proxy()
        .connector_layer(Recording(Arc::clone(&recorded)))
        .build()
        .expect("build recording client");
    block_on(async {
        for url in urls {
            let _ = client.get(*url).send().await;
        }
    });
    let recorded = recorded.lock().unwrap_or_else(PoisonError::into_inner);
    recorded.clone()
}

/// Records each connector request and fails it without calling the connector it wraps.
#[derive(Clone)]
struct Recording(Arc<Mutex<Vec<String>>>);

impl<S> Layer<S> for Recording {
    type Service = RecordingService<S>;

    fn layer(&self, _inner: S) -> Self::Service {
        RecordingService {
            recorded: Arc::clone(&self.0),
            connector: PhantomData,
        }
    }
}

struct RecordingService<S> {
    recorded: Arc<Mutex<Vec<String>>>,
    connector: PhantomData<fn() -> S>,
}

impl<S> Clone for RecordingService<S> {
    fn clone(&self) -> Self {
        Self {
            recorded: Arc::clone(&self.recorded),
            connector: PhantomData,
        }
    }
}

impl<S, R> Service<R> for RecordingService<S>
where
    S: Service<R, Error = BoxError>,
    R: Debug,
{
    type Response = S::Response;
    type Error = BoxError;
    type Future = Ready<Result<S::Response, BoxError>>;

    fn poll_ready(&mut self, _context: &mut Context<'_>) -> Poll<Result<(), BoxError>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: R) -> Self::Future {
        self.recorded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(format!("{request:?}"));
        future::ready(Err("recorded without connecting".into()))
    }
}

#[test]
fn the_shared_client_refuses_non_loopback_hosts_before_connecting() {
    let client = build_client(true).expect("build client");

    let refusals = block_on(async {
        let mut refusals = Vec::new();
        for (url, host) in [
            ("http://forager-guard.example/path", "forager-guard.example"),
            ("http://192.0.2.1:8080/path", "192.0.2.1"),
        ] {
            let error = client
                .get(url)
                .send()
                .await
                .expect_err("non-loopback request must fail");
            refusals
                .push(error_chain(&error).contains(&format!("refused non-loopback host {host}")));
        }
        refusals
    });

    assert_eq!(refusals, [true, true]);
}

fn error_chain(error: &dyn Error) -> String {
    let mut chain = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        chain.push_str(": ");
        chain.push_str(&cause.to_string());
        source = cause.source();
    }
    chain
}
