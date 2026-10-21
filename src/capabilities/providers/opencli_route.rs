//! The OpenCLI invocation bound to one process route: the route identity and its adapter, the
//! route configuration (`command`, `timeout`), its access limiter, and the request deadline.
//! Route adapters build commands, execution settings, and page-fact reads through it; the page
//! facts themselves and how to judge them stay with each route.

use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::catalog::{OpenCliAdapter, ProviderId, ProviderTransport, registration};
use crate::config::ProcessRouteRuntimeConfig;
use crate::net::{AttemptFailure, RetryPolicy};
use crate::providers::execution::ExecutionSettings;
use crate::providers::opencli::{
    self, CommandFailure, Envelope, EnvelopeStatus, OpenCliCommand, Window,
};
use crate::rate_limit::RateLimiter;
use crate::types::{AttemptErrorKind, AttemptTarget, Deadline};

pub(crate) struct OpenCliRoute {
    route: ProviderId,
    adapter: OpenCliAdapter,
    config: ProcessRouteRuntimeConfig,
    limiter: RateLimiter,
    deadline: Deadline,
}

impl OpenCliRoute {
    /// Binds the invocation to `route`, whose registration declares an OpenCLI transport.
    pub(crate) fn new(
        route: ProviderId,
        config: ProcessRouteRuntimeConfig,
        limiter: RateLimiter,
        deadline: Deadline,
    ) -> Self {
        let ProviderTransport::OpenCli(adapter) = registration(route).transport else {
            unreachable!("{} registers an OpenCLI transport", route.name());
        };
        Self {
            route,
            adapter,
            config,
            limiter,
            deadline,
        }
    }

    /// One command of the route's adapter, run with the configured executable in `window`.
    pub(crate) fn command(
        &self,
        command: &'static str,
        options: Vec<(&'static str, String)>,
        window: Window,
    ) -> OpenCliCommand<'_> {
        OpenCliCommand {
            executable: &self.config.command,
            adapter: self.adapter,
            command,
            options,
            window,
        }
    }

    /// The execution settings of one route operation: each attempt runs one OpenCLI command
    /// within the configured timeout and the request deadline.
    pub(crate) fn settings(
        &self,
        target: AttemptTarget,
        retry_policy: RetryPolicy,
    ) -> ExecutionSettings {
        ExecutionSettings {
            provider: self.route.name(),
            target,
            retry_policy,
            deadline: self.deadline,
            attempt_timeout: Duration::from_secs(self.config.timeout_seconds),
            verbose: false,
            timeout_message: "OpenCLI command timed out",
            model: None,
            transport: Some("process"),
            endpoint_host: None,
        }
    }

    /// Runs one adapter command and returns its envelope, for a caller that tells the site's
    /// own `no_results` answer apart from page facts.
    ///
    /// # Errors
    ///
    /// Returns the transport failure of [`opencli::run`].
    pub(crate) async fn run<T: DeserializeOwned>(
        &self,
        command: &OpenCliCommand<'_>,
        deadline: Deadline,
    ) -> Result<Envelope<T>, CommandFailure> {
        opencli::run(command, &self.limiter, deadline).await
    }

    /// Runs one adapter command and returns the page facts it reported.
    ///
    /// # Errors
    ///
    /// Returns the transport failure of [`opencli::run`], or Runtime when the adapter reports
    /// `no_results` instead of page facts.
    pub(crate) async fn read<T: DeserializeOwned>(
        &self,
        command: &OpenCliCommand<'_>,
        deadline: Deadline,
    ) -> Result<T, CommandFailure> {
        let envelope = self.run::<T>(command, deadline).await?;
        if envelope.status != EnvelopeStatus::Ok {
            return Err(AttemptFailure {
                kind: AttemptErrorKind::Runtime,
                status: None,
                message: format!(
                    "the {} adapter reported no results instead of page facts",
                    self.adapter.site
                ),
            }
            .into());
        }
        Ok(envelope.data)
    }
}

/// The retry policy of an operation that runs exactly one attempt.
pub(crate) fn single_attempt() -> RetryPolicy {
    RetryPolicy::new(1, 1.0, Duration::ZERO)
}
