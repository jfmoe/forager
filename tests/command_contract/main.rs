//! The command contract tests, organised by domain: what only the real binary, spawned through
//! the shared test environment, can show. docs/agents/testing.md says what belongs here and how a
//! module's name lets one filter select a provider's or a platform's tests.

#[path = "../support/mod.rs"]
mod support;

mod access_windows;
mod browser;
mod cli;
mod config;
mod doctor;
mod environment;
mod platforms;
mod web;
