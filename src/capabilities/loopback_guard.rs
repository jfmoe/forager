//! Under the test build, the shared HTTP client refuses every host that is not loopback, so an
//! in-crate test that misses a fixture fails instead of reaching a real site.
//!
//! The check sits in the connector because that is the one point that sees the connection
//! target for every request, IP literals included; a DNS resolver never sees an IP literal.
//! reqwest seals the connector's request type, so the target is read from its `Debug` form,
//! and a target that cannot be read is refused.

use std::fmt::Debug;
use std::future::{self, Future};
use std::net::IpAddr;
use std::pin::Pin;
use std::task::{Context, Poll};

use reqwest::{ClientBuilder, Url};
use tower_layer::Layer;
use tower_service::Service;

// The error type reqwest's `connector_layer` requires; it never reaches attribution.
type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Applies the guard and drops proxies from the environment: a proxy resolves the target host
/// itself, so the connector would only ever see the proxy's address.
pub(super) fn apply(builder: ClientBuilder) -> ClientBuilder {
    builder.no_proxy().connector_layer(LoopbackOnlyLayer)
}

#[derive(Clone)]
struct LoopbackOnlyLayer;

impl<S> Layer<S> for LoopbackOnlyLayer {
    type Service = LoopbackOnly<S>;

    fn layer(&self, inner: S) -> Self::Service {
        LoopbackOnly(inner)
    }
}

#[derive(Clone)]
struct LoopbackOnly<S>(S);

impl<S, R> Service<R> for LoopbackOnly<S>
where
    S: Service<R, Error = BoxError>,
    S::Future: Send + 'static,
    S::Response: Send + 'static,
    R: Debug,
{
    type Response = S::Response;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<S::Response, BoxError>> + Send>>;

    fn poll_ready(&mut self, context: &mut Context<'_>) -> Poll<Result<(), BoxError>> {
        self.0.poll_ready(context)
    }

    fn call(&mut self, request: R) -> Self::Future {
        match refused_target(&format!("{request:?}")) {
            Some(target) => Box::pin(future::ready(Err(format!(
                "in-crate tests may reach only loopback hosts; refused non-loopback host {target}"
            )
            .into()))),
            None => Box::pin(self.0.call(request)),
        }
    }
}

/// The host to refuse in a connector request's `Debug` form, which wraps the target URI in
/// parentheses; `None` when the target is loopback.
fn refused_target(request: &str) -> Option<String> {
    let Some(url) = request
        .split_once('(')
        .and_then(|(_, rest)| rest.strip_suffix(')'))
        .and_then(|uri| Url::parse(uri).ok())
    else {
        return Some(request.to_owned());
    };
    let host = url.host_str().unwrap_or_default();
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    let loopback = bare.eq_ignore_ascii_case("localhost")
        || bare
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    (!loopback).then(|| host.to_owned())
}

#[path = "loopback_guard_tests.rs"]
mod tests;
