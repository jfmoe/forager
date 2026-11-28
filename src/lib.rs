#![forbid(unsafe_code)]

//! Core application services for the `forager` CLI.

#[path = "cli/app.rs"]
pub mod app;
#[cfg(test)]
#[path = "../tests/support/arxiv_api_samples.rs"]
mod arxiv_api_samples;
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
#[cfg(test)]
#[path = "../tests/support/exa_samples.rs"]
mod exa_samples;
#[cfg(test)]
#[path = "capabilities/fixture_server.rs"]
mod fixture_server;
#[cfg(test)]
#[path = "../tests/support/gemini_api_samples.rs"]
mod gemini_api_samples;
#[path = "ops/health.rs"]
mod health;
#[cfg(test)]
#[path = "../tests/support/hn_api_samples.rs"]
mod hn_api_samples;
#[path = "evidence/journal.rs"]
mod journal;
#[path = "ops/live.rs"]
mod live;
#[cfg(test)]
#[path = "../tests/support/main_search_samples.rs"]
mod main_search_samples;
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
#[cfg(test)]
#[path = "../tests/support/polymarket_api_samples.rs"]
mod polymarket_api_samples;
#[path = "capabilities/providers/mod.rs"]
mod providers;
#[path = "capabilities/rate_limit.rs"]
mod rate_limit;
#[path = "infra/record_directory.rs"]
mod record_directory;
#[path = "infra/redact.rs"]
mod redact;
// The sample reader, shared with the command contract tests like the fixture server, as is each
// `*_samples` module of variants derived from the stored samples.
#[cfg(test)]
#[path = "../tests/support/samples.rs"]
mod samples;
#[path = "core/search_fanout.rs"]
mod search_fanout;
#[path = "infra/secure_fs.rs"]
mod secure_fs;
#[cfg(test)]
#[path = "../tests/support/ssrn_crossref_samples.rs"]
mod ssrn_crossref_samples;
#[path = "infra/state_file.rs"]
mod state_file;
#[path = "evidence/telemetry.rs"]
mod telemetry;
#[path = "infra/types/mod.rs"]
pub mod types;
#[cfg(test)]
#[path = "capabilities/web_samples.rs"]
mod web_samples;
#[cfg(test)]
#[path = "../tests/support/youtube_api_samples.rs"]
mod youtube_api_samples;
