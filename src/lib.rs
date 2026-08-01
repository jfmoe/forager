#![forbid(unsafe_code)]

//! Core application services for the `forager` CLI.

pub mod app;
mod classifier;
pub mod config;
mod credentials;
mod doctor;
mod engine;
mod journal;
mod net;
mod providers;
mod redact;
mod research;
mod secure_fs;
mod smoke;
pub mod types;
