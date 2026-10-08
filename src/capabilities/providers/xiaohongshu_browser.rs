//! The `xiaohongshu_browser` route: Xiaohongshu search, read in the user's own logged-in Chrome
//! through the forager OpenCLI adapter `forager-xhs`.
//!
//! The JavaScript adapter only navigates, hovers, clicks, and scrolls, and reports what the page
//! itself requested and received. This route classifies the page facts, checks that the
//! responses answer the requested conditions, and decodes them into items.

use std::time::Duration;

use crate::catalog::{PlatformOperation, ProviderId, ProviderTransport, registration};
use crate::config::ProcessRouteRuntimeConfig;
use crate::net::RetryPolicy;
use crate::providers::execution::ExecutionSettings;
use crate::providers::opencli::OpenCliCommand;
use crate::rate_limit::RateLimiter;
use crate::types::{AttemptTarget, Deadline, Platform};

const ROUTE: ProviderId = ProviderId::XiaohongshuBrowser;

#[path = "xiaohongshu_browser_search.rs"]
mod search;

pub(crate) use search::search_support;

pub(crate) struct XiaohongshuBrowser {
    config: ProcessRouteRuntimeConfig,
    limiter: RateLimiter,
    deadline: Deadline,
}

impl XiaohongshuBrowser {
    pub(crate) fn new(
        config: ProcessRouteRuntimeConfig,
        limiter: RateLimiter,
        deadline: Deadline,
    ) -> Self {
        Self {
            config,
            limiter,
            deadline,
        }
    }

    fn command(
        &self,
        command: &'static str,
        options: Vec<(&'static str, String)>,
    ) -> OpenCliCommand<'_> {
        let ProviderTransport::OpenCli(adapter) = registration(ROUTE).transport else {
            unreachable!("xiaohongshu_browser registers an OpenCLI transport");
        };
        OpenCliCommand {
            executable: &self.config.command,
            adapter,
            command,
            options,
        }
    }

    // The route never retries: repeating a blocked page pushes the account further into risk
    // control.
    fn settings(&self, operation: PlatformOperation) -> ExecutionSettings {
        ExecutionSettings {
            provider: ROUTE.name(),
            target: AttemptTarget::platform(Platform::Xiaohongshu.as_str(), operation.as_str()),
            retry_policy: RetryPolicy::new(1, 1.0, Duration::ZERO),
            deadline: self.deadline,
            attempt_timeout: Duration::from_secs(self.config.timeout_seconds),
            verbose: false,
            timeout_message: "OpenCLI command timed out",
            model: None,
            transport: Some("process"),
            endpoint_host: None,
            breaker_event: None,
        }
    }
}
