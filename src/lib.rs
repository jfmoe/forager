#![forbid(unsafe_code)]

//! Core application services for the `forager` CLI.

#[path = "cli/app.rs"]
pub mod app;
#[path = "evidence/attempt_log.rs"]
mod attempt_log;
#[path = "core/attempt_trace.rs"]
mod attempt_trace;
#[path = "capabilities/catalog.rs"]
mod catalog;
#[path = "core/chain.rs"]
mod chain;
#[path = "core/classifier.rs"]
mod classifier;
#[path = "infra/config/mod.rs"]
pub mod config;
#[path = "capabilities/credentials.rs"]
mod credentials;
#[path = "ops/doctor.rs"]
mod doctor;
#[path = "core/engine.rs"]
mod engine;
#[path = "evidence/journal.rs"]
mod journal;
#[path = "capabilities/net.rs"]
mod net;
#[path = "capabilities/providers/mod.rs"]
mod providers;
#[path = "infra/redact.rs"]
mod redact;
#[path = "evidence/research.rs"]
mod research;
#[path = "infra/secure_fs.rs"]
mod secure_fs;
#[path = "ops/smoke.rs"]
mod smoke;
#[path = "infra/types.rs"]
pub mod types;
