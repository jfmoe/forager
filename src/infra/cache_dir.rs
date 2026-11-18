//! forager's cache directory: downloaded material that forager can fetch again, such as the
//! Managed Browser.
//!
//! The cache lives in `$XDG_CACHE_HOME/forager`, or `$HOME/.cache/forager` when
//! `XDG_CACHE_HOME` is not an absolute path.

use std::env;
use std::path::PathBuf;

/// Returns the cache directory, or `None` when neither `XDG_CACHE_HOME` nor `HOME` is an
/// absolute path.
pub(crate) fn cache_directory() -> Option<PathBuf> {
    absolute_env("XDG_CACHE_HOME")
        .map(|path| path.join("forager"))
        .or_else(|| absolute_env("HOME").map(|path| path.join(".cache/forager")))
}

fn absolute_env(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}
