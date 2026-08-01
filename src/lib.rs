#![forbid(unsafe_code)]

//! Core application services for the `forager` CLI.

pub mod app;
// PROTOTYPE (issue #44): throwaway signature skeleton, delete with the branch.
mod chain_prototype;
mod classifier;
pub mod config;
mod credentials;
mod doctor;
mod engine;
mod journal;
mod net;
mod providers;
mod research;
mod smoke;
pub mod types;
