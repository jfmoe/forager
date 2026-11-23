#![forbid(unsafe_code)]

//! Core application services for the `forager` CLI.

#[path = "cli/app.rs"]
pub mod app;
#[path = "evidence/attempt_log.rs"]
mod attempt_log;
#[path = "core/attempt_trace.rs"]
mod attempt_trace;
#[path = "ops/browser_setup.rs"]
mod browser_setup;
#[path = "infra/cache_dir.rs"]
mod cache_dir;
#[path = "capabilities/catalog.rs"]
mod catalog;
#[path = "core/chain.rs"]
mod chain;
#[path = "infra/config/mod.rs"]
pub mod config;
#[path = "capabilities/credentials.rs"]
mod credentials;
#[path = "ops/doctor.rs"]
mod doctor;
#[path = "core/engine.rs"]
mod engine;
// The file lives with the test support under `tests/` because the in-crate tests and the
// command contract tests share this one implementation.
#[cfg(test)]
#[expect(
    dead_code,
    reason = "the in-crate tests use only some of the fixture server's modes"
)]
#[path = "../tests/support/fixture_server.rs"]
mod fixture_server;
#[path = "evidence/journal.rs"]
mod journal;
#[path = "capabilities/net.rs"]
mod net;
#[path = "core/page_cursor.rs"]
mod page_cursor;
#[path = "core/platform_chain.rs"]
mod platform_chain;
#[cfg(test)]
#[path = "core/platform_checklist.rs"]
mod platform_checklist;
#[path = "core/platform_fetch.rs"]
mod platform_fetch;
#[path = "core/platform_operation.rs"]
mod platform_operation;
#[path = "core/platform_precheck.rs"]
mod platform_precheck;
#[path = "capabilities/providers/mod.rs"]
mod providers;
#[path = "capabilities/rate_limit.rs"]
mod rate_limit;
#[path = "infra/redact.rs"]
mod redact;
// Shared with the command contract tests, like the fixture server.
#[cfg(test)]
#[path = "../tests/support/samples.rs"]
mod samples;
#[path = "core/search_fanout.rs"]
mod search_fanout;
#[path = "infra/secure_fs.rs"]
mod secure_fs;
#[path = "ops/smoke.rs"]
mod smoke;
#[path = "infra/state_file.rs"]
mod state_file;
#[path = "infra/types/mod.rs"]
pub mod types;
